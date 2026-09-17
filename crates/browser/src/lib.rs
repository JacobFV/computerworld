//! Synthetic browser: all requests go through a supplied transport, never the host.
use cw_protocol::{
    HttpRequest, HttpResponse, Page, PageAction, PageElement, Result, SimError, PAGE_MEDIA_TYPE,
};
use cw_scene::{text_cell, wrap_text, Color, Node, Primitive, Rect, Scene, Semantic};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

pub const RGBA_MEDIA_TYPE: &str = "application/vnd.computerworld.rgba+json";
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub page: Page,
    pub status: u16,
    #[serde(default)]
    pub images: BTreeMap<String, Arc<ImageAsset>>,
    #[serde(default)]
    pub image_errors: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    pub history: Vec<HistoryEntry>,
    pub position: usize,
    pub focused: Option<String>,
    pub fields: BTreeMap<String, String>,
    pub scroll_y: i32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub path: String,
    pub secure: bool,
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
}
impl Default for BrowserState {
    fn default() -> Self {
        Self {
            tabs: vec![Tab::default()],
            active: 0,
            storage: BTreeMap::new(),
            cookies: BTreeMap::new(),
            pending: None,
            image_cache: BTreeMap::new(),
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
    pub fn page(&self) -> Option<&Page> {
        self.tab().history.get(self.tab().position).map(|e| &e.page)
    }
    pub fn url(&self) -> Option<&str> {
        self.tab()
            .history
            .get(self.tab().position)
            .map(|e| e.url.as_str())
    }
    pub fn new_tab(&mut self) -> usize {
        self.tabs.push(Tab::default());
        self.active = self.tabs.len() - 1;
        self.active
    }
    pub fn switch_tab(&mut self, index: usize) -> Result<()> {
        if index >= self.tabs.len() {
            return Err(SimError::not_found("tab"));
        }
        self.active = index;
        Ok(())
    }
    pub fn close_tab(&mut self, index: usize) -> Result<()> {
        if index >= self.tabs.len() {
            return Err(SimError::not_found("tab"));
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.tabs.push(Tab::default())
        }
        if self.active > index {
            self.active -= 1
        }
        self.active = self.active.min(self.tabs.len() - 1);
        Ok(())
    }
    fn resolve(&self, url: &str) -> Result<Url> {
        let parsed = Url::parse(url)
            .or_else(|_| {
                self.url()
                    .and_then(|base| Url::parse(base).ok())
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
    pub fn back<F>(&mut self, _transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.tab().position == 0 {
            return Err(SimError::not_found("previous history entry"));
        }
        self.tab_mut().position -= 1;
        self.reset_fields();
        Ok(())
    }
    pub fn forward<F>(&mut self, _transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        if self.tab().position + 1 >= self.tab().history.len() {
            return Err(SimError::not_found("next history entry"));
        }
        self.tab_mut().position += 1;
        self.reset_fields();
        Ok(())
    }
    fn reset_fields(&mut self) {
        let mut fields = BTreeMap::new();
        if let Some(page) = self.page() {
            walk(&page.elements, &mut |e| {
                if let PageElement::Input { id, value, .. } = e {
                    fields.insert(id.clone(), value.clone());
                }
            })
        }
        let tab = self.tab_mut();
        tab.fields = fields;
        tab.focused = None;
        tab.scroll_y = 0;
    }
    fn request<F>(
        &mut self,
        mut request: HttpRequest,
        transport: &mut F,
        replace: bool,
    ) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
        for _ in 0..=16 {
            let mut url = self.resolve(&request.url)?;
            url.set_fragment(None);
            request.url = url.to_string();
            let origin = url.origin().ascii_serialization();
            request.headers.remove("cookie");
            if let Some(cookies) = self.cookies.get(&origin) {
                let value = cookies
                    .iter()
                    .filter(|c| {
                        cookie_path_matches(url.path(), &c.path)
                            && (!c.secure || url.scheme() == "https")
                    })
                    .map(|c| format!("{}={}", c.name, c.value))
                    .collect::<Vec<_>>()
                    .join("; ");
                if !value.is_empty() {
                    request.headers.insert("cookie".into(), value);
                }
            }
            self.pending = Some(request.clone());
            let result = transport(request.clone());
            self.pending = None;
            let response = result?;
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
            let page = if response
                .header("content-type")
                .is_some_and(|v| v.split(';').next().unwrap_or("").trim() == PAGE_MEDIA_TYPE)
            {
                let page: Page = serde_json::from_slice(&response.body)?;
                page.validate()?;
                page
            } else {
                let mut page = Page::new(url.as_str());
                page.elements.push(PageElement::Text {
                    id: "response".into(),
                    text: String::from_utf8_lossy(&response.body).into_owned(),
                });
                page
            };
            let (images, image_errors) = self.load_images(&page, &url, transport, replace);
            let entry = HistoryEntry {
                url: url.to_string(),
                page,
                status: response.status,
                images,
                image_errors,
            };
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
            return Ok(());
        }
        Err(SimError::new("redirect_limit", "more than 16 redirects"))
    }
    pub fn fill(&mut self, id: &str, value: &str) -> Result<()> {
        if !self.tab().fields.contains_key(id) {
            return Err(SimError::not_found(format!("input {id}")));
        }
        self.tab_mut().fields.insert(id.into(), value.into());
        self.tab_mut().focused = Some(id.into());
        Ok(())
    }
    pub fn text(&mut self, text: &str) -> Result<()> {
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
            _ => Err(SimError::invalid("element is not interactive")),
        }
    }
    pub fn submit<F>(&mut self, id: &str, transport: &mut F) -> Result<()>
    where
        F: FnMut(HttpRequest) -> Result<HttpResponse>,
    {
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
            url.query_pairs_mut().extend_pairs(fields);
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
                entry.page.validate()?;
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
        let mut url = base
            .join(source)
            .map_err(|e| SimError::invalid(e.to_string()))?;
        url.set_fragment(None);
        let original = url.to_string();
        // Native page assets use a deliberately strict same-origin policy. The
        // transport still enforces DNS/routing/gateway grants on every request.
        if url.origin() != base.origin() || !url.username().is_empty() || url.password().is_some() {
            return Err(SimError::denied("cross-origin native image"));
        }
        if !refresh {
            if let Some(asset) = self.image_cache.get(&original) {
                return Ok(asset.clone());
            }
        }
        for _ in 0..=8 {
            if url.origin() != base.origin()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(SimError::denied(
                    "cross-origin or credentialed image redirect",
                ));
            }
            let mut request = HttpRequest::get(url.as_str());
            if let Some(cookies) = self.cookies.get(&url.origin().ascii_serialization()) {
                let value = cookies
                    .iter()
                    .filter(|c| {
                        cookie_path_matches(url.path(), &c.path)
                            && (!c.secure || url.scheme() == "https")
                    })
                    .map(|c| format!("{}={}", c.name, c.value))
                    .collect::<Vec<_>>()
                    .join("; ");
                if !value.is_empty() {
                    request.headers.insert("cookie".into(), value);
                }
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
            if response
                .header("content-type")
                .and_then(|v| v.split(';').next())
                != Some(RGBA_MEDIA_TYPE)
            {
                return Err(SimError::new(
                    "image_format",
                    "unsupported native image media type",
                ));
            }
            if response.body.len() > MAX_IMAGE_BYTES * 4 + 1024 {
                return Err(SimError::invalid("image response exceeds budget"));
            }
            let asset: ImageAsset = serde_json::from_slice(&response.body)?;
            asset.validate()?;
            let asset = Arc::new(asset);
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
        match self.tab().history.get(self.tab().position) {
            Some(entry) => layout_page_with_images(
                &entry.page,
                &self.tab().fields,
                &entry.images,
                width,
                height,
                self.tab().scroll_y,
            ),
            None => Scene::new(width, height),
        }
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
    };
    for p in parts {
        let p = p.trim();
        if p.eq_ignore_ascii_case("secure") {
            cookie.secure = true
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
fn walk(elements: &[PageElement], f: &mut impl FnMut(&PageElement)) {
    for e in elements {
        f(e);
        match e {
            PageElement::Form { children, .. } | PageElement::Group { children, .. } => {
                walk(children, f)
            }
            _ => {}
        }
    }
}
fn element_id(e: &PageElement) -> &str {
    match e {
        PageElement::Heading { id, .. }
        | PageElement::Text { id, .. }
        | PageElement::Link { id, .. }
        | PageElement::Button { id, .. }
        | PageElement::Input { id, .. }
        | PageElement::Form { id, .. }
        | PageElement::Group { id, .. }
        | PageElement::Image { id, .. } => id,
    }
}
fn find<'a>(elements: &'a [PageElement], id: &str) -> Option<&'a PageElement> {
    for e in elements {
        if element_id(e) == id {
            return Some(e);
        }
        if let PageElement::Form { children, .. } | PageElement::Group { children, .. } = e {
            if let Some(v) = find(children, id) {
                return Some(v);
            }
        }
    }
    None
}
fn parent_form<'a>(elements: &'a [PageElement], id: &str) -> Option<&'a PageAction> {
    for e in elements {
        match e {
            PageElement::Form {
                action, children, ..
            } if find(children, id).is_some() => return Some(action),
            PageElement::Group { children, .. } => {
                if let Some(a) = parent_form(children, id) {
                    return Some(a);
                }
            }
            _ => {}
        }
    }
    None
}
fn find_form_scope<'a>(elements: &'a [PageElement], id: &str) -> Option<&'a [PageElement]> {
    for e in elements {
        match e {
            PageElement::Form {
                id: fid, children, ..
            } if fid == id || find(children, id).is_some() => return Some(children),
            PageElement::Group { children, .. } => {
                if let Some(a) = find_form_scope(children, id) {
                    return Some(a);
                }
            }
            _ => {}
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
    let mut scene = Scene::new(width, height);
    let mut y = 16i32.saturating_sub(scroll_y);
    let id = 1u64;
    let mut title = Node::text(
        id,
        Rect::new(16, y, width.saturating_sub(32), 28),
        &page.title,
        20,
        Color::rgb(20, 32, 52),
    );
    title.semantic = Some(Semantic {
        role: "heading".into(),
        label: page.title.clone(),
        ..Semantic::default()
    });
    scene.nodes.push(title);
    y = y.saturating_add(40);
    let mut used_ids = std::collections::BTreeSet::from([0u64, 1u64]);
    walk(&page.elements, &mut |element| {
        // Semantic IDs yield stable primitive identities across insertions and text
        // changes; paired IDs distinguish control backgrounds from text runs.
        let mut hash = 0xcbf29ce484222325u64;
        for byte in element_id(element).bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        // Keep IDs exactly representable in JavaScript Number as well as Rust.
        const NODE_ID_MASK: u64 = (1u64 << 53) - 2;
        let mut id = hash & NODE_ID_MASK;
        while used_ids.contains(&id) {
            id = id.wrapping_add(2) & NODE_ID_MASK;
        }
        used_ids.insert(id);
        used_ids.insert(id + 1);
        if let PageElement::Image {
            id: element_id,
            alt,
            width: image_width,
            height: image_height,
            ..
        } = element
        {
            if let Some(asset) = images.get(element_id) {
                let draw_width = if *image_width == 0 {
                    asset.width
                } else {
                    *image_width
                }
                .min(width.saturating_sub(32));
                let draw_height = if *image_height == 0 {
                    asset.height
                } else {
                    *image_height
                }
                .min(4096);
                let mut node = Node::new(
                    id,
                    Rect::new(16, y, draw_width, draw_height),
                    Primitive::Image {
                        width: asset.width,
                        height: asset.height,
                        rgba: asset.rgba.clone(),
                    },
                );
                node.semantic = Some(Semantic {
                    role: "img".into(),
                    label: alt.clone(),
                    ..Semantic::default()
                });
                node.clip = Some(Rect::new(0, 0, width, height));
                scene.nodes.push(node);
                y = y.saturating_add(draw_height as i32).saturating_add(8);
                return;
            }
        }
        let (text, size, interaction, role, value) = match element {
            PageElement::Heading { text, level, .. } => (
                text.clone(),
                if *level <= 1 { 20 } else { 16 },
                None,
                "heading",
                None,
            ),
            PageElement::Text { text, .. } => (text.clone(), 14, None, "text", None),
            PageElement::Link { id, text, .. } => {
                (text.clone(), 14, Some(id.clone()), "link", None)
            }
            PageElement::Button { id, text, .. } => {
                (text.clone(), 14, Some(id.clone()), "button", None)
            }
            PageElement::Input {
                id, label, value, ..
            } => {
                let value = fields.get(id).unwrap_or(value).clone();
                (
                    format!("{label}: {value}"),
                    14,
                    Some(id.clone()),
                    "textbox",
                    Some(value),
                )
            }
            PageElement::Image { alt, .. } => (format!("[image: {alt}]"), 14, None, "img", None),
            PageElement::Group { .. } | PageElement::Form { .. } => return,
        };
        let (cell, line) = text_cell(size);
        let lines = wrap_text(&text, (width.saturating_sub(48) / cell).max(1) as usize);
        let h = (lines.len() as u32).saturating_mul(line).saturating_add(12);
        let bounds = Rect::new(16, y, width.saturating_sub(32), h);
        if interaction.is_some() {
            let mut bg = Node::rectangle(
                id,
                bounds,
                if role == "textbox" {
                    Color::rgb(244, 247, 252)
                } else {
                    Color::rgb(228, 237, 252)
                },
            );
            bg.interaction = interaction.clone();
            bg.semantic = Some(Semantic {
                role: role.into(),
                label: text.clone(),
                value: value.clone(),
                focusable: true,
                ..Semantic::default()
            });
            scene.nodes.push(bg);
            id += 1;
        }
        let mut node = Node::text(
            id,
            Rect::new(
                22,
                y.saturating_add(6),
                width.saturating_sub(44),
                h.saturating_sub(12),
            ),
            lines.join("\n"),
            size,
            if role == "link" {
                Color::rgb(25, 78, 178)
            } else {
                Color::rgb(25, 32, 45)
            },
        );
        node.interaction = interaction;
        node.semantic = Some(Semantic {
            role: role.into(),
            label: text,
            value,
            focusable: node.interaction.is_some(),
            ..Semantic::default()
        });
        node.clip = Some(Rect::new(0, 0, width, height));
        scene.nodes.push(node);
        y = y
            .saturating_add(h.min(i32::MAX as u32) as i32)
            .saturating_add(8);
    });
    scene
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
                    },
                ],
            },
        ];
        p
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
