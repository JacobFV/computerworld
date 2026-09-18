//! Versioned, serializable contracts. This crate performs no host I/O.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
pub const SCHEMA_VERSION: u32 = 1;
pub const PAGE_MEDIA_TYPE: &str = "application/vnd.computerworld.page+json";
/// A page image: JSON `{width, height, rgba}`, straight-alpha RGBA8, row-major.
pub const RGBA_MEDIA_TYPE: &str = "application/vnd.computerworld.rgba+json";
/// Set on a request a browser makes because the page on show asked to be refreshed
/// (a `refresh: <seconds>; url=<path>` response header), so a site can tell a page
/// keeping itself current from a person visiting it.
pub const REFRESH_HEADER: &str = "x-computerworld-refresh";
pub type Result<T> = std::result::Result<T, SimError>;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct SimError {
    pub code: String,
    pub message: String,
}
impl SimError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid", message)
    }
    pub fn denied(message: impl Into<String>) -> Self {
        Self::new("denied", message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }
}
impl From<serde_json::Error> for SimError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("serialization", e.to_string())
    }
}
fn yes() -> bool {
    true
}
fn port() -> u16 {
    80
}
fn version() -> u32 {
    SCHEMA_VERSION
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldDefinition {
    #[serde(default = "version")]
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub profiles: Vec<OsProfile>,
    #[serde(default)]
    pub computers: Vec<ComputerDefinition>,
    #[serde(default)]
    pub network: NetworkDefinition,
    #[serde(default)]
    pub services: Vec<ServiceDefinition>,
    #[serde(default)]
    pub metadata: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsProfile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub home: String,
    #[serde(default = "yes")]
    pub case_sensitive: bool,
    #[serde(default)]
    pub shell: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerDefinition {
    pub id: String,
    pub profile: String,
    pub address: String,
    pub user: String,
    #[serde(default)]
    pub node: String,
    #[serde(default)]
    pub initial_files: BTreeMap<String, String>,
    /// Files whose bytes are not text (a workbook, a database), base64-encoded, under
    /// the same paths `initial_files` takes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub initial_binary_files: BTreeMap<String, String>,
    #[serde(default)]
    pub installed_apps: Vec<String>,
    #[serde(default)]
    pub packages: Vec<String>,
}
impl ComputerDefinition {
    /// The seeded files that are not text, decoded.
    pub fn binary_files(&self) -> Result<Vec<(&str, Vec<u8>)>> {
        self.initial_binary_files
            .iter()
            .map(|(path, text)| {
                decode_base64(text)
                    .map(|bytes| (path.as_str(), bytes))
                    .map_err(|e| SimError::invalid(format!("{path}: {e}")))
            })
            .collect()
    }
    pub fn node_id(&self) -> &str {
        if self.node.is_empty() {
            &self.id
        } else {
            &self.node
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceDefinition {
    pub id: String,
    pub kind: String,
    pub node: String,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default = "port")]
    pub port: u16,
    #[serde(default)]
    pub initial_state: Value,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkDefinition {
    #[serde(default)]
    pub implicit_lan: bool,
    #[serde(default)]
    pub nodes: Vec<NetworkNode>,
    #[serde(default)]
    pub links: Vec<NetworkLink>,
    #[serde(default)]
    pub dns: Vec<DnsRecord>,
    #[serde(default)]
    pub routes: Vec<Route>,
    #[serde(default)]
    pub gateway: GatewayPolicy,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkNode {
    pub id: String,
    pub address: String,
    #[serde(default)]
    pub zone: NetworkZone,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkZone {
    #[default]
    Local,
    Internet,
    Host,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkLink {
    pub from: String,
    pub to: String,
    #[serde(default = "yes")]
    pub bidirectional: bool,
    #[serde(default)]
    pub latency_us: u64,
    #[serde(default)]
    pub loss_per_million: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsRecord {
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub ttl_us: u64,
    #[serde(default)]
    pub resolver: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub via: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayPolicy {
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub schemes: Vec<String>,
    #[serde(default)]
    pub allowed_cidrs: Vec<String>,
    #[serde(default)]
    pub denied_cidrs: Vec<String>,
    #[serde(default = "default_max_response_bytes")]
    pub max_response_bytes: usize,
    #[serde(default = "default_timeout_us")]
    pub timeout_us: u64,
    #[serde(default = "yes")]
    pub allow_local: bool,
    #[serde(default = "yes")]
    pub allow_internet: bool,
    #[serde(default)]
    pub allow_host: bool,
    #[serde(default)]
    pub host_allowlist: Vec<String>,
    #[serde(default)]
    pub denied_pairs: Vec<(String, String)>,
}
fn default_max_response_bytes() -> usize {
    1_048_576
}
fn default_timeout_us() -> u64 {
    30_000_000
}
impl Default for GatewayPolicy {
    fn default() -> Self {
        Self {
            sources: vec![],
            ports: vec![],
            schemes: vec![],
            allowed_cidrs: vec![],
            denied_cidrs: vec![],
            max_response_bytes: default_max_response_bytes(),
            timeout_us: default_timeout_us(),
            allow_local: true,
            allow_internet: true,
            allow_host: false,
            host_allowlist: vec![],
            denied_pairs: vec![],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Vec<u8>,
}
impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET".into(),
            url: url.into(),
            headers: BTreeMap::new(),
            body: vec![],
        }
    }
    pub fn json(
        method: impl Into<String>,
        url: impl Into<String>,
        value: &impl Serialize,
    ) -> Result<Self> {
        Ok(Self {
            method: method.into(),
            url: url.into(),
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body: serde_json::to_vec(value)?,
        })
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Vec<u8>,
}
impl HttpResponse {
    pub fn text(status: u16, text: impl Into<String>) -> Self {
        Self {
            status,
            headers: BTreeMap::from([("content-type".into(), "text/plain; charset=utf-8".into())]),
            body: text.into().into_bytes(),
        }
    }
    pub fn json(status: u16, value: &impl Serialize) -> Result<Self> {
        Ok(Self {
            status,
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body: serde_json::to_vec(value)?,
        })
    }
    pub fn page(page: &Page) -> Result<Self> {
        Ok(Self {
            status: 200,
            headers: BTreeMap::from([("content-type".into(), PAGE_MEDIA_TYPE.into())]),
            body: serde_json::to_vec(page)?,
        })
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    #[serde(default = "version")]
    pub version: u32,
    pub title: String,
    #[serde(default)]
    pub elements: Vec<PageElement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<PageTheme>,
}
/// Radius and padding budget: pages describe documents, not arbitrary geometry.
pub const MAX_STYLE_SPAN: u32 = 64;
/// A fully round element needs a radius of half its own size, and avatars are routinely
/// larger than `MAX_STYLE_SPAN`; the renderer clamps to the box, so the cap only has to
/// stop absurdity. Padding stays tight because it really does move layout.
pub const MAX_STYLE_RADIUS: u32 = 512;
pub const MAX_PAGE_GAP: u32 = 128;
pub const MAX_GRID_COLUMNS: u32 = 12;
pub const MAX_PAGE_EXTENT: u32 = 8192;
/// Glyphs a page's `Icon` may name: the monochrome symbols every renderer bundles
/// (`symbol/<name>`), tinted with the icon's colour. An unknown name is refused by
/// `Page::validate` rather than drawn as nothing.
pub const PAGE_ICONS: &[&str] = &[
    "arrow-left",
    "arrow-right",
    "arrow-up",
    "bell",
    "calendar",
    "cast",
    "chat",
    "check",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "chevron-up",
    "clock",
    "close",
    "compass",
    "copy",
    "document",
    "download",
    "edit",
    "eye",
    "filters",
    "flag",
    "folder",
    "gear",
    "globe",
    "grid-view",
    "headphones",
    "heart",
    "heart-fill",
    "home",
    "image",
    "info",
    "library",
    "link",
    "list-view",
    "lock",
    "menu",
    "mic",
    "minus",
    "more",
    "more-vertical",
    "music",
    "pause",
    "person",
    "play",
    "plus",
    "queue",
    "radio",
    "reload",
    "repeat",
    "repeat-one",
    "reply",
    "search",
    "send",
    "share",
    "shuffle",
    "skip-next",
    "skip-previous",
    "sliders",
    "star",
    "star-outline",
    "tag",
    "thumb-up",
    "thumb-up-fill",
    "trash",
    "volume",
    "volume-mute",
];
/// `#rrggbb` or `#rrggbbaa`; nothing else, so renderers never guess.
/// Standard base64 (RFC 4648, padded or not; whitespace ignored), as world files carry
/// binary seeds.
pub fn decode_base64(text: &str) -> std::result::Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return Err(format!("invalid base64 character {:?}", c as char)),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}
pub fn valid_color(value: &str) -> bool {
    matches!(value.len(), 7 | 9)
        && value.starts_with('#')
        && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
/// Presentation hints. Absent fields inherit the page theme and renderer defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u16>,
    /// "regular" | "medium" | "bold"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<u32>,
    /// "left" | "center" | "right"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    /// Fixed pixel width. Omitted means "fill the available width".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Share of the leftover width inside a Row. Defaults to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex: Option<u32>,
    /// true renders the text on a single clipped line instead of wrapping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub one_line: Option<bool>,
    /// "bottom" keeps a top-level element on the bottom edge of the viewport while the
    /// rest of the page scrolls under it, as a site's player bar does. Honoured on
    /// top-level elements only; anywhere else it is ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    /// `true` on a `Row` lays its children out on one line at their own widths
    /// (`width`, or their min-content width) and, when they run past the row, lets it
    /// scroll sideways instead of wrapping: a shelf of album covers. The browser
    /// publishes it as a horizontal scroll area, `pane:row:<row id>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_x: Option<bool>,
}
/// Chainable presentation setters keep page-building call sites to one line each.
impl Style {
    pub fn size(mut self, v: u16) -> Self {
        self.size = Some(v);
        self
    }
    pub fn bold(mut self) -> Self {
        self.weight = Some("bold".into());
        self
    }
    pub fn medium(mut self) -> Self {
        self.weight = Some("medium".into());
        self
    }
    pub fn color(mut self, v: impl Into<String>) -> Self {
        self.color = Some(v.into());
        self
    }
    pub fn background(mut self, v: impl Into<String>) -> Self {
        self.background = Some(v.into());
        self
    }
    pub fn border(mut self, v: impl Into<String>) -> Self {
        self.border = Some(v.into());
        self
    }
    pub fn radius(mut self, v: u32) -> Self {
        self.radius = Some(v);
        self
    }
    pub fn padding(mut self, v: u32) -> Self {
        self.padding = Some(v);
        self
    }
    pub fn align(mut self, v: impl Into<String>) -> Self {
        self.align = Some(v.into());
        self
    }
    pub fn width(mut self, v: u32) -> Self {
        self.width = Some(v);
        self
    }
    pub fn height(mut self, v: u32) -> Self {
        self.height = Some(v);
        self
    }
    pub fn flex(mut self, v: u32) -> Self {
        self.flex = Some(v);
        self
    }
    pub fn one_line(mut self) -> Self {
        self.one_line = Some(true);
        self
    }
    pub fn pin(mut self, edge: impl Into<String>) -> Self {
        self.pin = Some(edge.into());
        self
    }
    pub fn scroll_x(mut self) -> Self {
        self.scroll_x = Some(true);
        self
    }
    fn validate(&self) -> Result<()> {
        for c in [&self.color, &self.background, &self.border]
            .into_iter()
            .flatten()
        {
            if !valid_color(c) {
                return Err(SimError::invalid(format!("invalid page colour {c}")));
            }
        }
        let over = |v: &Option<u32>, limit: u32| v.is_some_and(|v| v > limit);
        if over(&self.radius, MAX_STYLE_RADIUS) {
            return Err(SimError::invalid(format!(
                "style radius exceeds {MAX_STYLE_RADIUS}"
            )));
        }
        if over(&self.padding, MAX_STYLE_SPAN) {
            return Err(SimError::invalid(format!(
                "style padding exceeds {MAX_STYLE_SPAN}"
            )));
        }
        if over(&self.width, MAX_PAGE_EXTENT) || over(&self.height, MAX_PAGE_EXTENT) {
            return Err(SimError::invalid("style width or height exceeds 8192"));
        }
        if over(&self.flex, 64) {
            return Err(SimError::invalid("style flex exceeds 64"));
        }
        if self.size.is_some_and(|v| !(6..=96).contains(&v)) {
            return Err(SimError::invalid("style size must be 6 through 96"));
        }
        if self.pin.as_deref().is_some_and(|edge| edge != "bottom") {
            return Err(SimError::invalid("style pin must be bottom"));
        }
        Ok(())
    }
}
/// Page-wide palette. `content_width` centres the column on wider viewports.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageTheme {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ink: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_width: Option<u32>,
}
impl PageTheme {
    fn validate(&self) -> Result<()> {
        for c in [
            &self.accent,
            &self.background,
            &self.surface,
            &self.ink,
            &self.muted,
        ]
        .into_iter()
        .flatten()
        {
            if !valid_color(c) {
                return Err(SimError::invalid(format!("invalid theme colour {c}")));
            }
        }
        if self.content_width.is_some_and(|v| v > MAX_PAGE_EXTENT) {
            return Err(SimError::invalid("theme content width exceeds 8192"));
        }
        Ok(())
    }
}
impl PageElement {
    pub fn id(&self) -> &str {
        match self {
            Self::Heading { id, .. }
            | Self::Text { id, .. }
            | Self::Link { id, .. }
            | Self::Button { id, .. }
            | Self::Input { id, .. }
            | Self::Form { id, .. }
            | Self::Group { id, .. }
            | Self::Image { id, .. }
            | Self::Row { id, .. }
            | Self::Grid { id, .. }
            | Self::Card { id, .. }
            | Self::Styled { id, .. }
            | Self::Thumbnail { id, .. }
            | Self::Badge { id, .. }
            | Self::Divider { id, .. }
            | Self::Icon { id, .. }
            | Self::Spacer { id, .. } => id,
        }
    }
}
impl Page {
    /// Reject ambiguous interaction targets and unsupported native-page contracts.
    pub fn validate(&self) -> Result<()> {
        if self.version != SCHEMA_VERSION {
            return Err(SimError::invalid("unsupported page version"));
        }
        fn visit(elements: &[PageElement], ids: &mut BTreeSet<String>, depth: usize) -> Result<()> {
            if depth > 64 {
                return Err(SimError::invalid("page nesting exceeds 64 levels"));
            }
            for element in elements {
                if element.id().is_empty() || !ids.insert(element.id().to_owned()) {
                    return Err(SimError::invalid("empty or duplicate page element id"));
                }
                if ids.len() > 100_000 {
                    return Err(SimError::invalid("page exceeds element budget"));
                }
                match element {
                    PageElement::Heading { level, .. } if !(1..=6).contains(level) => {
                        return Err(SimError::invalid("heading level must be 1 through 6"))
                    }
                    PageElement::Group { children, .. } | PageElement::Form { children, .. } => {
                        visit(children, ids, depth + 1)?
                    }
                    PageElement::Row {
                        children,
                        gap,
                        style,
                        ..
                    } => {
                        style.validate()?;
                        if *gap > MAX_PAGE_GAP {
                            return Err(SimError::invalid("row gap exceeds 128"));
                        }
                        visit(children, ids, depth + 1)?
                    }
                    PageElement::Grid {
                        columns,
                        children,
                        gap,
                        style,
                        ..
                    } => {
                        style.validate()?;
                        if *gap > MAX_PAGE_GAP {
                            return Err(SimError::invalid("grid gap exceeds 128"));
                        }
                        if !(1..=MAX_GRID_COLUMNS).contains(columns) {
                            return Err(SimError::invalid("grid columns must be 1 through 12"));
                        }
                        visit(children, ids, depth + 1)?
                    }
                    PageElement::Card {
                        children, style, ..
                    } => {
                        style.validate()?;
                        visit(children, ids, depth + 1)?
                    }
                    PageElement::Icon {
                        name, label, style, ..
                    } => {
                        style.validate()?;
                        if !PAGE_ICONS.contains(&name.as_str()) {
                            return Err(SimError::invalid(format!("unknown page icon {name}")));
                        }
                        if label.trim().is_empty() {
                            return Err(SimError::invalid(
                                "an icon needs a label to be its accessible name",
                            ));
                        }
                    }
                    PageElement::Styled { style, .. }
                    | PageElement::Thumbnail { style, .. }
                    | PageElement::Badge { style, .. }
                    | PageElement::Divider { style, .. } => style.validate()?,
                    PageElement::Spacer { height, .. } if *height > MAX_PAGE_EXTENT => {
                        return Err(SimError::invalid("spacer height exceeds 8192"))
                    }
                    _ => (),
                }
            }
            Ok(())
        }
        if let Some(theme) = &self.theme {
            theme.validate()?;
        }
        visit(&self.elements, &mut BTreeSet::new(), 0)
    }
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            version: SCHEMA_VERSION,
            title: title.into(),
            elements: vec![],
            theme: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PageElement {
    Heading {
        id: String,
        text: String,
        level: u8,
    },
    Text {
        id: String,
        text: String,
    },
    Link {
        id: String,
        text: String,
        url: String,
    },
    Button {
        id: String,
        text: String,
        action: PageAction,
    },
    Input {
        id: String,
        label: String,
        value: String,
        #[serde(default)]
        placeholder: String,
    },
    Form {
        id: String,
        action: PageAction,
        children: Vec<PageElement>,
    },
    Group {
        id: String,
        children: Vec<PageElement>,
    },
    Image {
        id: String,
        source: String,
        alt: String,
        width: u32,
        height: u32,
    },
    /// Children laid out left to right. Fixed-width children take `Style::width`; the
    /// rest split the remainder by `Style::flex`. `align` is "start"|"center"|"end"|"stretch".
    Row {
        id: String,
        children: Vec<PageElement>,
        #[serde(default)]
        gap: u32,
        #[serde(default)]
        align: String,
        #[serde(default)]
        style: Style,
    },
    /// Children flowed into `columns` equal columns, row-major.
    Grid {
        id: String,
        columns: u32,
        children: Vec<PageElement>,
        #[serde(default)]
        gap: u32,
        #[serde(default)]
        style: Style,
    },
    /// A padded, filled, optionally bordered container. With `action`, the whole card
    /// is one click target: search results, video tiles, feed posts.
    Card {
        id: String,
        children: Vec<PageElement>,
        #[serde(default)]
        style: Style,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<PageAction>,
    },
    /// Text with explicit presentation; `Heading`/`Text` remain for plain content.
    Styled {
        id: String,
        text: String,
        #[serde(default)]
        style: Style,
    },
    /// Flat-colour stand-in for photography, video stills, avatars and logos. `label`
    /// is drawn centred and is the accessible name; it never claims to be a real photo.
    Thumbnail {
        id: String,
        label: String,
        #[serde(default)]
        style: Style,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<PageAction>,
    },
    /// Small pill: unread counts, "LIVE", "Ad", tags.
    Badge {
        id: String,
        text: String,
        #[serde(default)]
        style: Style,
    },
    /// 1px horizontal rule.
    Divider {
        id: String,
        #[serde(default)]
        style: Style,
    },
    /// A glyph from `PAGE_ICONS`, drawn `Style::size` pixels square (20 by default) in
    /// `Style::color`, padded by `Style::padding` over `Style::background`. `label` is its
    /// accessible name and is required. With `action` the padded square is one click
    /// target (a transport button, a like button); without, it is a picture.
    Icon {
        id: String,
        name: String,
        label: String,
        #[serde(default)]
        style: Style,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<PageAction>,
    },
    /// Vertical gap.
    Spacer {
        id: String,
        #[serde(default)]
        height: u32,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageAction {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionEnvelope {
    pub family: String,
    pub op: String,
    pub machine: String,
    #[serde(default)]
    pub payload: Value,
}
impl ActionEnvelope {
    pub fn new(
        family: impl Into<String>,
        op: impl Into<String>,
        machine: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            family: family.into(),
            op: op.into(),
            machine: machine.into(),
            payload,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    pub actor: String,
    pub machines: Vec<String>,
    pub actions: Vec<String>,
    pub observations: Vec<String>,
    #[serde(default = "default_budget")]
    pub action_budget: u32,
}
fn default_budget() -> u32 {
    1024
}
impl EnvironmentConfig {
    pub fn terminal(actor: impl Into<String>, machine: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            machines: vec![machine.into()],
            actions: vec!["terminal.v1".into()],
            observations: vec!["terminal.v1".into()],
            action_budget: default_budget(),
        }
    }
    pub fn desktop(actor: impl Into<String>, machine: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            machines: vec![machine.into()],
            actions: vec![
                "terminal.v1",
                "filesystem.v1",
                "browser.v1",
                "pointer.v1",
                "keyboard.v1",
                "application.v1",
                "http.v1",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            observations: vec!["terminal.v1".into(), "semantic.v1".into()],
            action_budget: default_budget(),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub tick: u64,
    #[serde(default)]
    pub channels: BTreeMap<String, Value>,
}
/// Tags `ActionEffect::changed` uses. Stable strings, so a consumer can match on them.
pub mod effect {
    pub const WINDOW_OPENED: &str = "window.opened";
    pub const WINDOW_CLOSED: &str = "window.closed";
    pub const WINDOW_MOVED: &str = "window.moved";
    pub const WINDOW_FOCUSED: &str = "window.focused";
    pub const WINDOW_TITLE: &str = "window.title";
    /// A pane's text content changed.
    pub const CONTENT: &str = "content";
    /// The path, URL or document a window presents changed.
    pub const DOCUMENT: &str = "document";
    /// The browser's current page changed.
    pub const NAVIGATE: &str = "navigate";
    /// Keyboard focus, the focused field or the keystroke route changed.
    pub const FOCUS: &str = "focus";
    /// The terminal observation channel changed.
    pub const TERMINAL: &str = "terminal";
    /// A registered application's projected page changed.
    pub const APPLICATION: &str = "application";
}
/// Coarse, app-level consequence of one action. `ActionOutcome::success` reports the
/// envelope; this reports what moved in the world the actor can see. Derived by
/// comparing an actor-visible projection of the target machine before and after
/// dispatch, so it is deterministic and costs no wall-clock or host I/O.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionEffect {
    /// Sorted `effect::*` tags. Empty means nothing the actor can observe changed,
    /// which is a real answer and not the same as failure.
    pub changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows_opened: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows_closed: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_window: Option<u64>,
    /// Browser location after the action, when the machine has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Digest of the actor-visible state of the target machine after this action.
    /// Content-derived, never counted: equal digests mean equal observable state, in
    /// this process or after a snapshot restore. Carried as a string because a 64-bit
    /// integer crosses the Wasm boundary as a BigInt, which `JSON.stringify` refuses.
    #[serde(with = "digest_text")]
    pub state: u64,
}
/// A `u64` on the wire as decimal text, so every binding can serialise it.
mod digest_text {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Text(String),
            Number(u64),
        }
        match Either::deserialize(d)? {
            Either::Text(t) => t.parse().map_err(serde::de::Error::custom),
            Either::Number(n) => Ok(n),
        }
    }
}
#[cfg(test)]
mod effect_wire_tests {
    use super::*;
    #[test]
    fn the_state_digest_is_text_on_the_wire_so_every_binding_can_serialise_it() {
        // A u64 reaches JavaScript as a BigInt, which `JSON.stringify` refuses; the
        // digest is an identity, never an arithmetic value, so text is the right shape.
        let effect = ActionEffect {
            changed: vec!["content".into()],
            windows_opened: vec![],
            windows_closed: vec![],
            focused_window: None,
            url: None,
            state: 10_516_701_560_454_250_893,
        };
        let json = serde_json::to_value(&effect).unwrap();
        assert_eq!(json["state"], serde_json::json!("10516701560454250893"));
        assert_eq!(
            serde_json::from_value::<ActionEffect>(json).unwrap().state,
            effect.state
        );
        // A number still deserialises, so a report written before this change loads.
        let legacy = serde_json::json!({"changed":[],"state":42});
        assert_eq!(
            serde_json::from_value::<ActionEffect>(legacy)
                .unwrap()
                .state,
            42
        );
    }
}
impl ActionEffect {
    /// The action was accepted and changed nothing observable.
    pub fn is_noop(&self) -> bool {
        self.changed.is_empty()
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionOutcome {
    pub index: usize,
    pub success: bool,
    #[serde(default)]
    pub value: Value,
    #[serde(default)]
    pub error: Option<SimError>,
    /// What this action changed. `None` when the environment could not attribute an
    /// effect, e.g. a denied action that never reached a machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<ActionEffect>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepResult {
    pub observation: Observation,
    pub outcomes: Vec<ActionOutcome>,
    pub tick: u64,
    #[serde(default)]
    pub pending: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    pub sequence: u64,
    pub tick: u64,
    pub kind: String,
    #[serde(default)]
    pub machine: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub data: Value,
}
impl WorldDefinition {
    pub fn from_json(json: &str) -> Result<Self> {
        let value: Self = serde_json::from_str(json)?;
        value.validate()?;
        Ok(value)
    }
    pub fn profile(&self, id: &str) -> Result<&OsProfile> {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| SimError::not_found(format!("profile {id}")))
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(SimError::invalid("unsupported world schema version"));
        }
        if self.id.trim().is_empty() {
            return Err(SimError::invalid("world id is empty"));
        }
        fn unique<'a>(
            items: impl Iterator<Item = &'a str>,
            kind: &str,
        ) -> Result<BTreeSet<String>> {
            let mut out = BTreeSet::new();
            for id in items {
                if id.trim().is_empty() || !out.insert(id.to_owned()) {
                    return Err(SimError::invalid(format!(
                        "empty or duplicate {kind} id: {id}"
                    )));
                }
            }
            Ok(out)
        }
        let profiles = unique(self.profiles.iter().map(|x| x.id.as_str()), "profile")?;
        unique(self.computers.iter().map(|x| x.id.as_str()), "computer")?;
        unique(self.services.iter().map(|x| x.id.as_str()), "service")?;
        let mut nodes = unique(self.network.nodes.iter().map(|x| x.id.as_str()), "node")?;
        let mut addresses: BTreeMap<String, String> = BTreeMap::new();
        for n in &self.network.nodes {
            if n.address.parse::<std::net::IpAddr>().is_err() {
                return Err(SimError::invalid(format!(
                    "invalid node address {}",
                    n.address
                )));
            }
            if addresses.insert(n.address.clone(), n.id.clone()).is_some() {
                return Err(SimError::invalid("duplicate network address"));
            }
        }
        let mut computer_nodes = BTreeSet::new();
        for c in &self.computers {
            if !computer_nodes.insert(c.node_id()) {
                return Err(SimError::invalid("multiple computers own one node"));
            }
            if !profiles.contains(&c.profile) {
                return Err(SimError::invalid(format!("unknown profile {}", c.profile)));
            }
            if c.user.is_empty() {
                return Err(SimError::invalid("computer user is empty"));
            }
            if c.address.parse::<std::net::IpAddr>().is_err() {
                return Err(SimError::invalid("invalid computer address"));
            }
            if let Some(n) = addresses.get(&c.address) {
                if n != c.node_id() {
                    return Err(SimError::invalid(
                        "computer address belongs to another node",
                    ));
                }
            } else {
                addresses.insert(c.address.clone(), c.node_id().to_string());
            }
            nodes.insert(c.node_id().to_owned());
            if let Some(n) = self.network.nodes.iter().find(|n| n.id == c.node_id()) {
                if n.address != c.address {
                    return Err(SimError::invalid("computer/node address mismatch"));
                }
            }
        }
        let mut listeners = BTreeSet::new();
        let mut domains = BTreeMap::new();
        for s in &self.services {
            if !nodes.contains(&s.node) {
                return Err(SimError::invalid(format!(
                    "unknown service node {}",
                    s.node
                )));
            }
            if s.kind.is_empty() || s.port == 0 {
                return Err(SimError::invalid("invalid service kind or port"));
            }
            if !listeners.insert((&s.node, s.port)) {
                return Err(SimError::invalid("duplicate service listener"));
            }
            for d in &s.domains {
                let d = d.trim_end_matches('.').to_ascii_lowercase();
                if d.is_empty() {
                    return Err(SimError::invalid("empty service domain"));
                }
                if let Some(old) = domains.insert(d, s.node.clone()) {
                    if old != s.node {
                        return Err(SimError::invalid("domain owned by different service nodes"));
                    }
                }
            }
        }
        for l in &self.network.links {
            if !nodes.contains(&l.from) || !nodes.contains(&l.to) || l.loss_per_million > 1_000_000
            {
                return Err(SimError::invalid("invalid network link"));
            }
        }
        for r in &self.network.routes {
            if !nodes.contains(&r.from)
                || !nodes.contains(&r.to)
                || r.via.as_ref().is_some_and(|v| !nodes.contains(v))
            {
                return Err(SimError::invalid("invalid route"));
            }
        }
        let mut dns = BTreeSet::new();
        for r in &self.network.dns {
            if r.name.is_empty()
                || !dns.insert(r.name.trim_end_matches('.').to_ascii_lowercase())
                || !valid_dns_target(&r.address)
                || r.resolver.as_ref().is_some_and(|v| !nodes.contains(v))
            {
                return Err(SimError::invalid("invalid DNS record"));
            }
        }
        for (name, node) in domains {
            if let Some(record) = self
                .network
                .dns
                .iter()
                .find(|r| r.name.trim_end_matches('.').eq_ignore_ascii_case(&name))
            {
                if let Some(owner) = addresses.get(&record.address) {
                    if owner != &node {
                        return Err(SimError::invalid(
                            "service domain DNS points to a different node",
                        ));
                    }
                }
            }
        }
        for (a, b) in &self.network.gateway.denied_pairs {
            if !nodes.contains(a) || !nodes.contains(b) {
                return Err(SimError::invalid("unknown gateway policy node"));
            }
        }
        Ok(())
    }
}
/// DNS aliases use the same record representation as address records.
/// An address literal is an A/AAAA answer; a DNS name is a CNAME target.
pub fn valid_dns_target(value: &str) -> bool {
    value.parse::<std::net::IpAddr>().is_ok() || {
        let value = value.strip_suffix('.').unwrap_or(value);
        !value.is_empty()
            && value.len() <= 253
            && value.split('.').all(|part| {
                !part.is_empty()
                    && part.len() <= 63
                    && !part.starts_with('-')
                    && !part.ends_with('-')
                    && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_decodes_padded_unpadded_and_wrapped_text() {
        assert_eq!(super::decode_base64("TWFu").unwrap(), b"Man");
        assert_eq!(super::decode_base64("TWE=").unwrap(), b"Ma");
        assert_eq!(super::decode_base64("TQ").unwrap(), b"M");
        assert_eq!(super::decode_base64("TW\nFu").unwrap(), b"Man");
        assert_eq!(super::decode_base64("AP8=").unwrap(), [0, 255]);
        assert!(super::decode_base64("T*").is_err());
    }
    use super::*;
    #[test]
    fn response_page_roundtrip() {
        let p = Page::new("hello");
        let r = HttpResponse::page(&p).unwrap();
        assert_eq!(r.header("Content-Type"), Some(PAGE_MEDIA_TYPE));
        assert_eq!(serde_json::from_slice::<Page>(&r.body).unwrap(), p)
    }
    #[test]
    fn default_gateway_cannot_escape() {
        assert!(!GatewayPolicy::default().allow_host)
    }
    #[test]
    fn rich_elements_validate_colours_and_nested_ids() {
        let card = |style: Style, child_id: &str| {
            let mut page = Page::new("p");
            page.elements = vec![
                PageElement::Text {
                    id: "a".into(),
                    text: "a".into(),
                },
                PageElement::Card {
                    id: "card".into(),
                    children: vec![PageElement::Text {
                        id: child_id.into(),
                        text: "b".into(),
                    }],
                    style,
                    action: None,
                },
            ];
            page.validate()
        };
        assert!(card(Style::default(), "b").is_ok());
        assert!(card(Style::default(), "a").is_err());
        assert!(card(
            Style {
                background: Some("rebeccapurple".into()),
                ..Style::default()
            },
            "b"
        )
        .is_err());
        assert!(card(
            Style {
                radius: Some(4096),
                ..Style::default()
            },
            "b"
        )
        .is_err());
        let mut page = Page::new("p");
        page.elements = vec![PageElement::Grid {
            id: "g".into(),
            columns: 0,
            children: vec![],
            gap: 8,
            style: Style::default(),
        }];
        assert!(page.validate().is_err());
    }
    #[test]
    fn icons_name_a_bundled_glyph_and_carry_a_label() {
        let icon = |name: &str, label: &str, style: Style| {
            let mut page = Page::new("p");
            page.elements = vec![PageElement::Icon {
                id: "like".into(),
                name: name.into(),
                label: label.into(),
                style,
                action: Some(PageAction {
                    method: "POST".into(),
                    url: "/items/x/like".into(),
                    fields: BTreeMap::new(),
                }),
            }];
            page.validate()
        };
        assert!(icon("thumb-up", "Like", Style::default()).is_ok());
        assert!(icon("thumbs-up", "Like", Style::default()).is_err());
        assert!(icon("thumb-up", " ", Style::default()).is_err());
        assert!(icon("heart", "Save", Style::default().color("red")).is_err());
        // It round-trips as the documented `icon` kind.
        let json = r#"{"kind":"icon","id":"i","name":"play","label":"Play"}"#;
        let parsed: PageElement = serde_json::from_str(json).unwrap();
        assert!(
            matches!(parsed, PageElement::Icon { ref name, ref action, .. } if name == "play" && action.is_none())
        );
    }
    #[test]
    fn legacy_pages_deserialize_without_theme() {
        let json = r#"{"version":1,"title":"Mail","elements":[
            {"kind":"heading","id":"h","text":"Inbox","level":1},
            {"kind":"group","id":"g","children":[{"kind":"text","id":"t","text":"hi"}]}]}"#;
        let page: Page = serde_json::from_str(json).unwrap();
        page.validate().unwrap();
        assert_eq!(page.theme, None);
        assert_eq!(page.elements.len(), 2);
        // A themeless page must also serialise back to the old shape.
        let back = serde_json::to_value(&page).unwrap();
        assert!(back.get("theme").is_none());
        assert_eq!(serde_json::from_value::<Page>(back).unwrap(), page);
    }
    #[test]
    fn unknown_schema_rejected() {
        let w = WorldDefinition {
            schema_version: 88,
            id: "w".into(),
            profiles: vec![],
            computers: vec![],
            network: NetworkDefinition::default(),
            services: vec![],
            metadata: Value::Null,
        };
        assert!(w.validate().is_err())
    }
}
