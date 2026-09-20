//! Synthetic browser: all requests go through a supplied transport, never the host.
//!
//! A tab's history holds documents of two kinds: native `Page` JSON, drawn by
//! `page_scene`, and HTML (`text/html`, `text/plain` shown as preformatted text, and
//! pictures shown on their own) rendered by the `cw_web` engine through
//! `web_document`. Every action (`click`, `fill`, `text`, `key`, `submit`, scrolling,
//! hover) works on both.
use cw_protocol::{
    HttpRequest, HttpResponse, Page, PageAction, PageElement, Result, SimError, PAGE_MEDIA_TYPE,
};
#[cfg(test)]
use cw_scene::Primitive;
use cw_scene::{AxNode, Scene};
mod page_scene;
mod script_driver;
pub mod scripted;
pub mod web_document;
pub use scripted::{ConsoleEntry, PendingNav};
pub use web_document::{Inputs, Outcome, SheetSource, WebDocument};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

pub use cw_protocol::RGBA_MEDIA_TYPE;
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;
/// The most a stylesheet or a document fetched as a subresource may weigh.
const MAX_TEXT_RESOURCE_BYTES: usize = 2 * 1024 * 1024;
/// A portable pixel asset; integer RGBA8, row-major, straight alpha.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageAsset {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl ImageAsset {
    pub fn validate(&self) -> Result<()> {
        let expected = u64::from(self.width) * u64::from(self.height) * 4;
        if self.width == 0
            || self.height == 0
            || expected > MAX_IMAGE_BYTES as u64
            || expected != self.rgba.len() as u64
        {
            return Err(SimError::invalid("invalid or oversized RGBA image"));
        }
        Ok(())
    }
}
use url::Url;

/// What a history entry shows: a native page, or an HTML document. Both variants are
/// large and every entry holds exactly one, so neither is worth boxing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Content {
    #[serde(rename = "page")]
    Page(Page),
    #[serde(rename = "document")]
    Web(WebDocument),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    #[serde(flatten)]
    pub content: Content,
    pub status: u16,
    #[serde(default)]
    pub images: BTreeMap<String, Arc<ImageAsset>>,
    #[serde(default)]
    pub image_errors: BTreeMap<String, String>,
    /// The response asked to be fetched again after a while (`refresh: <s>; url=<path>`),
    /// as a page whose content moves with the world clock does: a music player's bar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<Refresh>,
}
impl HistoryEntry {
    /// The native page, when the entry is one.
    pub fn page(&self) -> Option<&Page> {
        match &self.content {
            Content::Page(p) => Some(p),
            Content::Web(_) => None,
        }
    }
    /// The HTML document, when the entry is one.
    pub fn web(&self) -> Option<&WebDocument> {
        match &self.content {
            Content::Web(w) => Some(w),
            Content::Page(_) => None,
        }
    }
    pub fn web_mut(&mut self) -> Option<&mut WebDocument> {
        match &mut self.content {
            Content::Web(w) => Some(w),
            Content::Page(_) => None,
        }
    }
    /// The document's title: a page's, or the HTML `<title>`.
    pub fn title(&self) -> &str {
        match &self.content {
            Content::Page(p) => &p.title,
            Content::Web(w) => &w.title,
        }
    }
}
/// A page's own refresh: `url` is requested again (`REFRESH_HEADER` set, so the site
/// can tell it from a visit) once `after_ms` of world time has passed since `fetched`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refresh {
    pub after_ms: u64,
    pub url: String,
    /// World tick the page was first seen or last refreshed at (`refresh_due` stamps it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched: Option<u64>,
}
pub use cw_protocol::REFRESH_HEADER;
/// Refresh intervals a page may ask for, in milliseconds of world time.
const REFRESH_MIN_MS: u64 = 1_000;
const REFRESH_MAX_MS: u64 = 3_600_000;
/// `Refresh: 1; url=/lyrics`, as browsers read it.
fn parse_refresh(value: &str, page_url: &Url) -> Option<Refresh> {
    let mut parts = value.split(';');
    let seconds: f64 = parts.next()?.trim().parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let after_ms = ((seconds * 1000.0) as u64).clamp(REFRESH_MIN_MS, REFRESH_MAX_MS);
    let url = match parts.next().map(str::trim) {
        Some(rest) => {
            let (key, target) = rest.split_once('=')?;
            if !key.trim().eq_ignore_ascii_case("url") {
                return None;
            }
            page_url
                .join(target.trim().trim_matches(['\'', '"']))
                .ok()?
        }
        None => page_url.clone(),
    };
    // A page refreshes itself, never another site.
    (url.origin() == page_url.origin()).then(|| Refresh {
        after_ms,
        url: url.to_string(),
        fetched: None,
    })
}
/// The page a browser shows for a request that never reached a server, in the words
/// and with the error code Chrome uses. `None` for failures that are not the
/// network's (a malformed request, a refused capability), which stay bare errors.
fn unreachable_page(url: &Url, error: &SimError) -> Option<Page> {
    let host = url.host_str().unwrap_or("this site");
    let (code, detail) = match error.code.as_str() {
        "dns" => (
            "ERR_NAME_NOT_RESOLVED",
            format!("{host}'s server IP address could not be found."),
        ),
        "connection_refused" => (
            "ERR_CONNECTION_REFUSED",
            format!("{host} refused to connect."),
        ),
        "unreachable" => (
            "ERR_ADDRESS_UNREACHABLE",
            format!("{host} is unreachable from this computer."),
        ),
        "network_denied" => (
            "ERR_NETWORK_ACCESS_DENIED",
            format!("Access to {host} is blocked by this computer's network policy."),
        ),
        "packet_loss" | "timeout" => (
            "ERR_CONNECTION_TIMED_OUT",
            format!("{host} took too long to respond."),
        ),
        _ => return None,
    };
    let mut page = Page::new(host);
    page.elements = vec![
        PageElement::Spacer {
            id: "error-lead".into(),
            height: 48,
        },
        PageElement::Heading {
            id: "error-title".into(),
            text: "This site can't be reached".into(),
            level: 1,
        },
        PageElement::Text {
            id: "error-detail".into(),
            text: detail,
        },
        PageElement::Text {
            id: "error-advice".into(),
            text: "Try:\n• Checking the connection\n• Checking the proxy and the firewall\n• Running Network Diagnostics".into(),
        },
        PageElement::Styled {
            id: "error-code".into(),
            text: code.into(),
            style: cw_protocol::Style::default().size(12).color("#5f6368").mono(),
        },
        PageElement::Button {
            id: "error-reload".into(),
            text: "Reload".into(),
            action: PageAction {
                method: "GET".into(),
                url: url.to_string(),
                fields: BTreeMap::new(),
            },
            style: None,
        },
    ];
    Some(page)
}
/// A site's own 404 as a page rather than its JSON, titled with the site the way a
/// browser's tab is. API paths keep their JSON: a program reading them wants the body.
fn not_found_page(url: &Url, response: &HttpResponse) -> Option<Page> {
    if response.status != 404 || url.path().starts_with("/api/") {
        return None;
    }
    let host = url.host_str().unwrap_or("this site");
    let message = serde_json::from_slice::<serde_json::Value>(&response.body)
        .ok()
        .and_then(|v| v.get("error")?.as_str().map(str::to_owned))
        .filter(|m| !m.is_empty() && m != "route not found");
    let mut page = Page::new(host);
    page.elements = vec![
        PageElement::Spacer {
            id: "error-lead".into(),
            height: 48,
        },
        PageElement::Heading {
            id: "error-title".into(),
            text: "404 Not Found".into(),
            level: 1,
        },
        PageElement::Text {
            id: "error-detail".into(),
            text: format!("There is no page at {} on {host}.", url.path()),
        },
    ];
    if let Some(message) = message {
        page.elements.push(PageElement::Text {
            id: "error-message".into(),
            text: message,
        });
    }
    page.elements.push(PageElement::Link {
        id: "error-home".into(),
        text: format!("Go to {host}"),
        url: format!("{}://{host}/", url.scheme()),
        style: None,
    });
    Some(page)
}
/// The media type of a response, lower-cased, without parameters.
fn media_type(response: &HttpResponse) -> String {
    response
        .header("content-type")
        .and_then(|v| v.split(';').next())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}
/// Decodes a picture response into an asset: the native RGBA JSON, PNG or JPEG.
fn decode_image(media_type: &str, body: &[u8]) -> Result<ImageAsset> {
    let asset = match media_type {
        RGBA_MEDIA_TYPE => serde_json::from_slice::<ImageAsset>(body)?,
        "image/png" => decode_png(body).ok_or_else(|| SimError::new("image_format", "undecodable PNG"))?,
        "image/jpeg" | "image/jpg" => {
            decode_jpeg(body).ok_or_else(|| SimError::new("image_format", "undecodable JPEG"))?
        }
        _ => {
            return Err(SimError::new(
                "image_format",
                format!("unsupported image media type {media_type}"),
            ))
        }
    };
    asset.validate()?;
    Ok(asset)
}
fn decode_png(bytes: &[u8]) -> Option<ImageAsset> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    if u64::from(reader.info().width) * u64::from(reader.info().height) * 4 > MAX_IMAGE_BYTES as u64 {
        return None;
    }
    let mut raw = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut raw).ok()?;
    let raw = &raw[..info.buffer_size()];
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(raw),
        png::ColorType::Rgb => {
            for p in raw.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for p in raw {
                rgba.extend_from_slice(&[*p, *p, *p, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in raw.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        png::ColorType::Indexed => return None,
    }
    Some(ImageAsset {
        width: info.width,
        height: info.height,
        rgba,
    })
}
fn decode_jpeg(bytes: &[u8]) -> Option<ImageAsset> {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    if u64::from(info.width) * u64::from(info.height) * 4 > MAX_IMAGE_BYTES as u64 {
        return None;
    }
    let pixels = decoder.decode().ok()?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => {
            for p in pixels.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        jpeg_decoder::PixelFormat::L8 => {
            for p in &pixels {
                rgba.extend_from_slice(&[*p, *p, *p, 255]);
            }
        }
        jpeg_decoder::PixelFormat::L16 => {
            for p in pixels.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[p[0], p[0], p[0], 255]);
            }
        }
        jpeg_decoder::PixelFormat::CMYK32 => {
            for p in pixels.as_chunks::<4>().0 {
                let k = u32::from(p[3]);
                let c = |v: u8| (u32::from(v) * k / 255) as u8;
                rgba.extend_from_slice(&[c(p[0]), c(p[1]), c(p[2]), 255]);
            }
        }
    }
    Some(ImageAsset { width, height, rgba })
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    pub history: Vec<HistoryEntry>,
    pub position: usize,
    pub focused: Option<String>,
    pub fields: BTreeMap<String, String>,
    pub scroll_y: i32,
    /// How far each sideways-scrolling row of the page is scrolled, by row id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scroll_x: BTreeMap<String, i32>,
    /// Stable identity of the tab (indices shift when a tab closes): names the tab's
    /// page-script entropy stream. The first tab is 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub id: u64,
    /// What the tab's pages wrote to the console, oldest first, capped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub console: Vec<ConsoleEntry>,
    /// `sessionStorage`, by origin: lives and dies with the tab.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub session_storage: BTreeMap<String, BTreeMap<String, String>>,
    /// The tab's page-script entropy stream (`Math.random`, `crypto`), once drawn from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entropy: Option<cw_determinism::Determinism>,
}
fn is_zero(v: &u64) -> bool {
    *v == 0
}
fn is_false(v: &bool) -> bool {
    !*v
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub path: String,
    pub secure: bool,
    /// Hidden from `document.cookie`, as `HttpOnly` asks.
    #[serde(default, skip_serializing_if = "is_false")]
    pub http_only: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserState {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Origin-scoped storage; origin includes scheme, host, and port.
    pub storage: BTreeMap<String, BTreeMap<String, String>>,
    pub cookies: BTreeMap<String, Vec<Cookie>>,
    pub pending: Option<HttpRequest>,
    #[serde(default)]
    pub image_cache: BTreeMap<String, Arc<ImageAsset>>,
    /// Page zoom in percent, remembered per site the way Safari and Chrome remember
    /// it. A site that is not listed is shown at 100%.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub zoom: BTreeMap<String, u16>,
    /// World clock (microseconds) page scripts see; the environment sets it before it
    /// acts and on every tick (`set_clock`, `tick`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub clock: u64,
    /// The world seed and the scope page-script entropy streams are named under
    /// (`<scope>/tab/<id>/page-script`), set by the environment (`set_entropy`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub entropy_seed: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub entropy_scope: String,
    /// The content viewport in scene px scripted pages are laid out for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<(u32, u32)>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub next_tab_id: u64,
    /// Show every HTML document through a realm, script or not (tests compare the
    /// two paths).
    #[serde(skip)]
    pub always_script: bool,
    /// Script-initiated navigations in flight, to stop a page that redirects forever.
    #[serde(skip)]
    nav_depth: u32,
    /// Navigations a document queued while it loaded, performed once it is committed.
    #[serde(skip)]
    load_navs: Vec<PendingNav>,
}
/// The zoom levels a browser steps through, in percent.
pub const ZOOM_LEVELS: [u16; 11] = [50, 75, 85, 100, 115, 125, 150, 175, 200, 250, 300];
impl Default for BrowserState {
    fn default() -> Self {
        Self {
            tabs: vec![Tab::default()],
            active: 0,
            storage: BTreeMap::new(),
            cookies: BTreeMap::new(),
            pending: None,
            image_cache: BTreeMap::new(),
            zoom: BTreeMap::new(),
            clock: 0,
            entropy_seed: 0,
            entropy_scope: String::new(),
            viewport: None,
            next_tab_id: 0,
            always_script: false,
            nav_depth: 0,
            load_navs: Vec::new(),
        }
    }
}
impl BrowserState {
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }
    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }
    fn entry(&self) -> Option<&HistoryEntry> {
        self.tab().history.get(self.tab().position)
    }
    fn entry_mut(&mut self) -> Option<&mut HistoryEntry> {
        let position = self.tab().position;
        self.tab_mut().history.get_mut(position)
    }
    /// The native page on show; `None` for an empty tab or an HTML document (see
    /// `document` and `current_page`).
    pub fn page(&self) -> Option<&Page> {
        self.entry().and_then(HistoryEntry::page)
    }
    /// The HTML document on show, when the tab shows one.
    pub fn document(&self) -> Option<&WebDocument> {
        self.entry().and_then(HistoryEntry::web)
    }
    pub fn document_mut(&mut self) -> Option<&mut WebDocument> {
        self.entry_mut().and_then(HistoryEntry::web_mut)
    }
    /// The title of the document on show, when there is one.
    pub fn title(&self) -> Option<String> {
        self.entry().map(|e| e.title().to_owned())
    }
    /// The document on show as a page: the native page itself, or an HTML document
    /// projected into headings, text, links and controls with its live values.
    pub fn current_page(&self) -> Option<Cow<'_, Page>> {
        match &self.entry()?.content {
            Content::Page(p) => Some(Cow::Borrowed(p)),
            Content::Web(w) => Some(Cow::Owned(w.to_page(&self.tab().fields))),
        }
    }
    /// Whether `id` names a text control of the document on show.
    pub fn has_input(&self, id: &str) -> bool {
        match self.entry().map(|e| &e.content) {
            Some(Content::Page(p)) => {
                let mut found = false;
                walk(&p.elements, &mut |e| {
                    if let PageElement::Input { id: i, .. } = e {
                        found |= i == id;
                    }
                });
                found
            }
            Some(Content::Web(_)) => self.tab().fields.contains_key(id),
            None => false,
        }
    }
    /// The site a zoom level belongs to: scheme, host and port of the page on screen.
    fn zoom_site(&self) -> Option<String> {
        let url = self.url()?;
        let (scheme, rest) = url.split_once("://")?;
        let host = rest.split(['/', '?', '#']).next()?;
        Some(format!("{scheme}://{host}"))
    }
    /// Zoom of the page on screen, in percent.
    pub fn zoom(&self) -> u16 {
        self.zoom_site()
            .and_then(|site| self.zoom.get(&site).copied())
            .unwrap_or(100)
    }
    /// Step the page on screen's zoom `in`, `out`, or `reset` it to 100%, returning the
    /// new level. Refused when no page is showing.
    pub fn step_zoom(&mut self, step: &str) -> Result<u16> {
        let site = self
            .zoom_site()
            .ok_or_else(|| SimError::invalid("no page to zoom"))?;
        let now = self.zoom();
        let next = match step {
            "in" => ZOOM_LEVELS
                .iter()
                .copied()
                .find(|z| *z > now)
                .unwrap_or(now),
            "out" => ZOOM_LEVELS
                .iter()
                .rev()
                .copied()
                .find(|z| *z < now)
                .unwrap_or(now),
            "reset" => 100,
            other => return Err(SimError::invalid(format!("unknown zoom step {other}"))),
        };
        if next == 100 {
            self.zoom.remove(&site);
        } else {
            self.zoom.insert(site, next);
        }
        self.sync_script_viewport();
        Ok(next)
    }
    pub fn url(&self) -> Option<&str> {
        self.entry().map(|e| e.url.as_str())
    }
    pub fn new_tab(&mut self) -> usize {
        self.script_visibility(self.active, true);
        self.next_tab_id += 1;
        self.tabs.push(Tab { id: self.next_tab_id, ..Tab::default() });
        self.active = self.tabs.len() - 1;
        self.active
    }
    pub fn switch_tab(&mut self, index: usize) -> Result<()> {
        if index >= self.tabs.len() {
            return Err(SimError::not_found("tab"));
        }
        if index != self.active {
            self.script_visibility(self.active, true);
            self.active = index;
            self.script_visibility(index, false);
        }
        Ok(())
    }
    pub fn close_tab(&mut self, index: usize) -> Result<()> {
        if index >= self.tabs.len() {
            return Err(SimError::not_found("tab"));
        }
        self.script_unload(index);
        let was_active = index == self.active;
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.tabs.push(Tab::default())
        }
        if self.active > index {
            self.active -= 1
        }
        self.active = self.active.min(self.tabs.len() - 1);
        if was_active {
            self.script_visibility(self.active, false);
        }
        Ok(())
    }
    fn resolve(&self, url: &str) -> Result<Url> {
        let base = self
            .document()
            .map(|d| d.base.clone())
            .or_else(|| self.url().map(str::to_owned));
        let parsed = Url::parse(url)
            .or_else(|_| {
                base.and_then(|base| Url::parse(&base).ok())
                    .ok_or(url::ParseError::RelativeUrlWithoutBase)?
                    .join(url)
            })
            .map_err(|e| SimError::invalid(e.to_string()))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(SimError::denied(
                "browser supports credential-free http/https URLs only",
            ));
        }
        Ok(parsed)
    }
    pub fn navigate<F>(&mut self, url: &str, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let target = self.resolve(url)?;
        self.request(HttpRequest::get(target.as_str()), transport, false)
    }
    pub fn reload<F>(&mut self, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let url = self
            .url()
            .ok_or_else(|| SimError::invalid("empty tab"))?
            .to_owned();
        self.request(HttpRequest::get(url), transport, true)
    }
    /// History traversal restores the received document without issuing a new mutation/request.
    ///
    /// A scripted page's own entries (`history.pushState`, fragment changes) come
    /// first: going back inside one fires `popstate` and leaves the document alone.
    pub fn back<F>(&mut self, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.script_history_go(-1, transport)? {
            return Ok(());
        }
        if self.tab().position == 0 {
            return Err(SimError::not_found("previous history entry"));
        }
        self.script_leave(transport);
        self.tab_mut().position -= 1;
        self.reset_fields();
        self.script_arrive(transport);
        Ok(())
    }
    pub fn forward<F>(&mut self, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.script_history_go(1, transport)? {
            return Ok(());
        }
        if self.tab().position + 1 >= self.tab().history.len() {
            return Err(SimError::not_found("next history entry"));
        }
        self.script_leave(transport);
        self.tab_mut().position += 1;
        self.reset_fields();
        self.script_arrive(transport);
        Ok(())
    }
    fn reset_fields(&mut self) {
        let fields = match self.entry().map(|e| &e.content) {
            Some(Content::Page(page)) => {
                let mut fields = BTreeMap::new();
                walk(&page.elements, &mut |e| {
                    if let PageElement::Input { id, value, .. } = e {
                        fields.insert(id.clone(), value.clone());
                    }
                });
                fields
            }
            Some(Content::Web(web)) => web.initial_fields(),
            None => BTreeMap::new(),
        };
        let (focused, scroll_y) = self.document().filter(|w| w.is_scripted()).map(|w| w.script_focus_and_scroll()).unwrap_or((None, 0));
        let tab = self.tab_mut();
        tab.fields = fields;
        tab.focused = focused;
        tab.scroll_y = scroll_y;
        tab.scroll_x.clear();
    }
    fn request<F>(&mut self, request: HttpRequest, transport: &mut F, replace: bool) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        self.request_with(request, transport, replace, replace)
    }
    /// Whether the page on show wants attention at `now`: a refresh that is due, or one
    /// not yet stamped. Read-only, so a caller can skip taking the state mutably.
    pub fn refresh_pending(&self, now: u64) -> bool {
        self.script_pending(now) || self.page_refresh_pending(now)
    }
    fn page_refresh_pending(&self, now: u64) -> bool {
        self.entry()
            .and_then(|e| e.refresh.as_ref())
            .is_some_and(|r| {
                r.fetched
                    .is_none_or(|at| now >= at.saturating_add(r.after_ms.saturating_mul(1_000)))
            })
    }
    /// Whether the page on show asked to be refreshed and has waited long enough at
    /// `now` (world microseconds). A page not yet stamped is stamped here, so its wait
    /// starts when it was first seen.
    pub fn refresh_due(&mut self, now: u64) -> bool {
        let Some(refresh) = self.entry_mut().and_then(|e| e.refresh.as_mut()) else {
            return false;
        };
        match refresh.fetched {
            None => {
                refresh.fetched = Some(now);
                false
            }
            Some(at) => now >= at.saturating_add(refresh.after_ms.saturating_mul(1_000)),
        }
    }
    /// Fetch the page on show again, as its `refresh` asked, and show what comes back in
    /// its place. What the person had typed, where the field focus was and how far the
    /// page was scrolled all stay; pictures come from the cache.
    pub fn refresh<F>(&mut self, now: u64, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let url = self
            .entry()
            .and_then(|e| e.refresh.as_ref())
            .map(|r| r.url.clone())
            .ok_or_else(|| SimError::invalid("the page asked for no refresh"))?;
        let (fields, focused, scroll, shelves) = {
            let tab = self.tab();
            (
                tab.fields.clone(),
                tab.focused.clone(),
                tab.scroll_y,
                tab.scroll_x.clone(),
            )
        };
        let mut request = HttpRequest::get(url);
        request.headers.insert(REFRESH_HEADER.into(), "1".into());
        let result = self.request_with(request, transport, true, false);
        let position = self.tab().position;
        let tab = self.tab_mut();
        for (id, value) in fields {
            if tab.fields.contains_key(&id) {
                tab.fields.insert(id, value);
            }
        }
        tab.focused = focused.filter(|f| tab.fields.contains_key(f));
        tab.scroll_y = scroll;
        tab.scroll_x = shelves;
        // Stamp the new copy (or, when the fetch failed, the old one) so the next try
        // waits its interval again rather than hammering the network every step.
        if let Some(r) = tab
            .history
            .get_mut(position)
            .and_then(|e| e.refresh.as_mut())
        {
            r.fetched = Some(now);
        }
        result
    }
    /// The cookies to send to `url`, as a header value; empty when none apply.
    fn cookie_header(&self, url: &Url) -> String {
        self.cookies
            .get(&url.origin().ascii_serialization())
            .map(|cookies| {
                cookies
                    .iter()
                    .filter(|c| {
                        cookie_path_matches(url.path(), &c.path)
                            && (!c.secure || url.scheme() == "https")
                    })
                    .map(|c| format!("{}={}", c.name, c.value))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default()
    }
    fn request_with<F>(
        &mut self,
        mut request: HttpRequest,
        transport: &mut F,
        replace: bool,
        fresh_images: bool,
    ) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        for _ in 0..=16 {
            let mut url = self.resolve(&request.url)?;
            let fragment = url.fragment().map(str::to_owned);
            url.set_fragment(None);
            request.url = url.to_string();
            let origin = url.origin().ascii_serialization();
            request.headers.remove("cookie");
            let cookies = self.cookie_header(&url);
            if !cookies.is_empty() {
                request.headers.insert("cookie".into(), cookies);
            }
            self.pending = Some(request.clone());
            let result = transport(request.clone());
            self.pending = None;
            let response = match result {
                Ok(response) => response,
                Err(error) => {
                    // A host that cannot be reached still gives the tab a page, the
                    // way Chrome's "This site can't be reached" does; the action
                    // itself still fails, so the actor learns the request never landed.
                    if let Some(page) = unreachable_page(&url, &error) {
                        self.show(url.to_string(), page, 0, replace);
                    }
                    return Err(error);
                }
            };
            if let Some(header) = response.header("set-cookie") {
                if let Some(cookie) = parse_cookie(header, url.path()) {
                    let jar = self.cookies.entry(origin).or_default();
                    jar.retain(|c| c.name != cookie.name || c.path != cookie.path);
                    if !header.to_ascii_lowercase().contains("max-age=0") {
                        jar.push(cookie);
                        jar.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));
                    }
                }
            }
            if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                if let Some(location) = response.header("location") {
                    let next = url
                        .join(location)
                        .map_err(|e| SimError::invalid(e.to_string()))?;
                    if next.origin() != url.origin() {
                        request
                            .headers
                            .retain(|k, _| !k.eq_ignore_ascii_case("authorization"));
                    }
                    request.url = next.to_string();
                    if response.status == 303
                        || (matches!(response.status, 301 | 302) && request.method == "POST")
                    {
                        request.method = "GET".into();
                        request.body.clear();
                        request.headers.remove("content-type");
                    }
                    continue;
                }
            }
            let kind = media_type(&response);
            let mut document_url = url.clone();
            document_url.set_fragment(fragment.as_deref());
            // The page on show is about to be left: `beforeunload` (its answer
            // ignored), `pagehide`, `unload`.
            self.script_leave(transport);
            let (content, images, image_errors, refresh) = if kind == PAGE_MEDIA_TYPE {
                let page: Page = serde_json::from_slice(&response.body)?;
                page.validate()?;
                let (images, image_errors) =
                    self.load_images(&page, &url, transport, fresh_images);
                let refresh = response
                    .header("refresh")
                    .and_then(|value| parse_refresh(value, &url));
                (Content::Page(page), images, image_errors, refresh)
            } else if let Some(html) = html_source(&kind, &document_url, &response) {
                // A picture shown on its own is the body that just arrived: no second
                // request for it.
                if kind.starts_with("image/") || kind == RGBA_MEDIA_TYPE {
                    if let Ok(asset) = decode_image(&kind, &response.body) {
                        self.image_cache.insert(url.to_string(), Arc::new(asset));
                    }
                }
                let web = self.load_web(&html, &document_url, transport, false);
                let refresh = response
                    .header("refresh")
                    .map(str::to_owned)
                    .or_else(|| if web.is_scripted() { web.script_meta_refresh() } else { web.meta_refresh() })
                    .and_then(|value| parse_refresh(&value, &url));
                (Content::Web(web), BTreeMap::new(), BTreeMap::new(), refresh)
            } else if let Some(page) = not_found_page(&url, &response) {
                (Content::Page(page), BTreeMap::new(), BTreeMap::new(), None)
            } else {
                let mut page = Page::new(url.as_str());
                page.elements.push(PageElement::Text {
                    id: "response".into(),
                    text: String::from_utf8_lossy(&response.body).into_owned(),
                });
                (Content::Page(page), BTreeMap::new(), BTreeMap::new(), None)
            };
            let entry = HistoryEntry {
                url: document_url.to_string(),
                content,
                status: response.status,
                images,
                image_errors,
                refresh,
            };
            self.commit(entry, replace);
            if self.document().is_some_and(WebDocument::is_scripted) {
                // What the page asked for while it loaded (`location.assign` in a
                // script) happens now that the load is over.
                return self.script_after_load(transport);
            }
            if let Some(fragment) = fragment.filter(|f| !f.is_empty()) {
                if let Some(web) = self.document_mut() {
                    if let Outcome::ScrollTo(y) = web.jump_to(&fragment) {
                        self.tab_mut().scroll_y = y;
                    }
                }
            }
            return Ok(());
        }
        Err(SimError::new("redirect_limit", "more than 16 redirects"))
    }
    /// Parses an HTML response and fetches what it links: stylesheets (and their
    /// imports) through the transport, then every picture it or its sheets reference.
    fn load_web<F>(
        &mut self,
        html: &str,
        url: &Url,
        transport: &mut F,
        fresh_images: bool,
    ) -> WebDocument
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let mut web = WebDocument::parse(html, url.as_str());
        if web.has_script() || self.always_script {
            return self.load_scripted(html, url, transport, fresh_images);
        }
        for plan in web.sheet_plan() {
            match plan {
                web_document::SheetPlan::Inline { media, source } => {
                    let base = web.base.clone();
                    web.add_sheet(&base, &media, source, &mut |u| self.fetch_text(u, transport));
                }
                web_document::SheetPlan::Linked { url: link, media } => {
                    if let Some(source) = self.fetch_text(&link, transport) {
                        web.add_sheet(&link, &media, source, &mut |u| self.fetch_text(u, transport));
                    }
                }
            }
        }
        for (reference, resolved) in web.image_references() {
            match Url::parse(&resolved) {
                Ok(target) => match self.load_image_from(target, transport, fresh_images, None) {
                    Ok(asset) => web.add_image(&reference, &resolved, asset),
                    Err(error) => web.add_image_error(&reference, &resolved, &error.code),
                },
                Err(_) => web.add_image_error(&reference, &resolved, "invalid"),
            }
        }
        web
    }
    /// Fetches a text subresource (a stylesheet): its body as a string, or `None` when
    /// the transport refused, the status was not a success or the body is not text.
    fn fetch_text<F>(&mut self, url: &str, transport: &mut F) -> Option<String>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let url = Url::parse(url).ok()?;
        if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
            return None;
        }
        let response = self.fetch_resource(url, transport).ok()?;
        if !(200..300).contains(&response.status) || response.body.len() > MAX_TEXT_RESOURCE_BYTES {
            return None;
        }
        let kind = media_type(&response);
        if kind == "text/html" || kind.starts_with("image/") || kind == PAGE_MEDIA_TYPE {
            return None;
        }
        Some(cw_web::html::decode(&response.body))
    }
    /// One subresource request with the origin's cookies, following up to eight
    /// redirects. Cross-origin is allowed: the transport enforces the network policy.
    fn fetch_resource<F>(&mut self, mut url: Url, transport: &mut F) -> Result<HttpResponse>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        for _ in 0..=8 {
            if !url.username().is_empty() || url.password().is_some() {
                return Err(SimError::denied("credentialed subresource"));
            }
            let mut request = HttpRequest::get(url.as_str());
            let cookies = self.cookie_header(&url);
            if !cookies.is_empty() {
                request.headers.insert("cookie".into(), cookies);
            }
            self.pending = Some(request.clone());
            let response = transport(request);
            self.pending = None;
            let response = response?;
            if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                let location = response
                    .header("location")
                    .ok_or_else(|| SimError::invalid("redirect lacks location"))?;
                url = url
                    .join(location)
                    .map_err(|e| SimError::invalid(e.to_string()))?;
                continue;
            }
            return Ok(response);
        }
        Err(SimError::new("redirect_limit", "subresource redirect limit"))
    }
    /// Show `page` as the tab's document at `url`, with no pictures to fetch.
    fn show(&mut self, url: String, page: Page, status: u16, replace: bool) {
        self.commit(
            HistoryEntry {
                url,
                content: Content::Page(page),
                status,
                images: BTreeMap::new(),
                image_errors: BTreeMap::new(),
                refresh: None,
            },
            replace,
        );
    }
    /// Make `entry` the tab's current document: in place of the one on show when
    /// `replace`, otherwise as a new history entry that drops any forward history.
    fn commit(&mut self, entry: HistoryEntry, replace: bool) {
        let tab = self.tab_mut();
        if replace && !tab.history.is_empty() {
            tab.history[tab.position] = entry
        } else {
            if !tab.history.is_empty() {
                tab.history.truncate(tab.position + 1)
            }
            tab.history.push(entry);
            tab.position = tab.history.len() - 1;
        }
        self.reset_fields();
    }
    /// Carries out what a document's action asked for.
    fn perform_outcome<F>(&mut self, outcome: Outcome, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        match outcome {
            Outcome::Nothing => Ok(()),
            Outcome::ScrollTo(y) => {
                // The fragment is part of the address the tab shows.
                if let Some(entry) = self.entry_mut() {
                    if let Some(url) = entry.web().map(|w| w.url.clone()) {
                        entry.url = url;
                    }
                }
                self.tab_mut().scroll_y = y;
                Ok(())
            }
            Outcome::Navigate { url, new_tab } => {
                if new_tab {
                    self.new_tab();
                }
                self.navigate(&url, transport)
            }
            Outcome::Request(request) => self.request(request, transport, false),
        }
    }
    /// The tab's document, fields and focus, split for a document action.
    fn web_parts(&mut self) -> Option<(&mut WebDocument, &mut BTreeMap<String, String>, &mut Option<String>)> {
        let tab = self.tab_mut();
        let position = tab.position;
        let Tab {
            history,
            fields,
            focused,
            ..
        } = tab;
        let web = history.get_mut(position)?.web_mut()?;
        Some((web, fields, focused))
    }
    pub fn fill(&mut self, id: &str, value: &str) -> Result<()> {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_fill::<script_driver::NoTransport>(id, value, None);
        }
        if self.document().is_some() {
            let (web, fields, focused) = self.web_parts().expect("document");
            let Some(node) = web.node_for(id) else {
                return Err(SimError::not_found(format!("input {id}")));
            };
            web.notice = None;
            let doc = web.document();
            if doc.is(node, "select") {
                web.choose(node, value)?;
                *focused = Some(web.id_of(node));
                return Ok(());
            }
            if doc.is(node, "input") && matches!(web_document::input_type(doc, node).as_str(), "checkbox" | "radio") {
                let on = matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on" | "checked" | "yes");
                let mut scratch = BTreeMap::new();
                if on != web.is_checked(node) {
                    let _ = web.click(node, &mut scratch, focused)?;
                }
                *focused = Some(web.id_of(node));
                return Ok(());
            }
            if !fields.contains_key(id) {
                return Err(SimError::not_found(format!("input {id}")));
            }
            let key = web.id_of(node);
            fields.insert(key.clone(), value.into());
            *focused = Some(key);
            web.set_caret_end();
            return Ok(());
        }
        if !self.tab().fields.contains_key(id) {
            return Err(SimError::not_found(format!("input {id}")));
        }
        self.tab_mut().fields.insert(id.into(), value.into());
        self.tab_mut().focused = Some(id.into());
        Ok(())
    }
    pub fn text(&mut self, text: &str) -> Result<()> {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_text::<script_driver::NoTransport>(text, None);
        }
        if self.document().is_some() {
            let (web, fields, focused) = self.web_parts().expect("document");
            return web.insert_text(text, fields, focused);
        }
        let id = self
            .tab()
            .focused
            .clone()
            .ok_or_else(|| SimError::invalid("no focused input"))?;
        let value = self
            .tab_mut()
            .fields
            .get_mut(&id)
            .ok_or_else(|| SimError::not_found("input"))?;
        value.push_str(text);
        Ok(())
    }
    pub fn key<F>(&mut self, key: &str, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_key(key, transport);
        }
        if self.document().is_some() {
            let (web, fields, focused) = self.web_parts().expect("document");
            let outcome = web.key(key, fields, focused)?;
            return self.perform_outcome(outcome, transport);
        }
        if key == "Tab" {
            let mut ids = vec![];
            if let Some(p) = self.page() {
                walk(&p.elements, &mut |e| {
                    if let PageElement::Input { id, .. } = e {
                        ids.push(id.clone())
                    }
                })
            }
            if ids.is_empty() {
                return Ok(());
            }
            let next = self
                .tab()
                .focused
                .as_ref()
                .and_then(|id| ids.iter().position(|x| x == id))
                .map(|i| (i + 1) % ids.len())
                .unwrap_or(0);
            self.tab_mut().focused = Some(ids[next].clone());
            return Ok(());
        }
        let id = self
            .tab()
            .focused
            .clone()
            .ok_or_else(|| SimError::invalid("no focused input"))?;
        match key {
            "Backspace" => {
                self.tab_mut().fields.get_mut(&id).unwrap().pop();
                Ok(())
            }
            "Enter" => {
                let action = self
                    .page()
                    .and_then(|p| parent_form(&p.elements, &id))
                    .cloned()
                    .ok_or_else(|| SimError::invalid("input has no form"))?;
                self.perform(action, Some(&id), transport)
            }
            _ => Err(SimError::invalid(format!("unsupported browser key {key}"))),
        }
    }
    pub fn click<F>(&mut self, id: &str, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_click(id, transport);
        }
        if self.document().is_some() {
            let (web, fields, focused) = self.web_parts().expect("document");
            let node = web
                .node_for(id)
                .ok_or_else(|| SimError::not_found(format!("element {id}")))?;
            let outcome = web.click(node, fields, focused)?;
            return self.perform_outcome(outcome, transport);
        }
        let element = self
            .page()
            .and_then(|p| find(&p.elements, id))
            .cloned()
            .ok_or_else(|| SimError::not_found(format!("element {id}")))?;
        match element {
            PageElement::Link { url, .. } => self.navigate(&url, transport),
            PageElement::Input { .. } => {
                self.tab_mut().focused = Some(id.into());
                Ok(())
            }
            PageElement::Button { action, .. } | PageElement::Form { action, .. } => {
                self.perform(action, Some(id), transport)
            }
            // Cards, thumbnails and icons are controls only when they carry a real action.
            PageElement::Card {
                action: Some(action),
                ..
            }
            | PageElement::Thumbnail {
                action: Some(action),
                ..
            }
            | PageElement::Icon {
                action: Some(action),
                ..
            }
            | PageElement::Image {
                action: Some(action),
                ..
            } => self.perform(action, Some(id), transport),
            _ => Err(SimError::invalid("element is not interactive")),
        }
    }
    /// The render inputs of the tab for a `width` x `height` viewport.
    fn inputs(&self, width: u32, height: u32) -> Inputs<'_> {
        let tab = self.tab();
        Inputs {
            width,
            height,
            zoom: self.zoom(),
            fields: &tab.fields,
            focused: tab.focused.as_deref(),
            scroll_y: tab.scroll_y,
        }
    }
    /// Clicks whatever is painted at `(x, y)` in a `width` x `height` viewport.
    pub fn click_at<F>(&mut self, x: i32, y: i32, width: u32, height: u32, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_click_at(x, y, width, height, transport);
        }
        let target = match self.document() {
            Some(web) => web.hit(x, y, self.inputs(width, height)).map(|n| web.id_of(n)),
            None => self
                .scene(width, height)
                .hit_test(x, y)
                .and_then(|n| n.interaction.clone()),
        };
        let target = target.ok_or_else(|| SimError::not_found("nothing to click there"))?;
        if target.starts_with("pane:") {
            return Ok(());
        }
        self.click(&target, transport)
    }
    /// Moves the pointer over `(x, y)` in a `width` x `height` viewport: an HTML
    /// document updates its `:hover` state and answers with the CSS cursor for that
    /// point; a native page answers `None` (its cursor is the shell's business).
    pub fn hover_at(&mut self, x: i32, y: i32, width: u32, height: u32) -> Option<&'static str> {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_hover_at::<script_driver::NoTransport>(x, y, width, height, None);
        }
        self.document()?;
        let zoom = self.zoom();
        let tab = self.tab_mut();
        let position = tab.position;
        let Tab {
            history,
            fields,
            focused,
            scroll_y,
            ..
        } = tab;
        let web = history.get_mut(position)?.web_mut()?;
        let inputs = Inputs {
            width,
            height,
            zoom,
            fields,
            focused: focused.as_deref(),
            scroll_y: *scroll_y,
        };
        Some(web.hover_at(x, y, inputs))
    }
    /// The CSS cursor over `(x, y)` of an HTML document, without moving the hover.
    pub fn cursor_at(&self, x: i32, y: i32, width: u32, height: u32) -> Option<&'static str> {
        let web = self.document()?;
        Some(web.cursor_at(x, y, self.inputs(width, height)))
    }
    /// Scrolls a pane of the document on show to `offset` (CSS px) and says whether it
    /// moved: `page` is the document itself; a native page's `row:<id>` is one of its
    /// sideways shelves; any other name is an HTML scroll container's interaction id.
    pub fn scroll_pane(&mut self, pane: &str, offset: i32, horizontal: bool) -> bool {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_scroll::<script_driver::NoTransport>(pane, offset, horizontal, None);
        }
        let offset = offset.max(0);
        let is_web = self.document().is_some();
        match (pane, horizontal, is_web) {
            ("page", false, _) => {
                let tab = self.tab_mut();
                let moved = tab.scroll_y != offset;
                tab.scroll_y = offset;
                moved
            }
            ("page", true, true) => self
                .document_mut()
                .is_some_and(|w| w.scroll_pane("page", offset, true)),
            ("page", true, false) => false,
            (other, _, true) => {
                let id = other.strip_prefix("row:").unwrap_or(other);
                self.document_mut()
                    .is_some_and(|w| w.scroll_pane(id, offset, horizontal))
            }
            (other, _, false) => match other.strip_prefix("row:") {
                Some(row) => self.tab_mut().scroll_x.insert(row.to_owned(), offset) != Some(offset),
                None => false,
            },
        }
    }
    pub fn submit<F>(&mut self, id: &str, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.document().is_some_and(WebDocument::is_scripted) {
            return self.script_submit(id, transport);
        }
        if self.document().is_some() {
            let (web, fields, _) = self.web_parts().expect("document");
            let request = web.submit(id, fields)?;
            return self.request(request, transport, false);
        }
        match self.page().and_then(|p| find(&p.elements, id)) {
            Some(PageElement::Form { action, .. }) => {
                self.perform(action.clone(), Some(id), transport)
            }
            _ => Err(SimError::not_found("form")),
        }
    }
    fn perform<F>(
        &mut self,
        action: PageAction,
        target: Option<&str>,
        transport: &mut F,
    ) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let mut fields = action.fields;
        let mut ids = vec![];
        if let (Some(page), Some(id)) = (self.page(), target) {
            let scope = find_form_scope(&page.elements, id);
            if let Some(children) = scope {
                walk(children, &mut |e| {
                    if let PageElement::Input { id, .. } = e {
                        ids.push(id.clone())
                    }
                });
            }
        }
        for id in ids {
            if let Some(value) = self.tab().fields.get(&id) {
                fields.insert(id, value.clone());
            }
        }
        // Explicit field values may reference controls outside forms using $input_id.
        for value in fields.values_mut() {
            if let Some(id) = value.strip_prefix('$') {
                if let Some(input) = self.tab().fields.get(id) {
                    *value = input.clone();
                }
            }
        }
        let mut url = self.resolve(&action.url)?;
        let method = action.method.to_ascii_uppercase();
        let mut request = HttpRequest::get(url.as_str());
        request.method = method.clone();
        if method == "GET" {
            // A fieldless GET is plain navigation; do not decorate it with an empty query.
            if !fields.is_empty() {
                url.query_pairs_mut().extend_pairs(fields);
            }
            request.url = url.to_string()
        } else {
            request.headers.insert(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            );
            request.body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields)
                .finish()
                .into_bytes();
        }
        self.request(request, transport, false)
    }
    pub fn storage_set(&mut self, key: &str, value: &str) -> Result<()> {
        let origin = self
            .resolve(self.url().ok_or_else(|| SimError::invalid("empty tab"))?)?
            .origin()
            .ascii_serialization();
        self.storage
            .entry(origin)
            .or_default()
            .insert(key.into(), value.into());
        Ok(())
    }
    pub fn storage_get(&self, key: &str) -> Result<Option<&str>> {
        let origin = self
            .resolve(self.url().ok_or_else(|| SimError::invalid("empty tab"))?)?
            .origin()
            .ascii_serialization();
        Ok(self
            .storage
            .get(&origin)
            .and_then(|s| s.get(key))
            .map(String::as_str))
    }
    /// Validate image and tab invariants before accepting an external checkpoint.
    pub fn validate_assets(&self) -> Result<()> {
        let mut bytes = 0usize;
        for asset in self.image_cache.values() {
            asset.validate()?;
            bytes = bytes.saturating_add(asset.rgba.len());
        }
        if bytes > MAX_CACHE_BYTES {
            return Err(SimError::invalid("image cache exceeds budget"));
        }
        for tab in &self.tabs {
            for entry in &tab.history {
                match &entry.content {
                    Content::Page(page) => page.validate()?,
                    Content::Web(web) => {
                        for asset in web.images().values() {
                            asset.validate()?;
                        }
                    }
                }
                for asset in entry.images.values() {
                    asset.validate()?;
                }
            }
        }
        Ok(())
    }
    fn load_images<F>(
        &mut self,
        page: &Page,
        base: &Url,
        transport: &mut F,
        refresh: bool,
    ) -> (BTreeMap<String, Arc<ImageAsset>>, BTreeMap<String, String>)
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let mut sources = Vec::new();
        walk(&page.elements, &mut |e| {
            if let PageElement::Image { id, source, .. } = e {
                sources.push((id.clone(), source.clone()));
            }
        });
        let mut images = BTreeMap::new();
        let mut errors = BTreeMap::new();
        for (id, source) in sources {
            match self.load_image(base, &source, transport, refresh) {
                Ok(image) => {
                    images.insert(id, image);
                }
                Err(error) => {
                    errors.insert(id, error.code);
                }
            }
        }
        (images, errors)
    }
    /// A native page's picture: same-origin only, the native RGBA format only.
    fn load_image<F>(
        &mut self,
        base: &Url,
        source: &str,
        transport: &mut F,
        refresh: bool,
    ) -> Result<Arc<ImageAsset>>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        let url = base
            .join(source)
            .map_err(|e| SimError::invalid(e.to_string()))?;
        self.load_image_from(url, transport, refresh, Some(base))
    }
    /// Fetches and decodes a picture, through the cache. With `same_origin_as`, the
    /// picture and every redirect must stay on that origin and be the native RGBA
    /// format (native page assets use a deliberately strict policy); without it, any
    /// origin the transport allows and any decodable format (RGBA, PNG, JPEG) will do.
    fn load_image_from<F>(
        &mut self,
        mut url: Url,
        transport: &mut F,
        refresh: bool,
        same_origin_as: Option<&Url>,
    ) -> Result<Arc<ImageAsset>>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        url.set_fragment(None);
        let original = url.to_string();
        let allowed = |url: &Url| {
            same_origin_as.is_none_or(|base| url.origin() == base.origin())
                && url.username().is_empty()
                && url.password().is_none()
        };
        if !allowed(&url) {
            return Err(SimError::denied("cross-origin native image"));
        }
        if !refresh {
            if let Some(asset) = self.image_cache.get(&original) {
                return Ok(asset.clone());
            }
        }
        for _ in 0..=8 {
            if !allowed(&url) {
                return Err(SimError::denied(
                    "cross-origin or credentialed image redirect",
                ));
            }
            let mut request = HttpRequest::get(url.as_str());
            let cookies = self.cookie_header(&url);
            if !cookies.is_empty() {
                request.headers.insert("cookie".into(), cookies);
            }
            self.pending = Some(request.clone());
            let response = transport(request);
            self.pending = None;
            let response = response?;
            if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                let location = response
                    .header("location")
                    .ok_or_else(|| SimError::invalid("image redirect lacks location"))?;
                url = url
                    .join(location)
                    .map_err(|e| SimError::invalid(e.to_string()))?;
                continue;
            }
            if response.status != 200 {
                return Err(SimError::new(
                    "image_http",
                    format!("image HTTP {}", response.status),
                ));
            }
            let kind = media_type(&response);
            if same_origin_as.is_some() && kind != RGBA_MEDIA_TYPE {
                return Err(SimError::new(
                    "image_format",
                    "unsupported native image media type",
                ));
            }
            if response.body.len() > MAX_IMAGE_BYTES * 4 + 1024 {
                return Err(SimError::invalid("image response exceeds budget"));
            }
            let asset = Arc::new(decode_image(&kind, &response.body)?);
            let existing = self.image_cache.get(&original).map_or(0, |a| a.rgba.len());
            let mut bytes: usize = self
                .image_cache
                .values()
                .map(|a| a.rgba.len())
                .sum::<usize>()
                - existing;
            self.image_cache.remove(&original);
            // Lexical eviction is deterministic; history entries retain their own
            // shared asset handles so cache eviction cannot change a past frame.
            while bytes + asset.rgba.len() > MAX_CACHE_BYTES {
                if let Some((_, old)) = self.image_cache.pop_first() {
                    bytes -= old.rgba.len();
                } else {
                    break;
                }
            }
            self.image_cache.insert(original, asset.clone());
            return Ok(asset);
        }
        Err(SimError::new("redirect_limit", "image redirect limit"))
    }
    pub fn scene(&self, width: u32, height: u32) -> Scene {
        let Some(entry) = self.entry() else {
            return Scene::new(width, height);
        };
        let page = match &entry.content {
            Content::Web(web) => return web.scene(self.inputs(width, height)),
            Content::Page(page) => page,
        };
        // Zoom works as a browser's does: the page is laid out for a viewport as many
        // CSS pixels wide as fit at that zoom, then drawn larger or smaller, so text
        // reflows instead of running off the side.
        let zoom = u32::from(self.zoom());
        let css = |v: u32| (v * 100 / zoom).max(1);
        let mut scene = page_scene::layout_scrolled(
            page,
            &self.tab().fields,
            &entry.images,
            css(width),
            css(height),
            self.tab().scroll_y,
            &self.tab().scroll_x,
        );
        if zoom != 100 {
            page_scene::scale(&mut scene, zoom, width, height);
        }
        scene
    }
    /// The accessibility view of the document on show at `width` x `height`: for a
    /// native page the scene's own; for an HTML document every element with a role
    /// and every text run, with bounds.
    pub fn semantics(&self, width: u32, height: u32) -> Vec<AxNode> {
        match self.entry().map(|e| &e.content) {
            Some(Content::Web(web)) => web.semantics(self.inputs(width, height)),
            _ => self.scene(width, height).accessibility(),
        }
    }
}
/// The HTML the engine renders for a response, when the response is one it shows:
/// HTML itself, plain text as a preformatted block, a picture on its own.
fn html_source(kind: &str, url: &Url, response: &HttpResponse) -> Option<String> {
    match kind {
        "text/html" | "application/xhtml+xml" => Some(cw_web::html::decode(&response.body)),
        "text/plain" => Some(web_document::text_document(
            &cw_web::html::decode(&response.body),
            url.as_str(),
        )),
        k if k.starts_with("image/") || k == RGBA_MEDIA_TYPE => {
            let size = decode_image(kind, &response.body).ok().map(|a| (a.width, a.height));
            Some(web_document::image_document(url.as_str(), size))
        }
        _ => None,
    }
}
fn cookie_path_matches(path: &str, prefix: &str) -> bool {
    path == prefix
        || (path.starts_with(prefix)
            && (prefix.ends_with('/') || path.as_bytes().get(prefix.len()) == Some(&b'/')))
}
fn parse_cookie(value: &str, request_path: &str) -> Option<Cookie> {
    let mut parts = value.split(';');
    let (name, value) = parts.next()?.trim().split_once('=')?;
    if name.is_empty() {
        return None;
    }
    let mut cookie = Cookie {
        name: name.into(),
        value: value.into(),
        path: request_path
            .rsplit_once('/')
            .map(|(p, _)| if p.is_empty() { "/" } else { p })
            .unwrap_or("/")
            .into(),
        secure: false,
        http_only: false,
    };
    for p in parts {
        let p = p.trim();
        if p.eq_ignore_ascii_case("secure") {
            cookie.secure = true
        } else if p.eq_ignore_ascii_case("httponly") {
            cookie.http_only = true
        } else if let Some((k, v)) = p.split_once('=') {
            if k.eq_ignore_ascii_case("path") && v.starts_with('/') {
                cookie.path = v.into()
            } else if k.eq_ignore_ascii_case("domain") {
                return None;
            }
        }
    }
    Some(cookie)
}
/// Every container the page model nests through; traversal must cover all of them
/// or ids inside rows, grids and cards would be unreachable.
fn children_of(e: &PageElement) -> Option<&[PageElement]> {
    match e {
        PageElement::Form { children, .. }
        | PageElement::Group { children, .. }
        | PageElement::Row { children, .. }
        | PageElement::Grid { children, .. }
        | PageElement::Card { children, .. } => Some(children),
        _ => None,
    }
}
fn walk(elements: &[PageElement], f: &mut impl FnMut(&PageElement)) {
    for e in elements {
        f(e);
        if let Some(children) = children_of(e) {
            walk(children, f)
        }
    }
}
fn find<'a>(elements: &'a [PageElement], id: &str) -> Option<&'a PageElement> {
    for e in elements {
        if e.id() == id {
            return Some(e);
        }
        if let Some(v) = children_of(e).and_then(|c| find(c, id)) {
            return Some(v);
        }
    }
    None
}
fn parent_form<'a>(elements: &'a [PageElement], id: &str) -> Option<&'a PageAction> {
    for e in elements {
        if let PageElement::Form {
            action, children, ..
        } = e
        {
            if find(children, id).is_some() {
                return Some(action);
            }
        }
        if let Some(a) = children_of(e).and_then(|c| parent_form(c, id)) {
            return Some(a);
        }
    }
    None
}
fn find_form_scope<'a>(elements: &'a [PageElement], id: &str) -> Option<&'a [PageElement]> {
    for e in elements {
        if let PageElement::Form {
            id: fid, children, ..
        } = e
        {
            if fid == id || find(children, id).is_some() {
                return Some(children);
            }
        }
        if let Some(a) = children_of(e).and_then(|c| find_form_scope(c, id)) {
            return Some(a);
        }
    }
    None
}

/// Integer layout shared by browsers and local applications; no rasterization occurs.
pub fn layout_page(
    page: &Page,
    fields: &BTreeMap<String, String>,
    width: u32,
    height: u32,
    scroll_y: i32,
) -> Scene {
    layout_page_with_images(page, fields, &BTreeMap::new(), width, height, scroll_y)
}
/// Project received/cached image assets without fetching or decoding while rendering.
pub fn layout_page_with_images(
    page: &Page,
    fields: &BTreeMap<String, String>,
    images: &BTreeMap<String, Arc<ImageAsset>>,
    width: u32,
    height: u32,
    scroll_y: i32,
) -> Scene {
    page_scene::layout(page, fields, images, width, height, scroll_y)
}


#[cfg(test)]
mod tests {
    use super::*;
    fn page() -> Page {
        let mut p = Page::new("Site");
        p.elements = vec![
            PageElement::Link {
                id: "next".into(),
                text: "next".into(),
                url: "/second".into(),
                style: None,
            },
            PageElement::Form {
                id: "form".into(),
                action: PageAction {
                    method: "POST".into(),
                    url: "/save".into(),
                    fields: BTreeMap::new(),
                },
                children: vec![
                    PageElement::Input {
                        id: "message".into(),
                        label: "Message".into(),
                        value: "before".into(),
                        placeholder: String::new(),
                    },
                    PageElement::Button {
                        id: "save".into(),
                        text: "Save".into(),
                        action: PageAction {
                            method: "POST".into(),
                            url: "/save".into(),
                            fields: BTreeMap::new(),
                        },
                        style: None,
                    },
                ],
            },
        ];
        p
    }
    #[test]
    fn an_unreachable_host_shows_an_error_page_and_the_action_still_fails() {
        let mut b = BrowserState::default();
        let mut http = |_: HttpRequest| HttpResponse::page(&page());
        b.navigate("http://internal.test", &mut http).unwrap();
        let mut dead = |r: HttpRequest| -> Result<HttpResponse> {
            Err(if r.url.contains("nowhere") {
                SimError::new("dns", "no such host")
            } else {
                SimError::new("connection_refused", "10.0.0.2:443")
            })
        };
        let error = b.navigate("https://nowhere.test/x", &mut dead).unwrap_err();
        assert_eq!(error.code, "dns");
        let shown = b.page().unwrap();
        assert_eq!(shown.title, "nowhere.test");
        let texts: Vec<&str> = shown
            .elements
            .iter()
            .filter_map(|e| match e {
                PageElement::Heading { text, .. }
                | PageElement::Text { text, .. }
                | PageElement::Styled { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"This site can't be reached"), "{texts:?}");
        assert!(texts.contains(&"ERR_NAME_NOT_RESOLVED"), "{texts:?}");
        assert!(
            texts.iter().any(|t| t.contains("nowhere.test")),
            "{texts:?}"
        );
        assert_eq!(b.url(), Some("https://nowhere.test/x"));
        assert_eq!(b.tab().history[b.tab().position].status, 0);
        // History still holds the working page behind it.
        b.back(&mut dead).unwrap();
        assert_eq!(b.page().unwrap().title, "Site");
        let error = b.navigate("https://internal.test/", &mut dead).unwrap_err();
        assert_eq!(error.code, "connection_refused");
        let shown = b.page().unwrap();
        assert!(shown.elements.iter().any(
            |e| matches!(e, PageElement::Styled { text, .. } if text == "ERR_CONNECTION_REFUSED")
        ));
        // A refusal that is not the network's leaves the tab alone.
        let mut denied = |_: HttpRequest| -> Result<HttpResponse> { Err(SimError::denied("no")) };
        assert!(b
            .navigate("http://internal.test/again", &mut denied)
            .is_err());
        assert_eq!(b.url(), Some("https://internal.test/"));
    }
    #[test]
    fn a_sites_404_is_a_page_but_api_paths_keep_their_json() {
        let mut b = BrowserState::default();
        let mut http = |r: HttpRequest| -> Result<HttpResponse> {
            if r.url.ends_with("/missing") || r.url.ends_with("/api/missing") {
                HttpResponse::json(404, &serde_json::json!({"error":"route not found"}))
            } else {
                HttpResponse::page(&page())
            }
        };
        b.navigate("http://github.test/", &mut http).unwrap();
        b.navigate("http://github.test/missing", &mut http).unwrap();
        let shown = b.page().unwrap();
        assert_eq!(shown.title, "github.test");
        assert!(shown
            .elements
            .iter()
            .any(|e| matches!(e, PageElement::Heading { text, .. } if text == "404 Not Found")));
        assert!(shown
            .elements
            .iter()
            .any(|e| matches!(e, PageElement::Link { url, .. } if url == "http://github.test/")));
        assert_eq!(b.tab().history[b.tab().position].status, 404);
        b.navigate("http://github.test/api/missing", &mut http)
            .unwrap();
        let shown = b.page().unwrap();
        assert!(shown.elements.iter().any(
            |e| matches!(e, PageElement::Text { text, .. } if text.contains("route not found"))
        ));
    }
    #[test]
    fn a_picture_with_an_action_is_a_control() {
        let mut b = BrowserState::default();
        let mut http = |r: HttpRequest| -> Result<HttpResponse> {
            if r.url.ends_with("/users/ada") {
                return HttpResponse::page(&Page::new("Ada"));
            }
            let mut p = Page::new("Profile");
            p.elements.push(PageElement::Image {
                id: "avatar".into(),
                source: "/avatar.rgba".into(),
                alt: "Ada".into(),
                width: 40,
                height: 40,
                style: Some(cw_protocol::Style::default().radius(20)),
                action: Some(PageAction {
                    method: "GET".into(),
                    url: "/users/ada".into(),
                    fields: BTreeMap::new(),
                }),
            });
            HttpResponse::page(&p)
        };
        b.navigate("http://site.test/", &mut http).unwrap();
        b.click("avatar", &mut http).unwrap();
        assert_eq!(b.url(), Some("http://site.test/users/ada"));
    }
    #[test]
    fn links_forms_and_history_use_transport() {
        let mut b = BrowserState::default();
        let mut requests = vec![];
        {
            let mut http = |r: HttpRequest| {
                requests.push(r);
                HttpResponse::page(&page())
            };
            b.navigate("http://internal.test", &mut http).unwrap();
            b.fill("message", "hello & world").unwrap();
            b.click("save", &mut http).unwrap();
            b.click("next", &mut http).unwrap();
            b.back(&mut http).unwrap();
            assert_eq!(b.url(), Some("http://internal.test/save"));
            b.forward(&mut http).unwrap();
        }
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1].method, "POST");
        assert_eq!(
            String::from_utf8_lossy(&requests[1].body),
            "message=hello+%26+world"
        );
        assert_eq!(requests[2].url, "http://internal.test/second");
    }
    #[test]
    fn card_and_thumbnail_actions_navigate_but_inert_ones_do_not() {
        let mut p = Page::new("Results");
        p.elements = vec![PageElement::Card {
            id: "hit".into(),
            children: vec![
                PageElement::Thumbnail {
                    id: "still".into(),
                    label: "Still".into(),
                    style: cw_protocol::Style::default(),
                    action: None,
                },
                PageElement::Thumbnail {
                    id: "play".into(),
                    label: "Play".into(),
                    style: cw_protocol::Style::default(),
                    action: Some(PageAction {
                        method: "GET".into(),
                        url: "/watch".into(),
                        fields: BTreeMap::new(),
                    }),
                },
            ],
            style: cw_protocol::Style::default(),
            action: Some(PageAction {
                method: "GET".into(),
                url: "/result".into(),
                fields: BTreeMap::new(),
            }),
        }];
        let mut b = BrowserState::default();
        let mut seen = vec![];
        let mut http = |r: HttpRequest| {
            seen.push(r.url.clone());
            HttpResponse::page(&p)
        };
        b.navigate("http://internal.test", &mut http).unwrap();
        b.click("hit", &mut http).unwrap();
        // Ids nested inside a card stay reachable, and inert artwork stays inert.
        b.click("play", &mut http).unwrap();
        assert!(b.click("still", &mut http).is_err());
        assert_eq!(
            seen,
            [
                "http://internal.test/",
                "http://internal.test/result",
                "http://internal.test/watch"
            ]
        );
    }
    #[test]
    fn redirects_cookie_scope_storage_and_snapshot() {
        let mut b = BrowserState::default();
        let mut seen = vec![];
        let mut http = |r: HttpRequest| {
            seen.push(r.clone());
            let mut response = HttpResponse::page(&page())?;
            if r.url == "https://a.test/start" {
                response.status = 302;
                response.headers.insert("location".into(), "/next".into());
                response
                    .headers
                    .insert("set-cookie".into(), "session=secret; Path=/; Secure".into());
            }
            Ok(response)
        };
        b.navigate("https://a.test/start", &mut http).unwrap();
        b.storage_set("key", "a").unwrap();
        b.navigate("https://b.test", &mut http).unwrap();
        assert_eq!(b.storage_get("key").unwrap(), None);
        assert_eq!(seen[1].header("cookie"), Some("session=secret"));
        assert_eq!(seen[2].header("cookie"), None);
        let restored: BrowserState =
            serde_json::from_slice(&serde_json::to_vec(&b).unwrap()).unwrap();
        assert_eq!(restored, b);
    }
    #[test]
    fn structured_layout_hit_test_and_keyboard() {
        let mut b = BrowserState::default();
        let mut http = |_: HttpRequest| HttpResponse::page(&page());
        b.navigate("http://a.test", &mut http).unwrap();
        let scene = b.scene(640, 480);
        assert_eq!(scene, b.scene(640, 480));
        let node = scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some("message"))
            .unwrap();
        assert_eq!(
            scene
                .hit_test(node.bounds.x + 1, node.bounds.y + 1)
                .unwrap()
                .interaction
                .as_deref(),
            Some("message")
        );
        b.click("message", &mut http).unwrap();
        b.key("Backspace", &mut http).unwrap();
        b.text("!").unwrap();
        assert_eq!(b.tab().fields["message"], "befor!");
    }
    #[test]
    fn rejected_navigation_does_not_destroy_received_page() {
        let mut b = BrowserState::default();
        b.navigate("http://a.test", &mut |_| HttpResponse::page(&page()))
            .unwrap();
        let before = b.page().cloned();
        assert!(b
            .navigate("file:///etc/passwd", &mut |_| panic!("must not dispatch"))
            .is_err());
        assert!(b
            .navigate("http://b.test", &mut |_| Err(SimError::denied("blocked")))
            .is_err());
        assert_eq!(b.page().cloned(), before);
        assert!(b.pending.is_none());
    }
    #[test]
    fn duplicate_ids_and_redirect_loops_rejected() {
        let mut b = BrowserState::default();
        let mut p = page();
        p.elements.push(p.elements[0].clone());
        assert!(b
            .navigate("http://a.test", &mut |_| HttpResponse::page(&p))
            .is_err());
        let mut calls = 0;
        assert!(b
            .navigate("http://a.test", &mut |_| {
                calls += 1;
                Ok(HttpResponse {
                    status: 302,
                    headers: BTreeMap::from([("location".into(), "/loop".into())]),
                    body: vec![],
                })
            })
            .is_err());
        assert_eq!(calls, 17);
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    fn image_page(source: &str) -> Page {
        let mut page = Page::new("Images");
        page.elements.push(PageElement::Image {
            id: "logo".into(),
            source: source.into(),
            alt: "Company logo".into(),
            width: 20,
            height: 20,
            style: None,
            action: None,
        });
        page
    }
    fn image_response(red: u8) -> HttpResponse {
        let asset = ImageAsset {
            width: 1,
            height: 1,
            rgba: vec![red, 20, 30, 255],
        };
        HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), RGBA_MEDIA_TYPE.into())]),
            body: serde_json::to_vec(&asset).unwrap(),
        }
    }
    #[test]
    fn assets_use_transport_cache_and_checkpointed_history() {
        let mut browser = BrowserState::default();
        let mut paths = vec![];
        let mut red = 1;
        {
            let mut http = |r: HttpRequest| {
                paths.push(r.url.clone());
                if r.url.ends_with("/logo.rgba") {
                    Ok(image_response(red))
                } else {
                    HttpResponse::page(&image_page("/logo.rgba"))
                }
            };
            browser
                .navigate("https://site.test/home", &mut http)
                .unwrap();
            browser
                .navigate("https://site.test/next", &mut http)
                .unwrap();
        }
        assert_eq!(
            paths,
            vec![
                "https://site.test/home",
                "https://site.test/logo.rgba",
                "https://site.test/next"
            ]
        );
        let old_frame = browser.scene(200, 200);
        assert!(old_frame
            .nodes
            .iter()
            .any(|n| matches!(&n.primitive,Primitive::Image{rgba,..}if rgba==&[1,20,30,255])));
        let checkpoint = serde_json::to_vec(&browser).unwrap();
        let restored: BrowserState = serde_json::from_slice(&checkpoint).unwrap();
        restored.validate_assets().unwrap();
        assert_eq!(restored.scene(200, 200), old_frame);
        red = 240;
        browser
            .reload(&mut |r: HttpRequest| {
                if r.url.ends_with("/logo.rgba") {
                    Ok(image_response(red))
                } else {
                    HttpResponse::page(&image_page("/logo.rgba"))
                }
            })
            .unwrap();
        assert_ne!(browser.scene(200, 200), old_frame);
        browser
            .back(&mut |_| panic!("history must not refetch"))
            .unwrap();
        assert_eq!(browser.scene(200, 200), old_frame);
    }
    #[test]
    fn a_page_that_asks_to_be_refreshed_is_fetched_again_on_the_world_clock() {
        let mut browser = BrowserState::default();
        let mut seen: Vec<(String, bool)> = vec![];
        let mut version = 0;
        let serve = |r: HttpRequest, version: u32| {
            if r.url.ends_with("/logo.rgba") {
                return Ok(image_response(9));
            }
            let mut page = image_page("/logo.rgba");
            page.elements.push(PageElement::Input {
                id: "q".into(),
                label: "Search".into(),
                value: String::new(),
                placeholder: String::new(),
            });
            page.elements.push(PageElement::Text {
                id: "position".into(),
                text: format!("version {version}"),
            });
            let mut response = HttpResponse::page(&page)?;
            response
                .headers
                .insert("refresh".into(), "1; url=/player?live=1".into());
            Ok(response)
        };
        browser
            .navigate("https://site.test/player", &mut |r: HttpRequest| {
                seen.push((r.url.clone(), r.header(REFRESH_HEADER).is_some()));
                serve(r, 0)
            })
            .unwrap();
        let refresh = browser.tab().history[0].refresh.clone().unwrap();
        assert_eq!(
            (refresh.after_ms, refresh.url.as_str()),
            (1_000, "https://site.test/player?live=1")
        );
        // The first look stamps it; a second before its interval is up does nothing.
        assert!(browser.refresh_pending(5_000_000));
        assert!(!browser.refresh_due(5_000_000));
        assert!(!browser.refresh_pending(5_500_000));
        assert!(browser.refresh_pending(6_000_000));
        browser.fill("q", "half typed").unwrap();
        browser.tab_mut().scroll_y = 40;
        assert!(browser.refresh_due(6_000_000));
        version += 1;
        browser
            .refresh(6_000_000, &mut |r: HttpRequest| {
                seen.push((r.url.clone(), r.header(REFRESH_HEADER).is_some()));
                serve(r, version)
            })
            .unwrap();
        // It fetched the page again, marked as a refresh, and not its (cached) picture.
        assert_eq!(
            seen,
            [
                ("https://site.test/player".to_owned(), false),
                ("https://site.test/logo.rgba".to_owned(), false),
                ("https://site.test/player?live=1".to_owned(), true),
            ]
        );
        assert!(matches!(
            browser.page().unwrap().elements.last(),
            Some(PageElement::Text { text, .. }) if text == "version 1"
        ));
        // What was typed and where the page was scrolled survive; the history did not grow.
        assert_eq!(browser.tab().fields["q"], "half typed");
        assert_eq!(browser.tab().scroll_y, 40);
        assert_eq!(browser.tab().history.len(), 1);
        assert!(!browser.refresh_pending(6_500_000));
        // A refresh aimed at another site is ignored.
        assert!(parse_refresh(
            "1; url=https://evil.test/",
            &Url::parse("https://site.test/").unwrap()
        )
        .is_none());
    }
    #[test]
    fn asset_origin_redirect_and_decode_errors_are_explicit() {
        let mut browser = BrowserState::default();
        let mut paths = vec![];
        browser
            .navigate("https://site.test/", &mut |r: HttpRequest| {
                paths.push(r.url);
                HttpResponse::page(&image_page("https://other.test/logo"))
            })
            .unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(browser.tab().history[0].image_errors["logo"], "denied");
        browser
            .navigate("https://site.test/redirect", &mut |r: HttpRequest| {
                if r.url.ends_with("/asset") {
                    Ok(HttpResponse {
                        status: 302,
                        headers: BTreeMap::from([(
                            "location".into(),
                            "https://other.test/image".into(),
                        )]),
                        body: vec![],
                    })
                } else {
                    HttpResponse::page(&image_page("/asset"))
                }
            })
            .unwrap();
        assert_eq!(browser.tab().history[1].image_errors["logo"], "denied");
        browser
            .navigate("https://site.test/bad", &mut |r: HttpRequest| {
                if r.url.ends_with("/bad-image") {
                    let mut r = image_response(1);
                    r.body = br#"{"width":10,"height":10,"rgba":[0]}"#.to_vec();
                    Ok(r)
                } else {
                    HttpResponse::page(&image_page("/bad-image"))
                }
            })
            .unwrap();
        assert_eq!(browser.tab().history[2].image_errors["logo"], "invalid");
        assert!(browser.pending.is_none());
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn semantic_node_identity_survives_preceding_insertions() {
        let mut page = Page::new("App");
        page.elements.push(PageElement::Input {
            id: "field".into(),
            label: "Field".into(),
            value: "one".into(),
            placeholder: String::new(),
        });
        let old = layout_page(&page, &BTreeMap::new(), 400, 300, 0);
        page.elements.insert(
            0,
            PageElement::Text {
                id: "notice".into(),
                text: "Added notice".into(),
            },
        );
        let next = layout_page(&page, &BTreeMap::new(), 400, 300, 0);
        let ids = |s: &Scene| {
            s.nodes
                .iter()
                .filter(|n| n.interaction.as_deref() == Some("field"))
                .map(|n| n.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&old), ids(&next));
        next.validate().unwrap();
    }
}

#[cfg(test)]
mod web_tests {
    use super::*;
    use cw_scene::Color;

    fn html(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: BTreeMap::from([("content-type".into(), "text/html; charset=utf-8".into())]),
            body: body.as_bytes().to_vec(),
        }
    }
    fn css(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), "text/css".into())]),
            body: body.as_bytes().to_vec(),
        }
    }
    fn rgba(red: u8) -> HttpResponse {
        let asset = ImageAsset { width: 2, height: 2, rgba: [red, 20, 30, 255].repeat(4) };
        HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), RGBA_MEDIA_TYPE.into())]),
            body: serde_json::to_vec(&asset).unwrap(),
        }
    }
    const HOME: &str = r##"<!DOCTYPE html><html><head><title>Home</title>
<link rel="stylesheet" href="/style.css">
<style>@import url("/imported.css"); p { margin: 0 }</style>
</head><body>
<h1 id="title">Welcome aboard</h1>
<p>The quick brown fox.</p>
<a id="next" href="/second">Second page</a>
<a id="blank" href="/second" target="_blank">New tab</a>
<form id="form" action="/search">
  <label for="q">Query</label> <input id="q" name="q" value="">
  <input type="checkbox" id="agree" name="agree">
  <select id="pick" name="pick"><option value="a">Alpha</option><option value="b">Beta</option></select>
  <button id="go" type="submit">Go</button>
</form>
<form id="pf" method="post" action="/post" enctype="multipart/form-data">
  <input id="m" name="m"><textarea id="notes" name="notes">a
b</textarea><input type="hidden" name="h" value="1"><button id="send" name="send" value="yes">Send</button>
</form>
<form id="strict" action="/strict"><input id="must" name="must" required><button id="try">Try</button></form>
<img id="logo" src="/logo.rgba" alt="Logo">
<div style="height: 3000px"></div>
<a id="down" href="#bottom">Down</a>
<p id="bottom">The end.</p>
</body></html>"##;

    fn serve(requests: &mut Vec<HttpRequest>, r: HttpRequest) -> Result<HttpResponse> {
        requests.push(r.clone());
        let path = Url::parse(&r.url).unwrap().path().to_owned();
        Ok(match path.as_str() {
            "/" => html(200, HOME),
            "/style.css" => css("h1 { color: #ff0000 } .imported { color: #0000ff }"),
            "/imported.css" => css("h1 { font-size: 40px }"),
            "/logo.rgba" => rgba(200),
            "/second" => html(200, "<title>Second</title><h1>Second page</h1><p class=imported>Blue text</p>"),
            "/search" => html(200, "<title>Results</title><p>Results</p>"),
            "/post" => html(200, "<title>Posted</title><p>Posted</p>"),
            "/meta" => html(200, r#"<meta http-equiv="refresh" content="2; url=/second"><p>Soon</p>"#),
            "/plain" => HttpResponse::text(200, "just <text>"),
            "/tabs" => html(200, r#"<a id="l1" href="/">One</a><input id="i1"><button id="b1">B</button><input id="i2" tabindex="1">"#),
            "/ids" => html(200, r#"<div><span id="doc-title">Documents</span></div><table><tr><td id="sheet-A1">Latency p95</td><td id="sheet-A2">412 ms</td></tr></table><p>plain <span id="inline-id">marked</span> prose</p>"#),
            _ => html(404, "<title>Lost</title><h1>Lost page</h1><p>No such page here.</p>"),
        })
    }
    fn texts(scene: &Scene) -> Vec<(String, Color, u16)> {
        scene
            .nodes
            .iter()
            .filter_map(|n| match &n.primitive {
                Primitive::UiText { text, color, size, .. } | Primitive::UiTextBold { text, color, size, .. } | Primitive::Text { text, color, size } => {
                    Some((text.clone(), *color, *size))
                }
                _ => None,
            })
            .collect()
    }
    fn browser() -> (BrowserState, Vec<HttpRequest>) {
        let mut b = BrowserState::default();
        let mut requests = vec![];
        b.navigate("http://site.test/", &mut |r| serve(&mut requests, r)).unwrap();
        (b, requests)
    }

    #[test]
    fn an_html_response_renders_through_the_engine_with_its_sheets_and_pictures() {
        let (b, requests) = browser();
        let seen: Vec<&str> = requests.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(seen, ["http://site.test/", "http://site.test/style.css", "http://site.test/imported.css", "http://site.test/logo.rgba"]);
        let doc = b.document().expect("html document");
        assert_eq!(doc.title, "Home");
        assert_eq!(b.title().as_deref(), Some("Home"));
        assert_eq!(doc.sheets().len(), 3, "linked, imported, inline in cascade order");
        assert_eq!(doc.sheets()[1].url, "http://site.test/imported.css");
        let scene = b.scene(800, 600);
        let texts = texts(&scene);
        let words: Vec<&str> = texts.iter().map(|t| t.0.as_str()).collect();
        assert!(words.iter().any(|w| w.contains("Welcome")), "{words:?}");
        assert!(words.iter().any(|w| w.contains("quick brown fox")), "{words:?}");
        // The linked sheet coloured the heading red and the imported one sized it.
        let heading = texts.iter().find(|t| t.0.contains("Welcome")).unwrap();
        assert_eq!(heading.1, Color::rgb(255, 0, 0));
        assert_eq!(heading.2, 40);
        let pictures: Vec<_> = scene.nodes.iter().filter_map(|n| match &n.primitive { Primitive::Image { width, height, rgba } => Some((*width, *height, rgba[0], n.bounds)), _ => None }).collect();
        assert!(pictures.iter().any(|p| p.2 == 200), "{pictures:?} {:?} {:?}", doc.images().keys().collect::<Vec<_>>(), doc.image_errors);
        assert_eq!(scene.scrolls[0].target, "pane:page");
        assert!(scene.scrolls[0].extent > 3000);
        assert_eq!(b.tab().fields.get("q").map(String::as_str), Some(""));
        // The accessibility view lists the page's controls and text.
        let ax = b.semantics(800, 600);
        assert!(ax.iter().any(|a| a.role == "link" && a.id == "next" && a.name == "Second page"));
        assert!(ax.iter().any(|a| a.role == "heading" && a.name == "h1: Welcome aboard"));
        assert!(ax.iter().any(|a| a.role == "textbox" && a.id == "q" && a.name == "Query"));
        assert!(ax.iter().any(|a| a.role == "text" && a.name.contains("quick")));
        let page = b.current_page().unwrap();
        assert!(page.elements.iter().any(|e| matches!(e, PageElement::Link { id, url, .. } if id == "next" && url == "http://site.test/second")));
        assert!(page.elements.iter().any(|e| matches!(e, PageElement::Form { children, .. } if children.iter().any(|c| matches!(c, PageElement::Input { id, label, .. } if id == "q" && label == "Query")))));
        assert!(b.has_input("q") && !b.has_input("agree"));
    }

    #[test]
    fn ids_on_cells_and_inline_chrome_reach_the_projection() {
        // The agent is told to read `#sheet-A2`; the projection used to hand it
        // `text:4`, so the id it was given addressed nothing it could see.
        let (mut b, mut requests) = browser();
        b.navigate("http://site.test/ids", &mut |r| serve(&mut requests, r)).unwrap();
        let page = b.current_page().unwrap();
        let ids: Vec<(&str, &str)> = page.elements.iter().filter_map(|e| match e { PageElement::Text { id, text } => Some((id.as_str(), text.as_str())), _ => None }).collect();
        assert!(ids.contains(&("sheet-A1", "Latency p95")), "{ids:?}");
        assert!(ids.contains(&("sheet-A2", "412 ms")), "{ids:?}");
        assert!(ids.contains(&("doc-title", "Documents")), "{ids:?}");
        assert!(ids.contains(&("inline-id", "marked")), "{ids:?}");
        // And the scene's semantic tree carries them too.
        let ax = b.semantics(800, 600);
        assert!(ax.iter().any(|a| a.role == "cell" && a.id == "sheet-A2"), "{:?}", ax.iter().map(|a| (&a.role, &a.id)).collect::<Vec<_>>());
        assert!(ax.iter().any(|a| a.id == "doc-title"));
    }

    #[test]
    fn links_navigate_and_fragments_scroll() {
        let (mut b, mut requests) = browser();
        b.click("down", &mut |r| serve(&mut requests, r)).unwrap();
        assert!(b.tab().scroll_y > 2000, "{}", b.tab().scroll_y);
        assert_eq!(b.url(), Some("http://site.test/#bottom"));
        assert_eq!(requests.len(), 4, "a fragment link makes no request");
        b.click("next", &mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!(b.url(), Some("http://site.test/second"));
        assert_eq!(b.title().as_deref(), Some("Second"));
        // The imported rule is gone with the old document; the second page has no sheets.
        let blue = texts(&b.scene(800, 600)).into_iter().find(|t| t.0.contains("Blue")).unwrap();
        assert_ne!(blue.1, Color::rgb(0, 0, 255));
        b.back(&mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!(b.title().as_deref(), Some("Home"));
        b.click("blank", &mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!((b.tabs.len(), b.active), (2, 1));
        assert_eq!(b.url(), Some("http://site.test/second"));
    }

    #[test]
    fn get_forms_carry_their_data_set_in_the_query() {
        let (mut b, mut requests) = browser();
        b.fill("q", "rust lang").unwrap();
        b.click("agree", &mut |r| serve(&mut requests, r)).unwrap();
        assert!(b.document().unwrap().is_checked(b.document().unwrap().node_for("agree").unwrap()));
        assert!(b.semantics(800, 600).iter().any(|a| a.id == "agree" && a.checked == Some(true)));
        b.fill("pick", "Beta").unwrap();
        b.click("go", &mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!(requests.last().unwrap().url, "http://site.test/search?q=rust+lang&agree=on&pick=b");
        assert_eq!(b.title().as_deref(), Some("Results"));
        b.back(&mut |r| serve(&mut requests, r)).unwrap();
        b.fill("q", "again").unwrap();
        b.key("Enter", &mut |r| serve(&mut requests, r)).unwrap();
        assert!(requests.last().unwrap().url.starts_with("http://site.test/search?q=again"), "{}", requests.last().unwrap().url);
        b.back(&mut |r| serve(&mut requests, r)).unwrap();
        b.fill("q", "third").unwrap();
        b.submit("form", &mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!(requests.last().unwrap().url, "http://site.test/search?q=third&agree=on&pick=b");
        // A required field left empty blocks the submission with a message.
        b.back(&mut |r| serve(&mut requests, r)).unwrap();
        let n = requests.len();
        let err = b.click("try", &mut |r| serve(&mut requests, r)).unwrap_err();
        assert!(err.message.contains("fill out"), "{}", err.message);
        assert_eq!(requests.len(), n);
        assert!(b.scene(800, 600).nodes.iter().any(|n| n.semantic.as_ref().is_some_and(|s| s.role == "status")));
    }

    #[test]
    fn post_forms_send_a_body_and_typing_edits_the_focused_control() {
        let (mut b, mut requests) = browser();
        b.click("m", &mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!(b.tab().focused.as_deref(), Some("m"));
        b.text("hello").unwrap();
        b.key("Backspace", &mut |r| serve(&mut requests, r)).unwrap();
        b.key("Home", &mut |r| serve(&mut requests, r)).unwrap();
        b.text("say ").unwrap();
        assert_eq!(b.tab().fields["m"], "say hell");
        b.click("send", &mut |r| serve(&mut requests, r)).unwrap();
        let post = requests.last().unwrap();
        assert_eq!((post.method.as_str(), post.url.as_str()), ("POST", "http://site.test/post"));
        let body = String::from_utf8_lossy(&post.body);
        let boundary = post.header("content-type").unwrap().split("boundary=").nth(1).unwrap().to_owned();
        assert!(body.starts_with(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"m\"\r\n\r\nsay hell\r\n")), "{body}");
        assert!(body.contains("name=\"notes\"\r\n\r\na\r\nb\r\n"), "{body}");
        assert!(body.contains("name=\"h\"\r\n\r\n1\r\n"), "{body}");
        assert!(body.contains("name=\"send\"\r\n\r\nyes\r\n"), "{body}");
        assert!(body.ends_with(&format!("--{boundary}--\r\n")));
        assert_eq!(b.title().as_deref(), Some("Posted"));
    }

    #[test]
    fn tab_walks_the_focus_order_and_space_activates() {
        let mut requests = vec![];
        let mut b = BrowserState::default();
        b.navigate("http://site.test/tabs", &mut |r| serve(&mut requests, r)).unwrap();
        let mut order = vec![];
        for _ in 0..5 {
            b.key("Tab", &mut |r| serve(&mut requests, r)).unwrap();
            order.push(b.tab().focused.clone().unwrap());
        }
        assert_eq!(order, ["i2", "l1", "i1", "b1", "i2"]);
        b.key("Shift+Tab", &mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!(b.tab().focused.as_deref(), Some("b1"));
        b.key("Escape", &mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!(b.tab().focused, None);
        let (mut b, mut requests) = browser();
        b.click("agree", &mut |r| serve(&mut requests, r)).unwrap();
        b.key(" ", &mut |r| serve(&mut requests, r)).unwrap();
        let doc = b.document().unwrap();
        assert!(!doc.is_checked(doc.node_for("agree").unwrap()));
        b.click("next", &mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!(b.title().as_deref(), Some("Second"));
    }

    #[test]
    fn error_bodies_plain_text_and_meta_refresh() {
        let mut requests = vec![];
        let mut b = BrowserState::default();
        b.navigate("http://site.test/nowhere", &mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!(b.tab().history[0].status, 404);
        assert_eq!(b.title().as_deref(), Some("Lost"));
        assert!(b.document().unwrap().text().contains("Lost page"));
        b.navigate("http://site.test/plain", &mut |r| serve(&mut requests, r)).unwrap();
        let words: Vec<String> = texts(&b.scene(400, 300)).into_iter().map(|t| t.0).collect();
        assert!(words.iter().any(|w| w.contains("just <text>")), "{words:?}");
        b.navigate("http://site.test/meta", &mut |r| serve(&mut requests, r)).unwrap();
        let refresh = b.tab().history[b.tab().position].refresh.clone().unwrap();
        assert_eq!((refresh.after_ms, refresh.url.as_str()), (2_000, "http://site.test/second"));
        assert!(b.refresh_pending(0));
    }

    #[test]
    fn zoom_reflows_hover_names_the_cursor_and_snapshots_round_trip() {
        let (mut b, mut requests) = browser();
        let before = texts(&b.scene(600, 400));
        b.step_zoom("in").unwrap();
        b.step_zoom("in").unwrap();
        let after = texts(&b.scene(600, 400));
        let size = |t: &[(String, Color, u16)]| t.iter().find(|x| x.0.contains("Welcome")).unwrap().2;
        assert!(size(&after) > size(&before), "{} vs {}", size(&after), size(&before));
        assert_eq!(b.document().unwrap().last_viewport(), (600, 400, 125));
        b.step_zoom("reset").unwrap();
        let scene = b.scene(600, 400);
        let link = scene.nodes.iter().find(|n| n.interaction.as_deref() == Some("next")).unwrap().bounds;
        let (lx, ly) = (link.x + 2, link.y + link.height as i32 / 2);
        assert_eq!(b.hover_at(lx, ly, 600, 400), Some("pointer"));
        assert_eq!(b.document().unwrap().hovered(), b.document().unwrap().node_for("next"));
        let input = scene.nodes.iter().find(|n| n.interaction.as_deref() == Some("q")).unwrap().bounds;
        assert_eq!(b.cursor_at(input.x + 3, input.y + input.height as i32 / 2, 600, 400), Some("text"));
        assert_eq!(b.cursor_at(599, 399, 600, 400), Some("default"));
        b.click_at(lx, ly, 600, 400, &mut |r| serve(&mut requests, r)).unwrap();
        assert_eq!(b.title().as_deref(), Some("Second"));
        b.back(&mut |r| serve(&mut requests, r)).unwrap();
        b.fill("q", "kept").unwrap();
        b.click("agree", &mut |r| serve(&mut requests, r)).unwrap();
        let frame = b.scene(600, 400);
        let restored: BrowserState = serde_json::from_slice(&serde_json::to_vec(&b).unwrap()).unwrap();
        assert_eq!(restored, b);
        restored.validate_assets().unwrap();
        assert_eq!(restored.scene(600, 400), frame);
        assert!(b.scroll_pane("page", 500, false));
        assert_eq!(b.scene(600, 400).scrolls[0].offset, 500);
    }
}
