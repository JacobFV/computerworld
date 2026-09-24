//! An HTML document shown in a tab: the parsed DOM, its stylesheets, the pictures it
//! references, and the interaction state a session keeps for it (scroll, hover, focus,
//! the caret, and what the person changed in its form controls). `cw_web` does the
//! cascade, layout and paint; this module owns the document lifecycle around it.
//!
//! What persists (the snapshot) is the DOM, the stylesheet sources, the decoded images
//! and the interaction state. The cascade and the fragment tree are derived and rebuilt
//! on demand from those, cached until something they depend on changes, so a frame that
//! repeats the last one costs a scene clone and nothing else.
//!
//! Live form state: typed text lives in the tab's `fields` map keyed by the control's
//! interaction id (the same map the `Page` renderer uses, so `fill`, `text` and the
//! `browser.v1` observation keep one shape). Checkedness of checkboxes and radios,
//! selectedness of options and `<details open>` are written into the DOM attributes,
//! which is what the cascade (`:checked`), the paint and the semantics all read; the
//! attribute values the page came with are remembered for `<button type=reset>`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use cw_protocol::{HttpRequest, Page, PageAction, PageElement, Result, SimError};
use cw_scene::{AxNode, Scene};
use cw_web::css::{
    self, ComponentValue, FormState, MatchContext, Media, MediaQueryList, Origin, Rule, Stylesheet,
    Token,
};
use cw_web::dom::{Document, NodeId, NodeKind};
use cw_web::geom::{Au, Point};
use cw_web::layout::{
    self, FragmentKind, FragmentTree, ImageSizes, LayoutCache, LayoutOptions, ScrollState,
};
use cw_web::paint::{self, semantics, ImageCache, PaintContext, RgbaImage};
use cw_web::style::{self, Cursor, StyleSet};
use cw_web::{Strictness, Viewport};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::scripted::Scripted;
use crate::ImageAsset;

mod script_path;

/// `@import` chains deeper than this are dropped, as browsers cap them.
pub const MAX_IMPORT_DEPTH: u32 = 3;
/// Pictures fetched for one document, at most.
pub const MAX_IMAGES: usize = 64;
/// Stylesheets fetched for one document, at most.
pub const MAX_SHEETS: usize = 32;

/// A stylesheet as it came in: where it came from (for resolving `url()`s inside it),
/// the `media` it applies under, and its text. Kept in cascade order: an `@import`ed
/// sheet precedes the sheet that imported it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetSource {
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub media: String,
    pub source: String,
}

/// What a click, a key or a submission asks the browser to do next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Navigate {
        url: String,
        new_tab: bool,
    },
    Request(HttpRequest),
    /// Scroll the document to `y` CSS px (a fragment link).
    ScrollTo(i32),
}

/// Everything a render of the document takes from the tab.
#[derive(Clone, Copy, Debug)]
pub struct Inputs<'a> {
    pub width: u32,
    pub height: u32,
    /// Percent; 100 is unzoomed.
    pub zoom: u16,
    pub fields: &'a BTreeMap<String, String>,
    pub focused: Option<&'a str>,
    /// Root scroll offset in CSS px.
    pub scroll_y: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RenderKey {
    generation: u64,
    width: u32,
    height: u32,
    zoom: u16,
    fields: BTreeMap<String, String>,
    focused: Option<String>,
    scroll_y: i32,
}

/// One render: the styles and fragments for a key, and the scene painted from them.
struct Render {
    key: RenderKey,
    /// The state the styles were computed under, to restyle incrementally.
    hovered: BTreeSet<NodeId>,
    focused: Option<NodeId>,
    target: Option<String>,
    css_width: u32,
    css_height: u32,
    styles: StyleSet,
    tree: FragmentTree,
    /// Root scroll (clamped) and inner scroll offsets as painted, for hit testing.
    scroll: Point,
    scroll_offsets: BTreeMap<NodeId, Point>,
    scene: Scene,
}

#[derive(Default)]
struct Cache {
    sheets: Option<Vec<Stylesheet>>,
    decoded: Option<BTreeMap<String, RgbaImage>>,
    layout: LayoutCache,
    render: Option<Render>,
    /// A scripted document's last painted scene, keyed by the realm's epoch.
    script_render: Option<script_path::ScriptRender>,
    /// A scripted document projected for the page readers, keyed the same way.
    projection: Option<(u64, Arc<WebDocument>)>,
}

/// The derived state behind a lock. `StyleSet` holds `Rc`s, which is why this wrapper
/// exists.
///
/// SAFETY: every `Rc` inside a `Cache` is created inside the render and dropped with
/// it; none is ever cloned out of the lock (`StyleSet::get_rc` is never called here,
/// and the engine's own clones live in a `BoxTree` that `layout_with` drops before it
/// returns, or in a `Painter` dropped by `paint`). The reference counts are therefore
/// only ever touched by the thread holding the mutex, which is what `Send` promises.
struct SendCache(Cache);
unsafe impl Send for SendCache {}

/// An HTML document and its session state. See the module documentation.
pub struct WebDocument {
    doc: Document,
    /// The URL the document was fetched from, fragment included.
    pub url: String,
    /// The base URL relative references resolve against (`<base href>`, else `url`).
    pub base: String,
    /// The `<title>`, whitespace-collapsed; empty when the page has none.
    pub title: String,
    sheets: Vec<SheetSource>,
    /// Decoded pictures by resolved URL.
    images: BTreeMap<String, Arc<ImageAsset>>,
    /// Why a picture could not be shown, by resolved URL.
    pub image_errors: BTreeMap<String, String>,
    /// Raw reference as written in the page (`src`, `url()`) to its resolved URL: what
    /// layout and paint ask for, keyed the way they ask.
    refs: BTreeMap<String, String>,
    /// The `checked`/`selected`/`open` state the page came with, for `type=reset`.
    defaults: BTreeMap<NodeId, bool>,
    /// Inner scroll containers by interaction id, `(x, y)` in CSS px.
    scrolls: BTreeMap<String, (i32, i32)>,
    hover: Option<NodeId>,
    /// The `:target` element's id, from the URL fragment or a fragment link.
    target: Option<String>,
    /// Caret in the focused text control, in characters; `None` is the end.
    caret: Option<usize>,
    /// A message for the person (a blocked submission), painted under the page.
    pub notice: Option<String>,
    /// Bumped by every change the render depends on.
    generation: u64,
    /// The realm of a document with script. It owns the DOM, the styles and the
    /// layout; `doc` is then empty and `sheets` unused.
    script: Option<Scripted>,
    cache: Mutex<SendCache>,
}

/// The persisted shape of a document.
#[derive(Serialize)]
struct SnapshotRef<'a> {
    doc: &'a Document,
    url: &'a str,
    base: &'a str,
    title: &'a str,
    sheets: &'a [SheetSource],
    images: &'a BTreeMap<String, Arc<ImageAsset>>,
    image_errors: &'a BTreeMap<String, String>,
    refs: &'a BTreeMap<String, String>,
    defaults: Vec<(NodeId, bool)>,
    scrolls: &'a BTreeMap<String, (i32, i32)>,
    hover: Option<NodeId>,
    target: &'a Option<String>,
    caret: Option<usize>,
    notice: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    script: &'a Option<Scripted>,
}

#[derive(Deserialize)]
struct Snapshot {
    doc: Document,
    url: String,
    base: String,
    title: String,
    #[serde(default)]
    sheets: Vec<SheetSource>,
    #[serde(default)]
    images: BTreeMap<String, Arc<ImageAsset>>,
    #[serde(default)]
    image_errors: BTreeMap<String, String>,
    #[serde(default)]
    refs: BTreeMap<String, String>,
    #[serde(default)]
    defaults: Vec<(NodeId, bool)>,
    #[serde(default)]
    scrolls: BTreeMap<String, (i32, i32)>,
    #[serde(default)]
    hover: Option<NodeId>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    caret: Option<usize>,
    #[serde(default)]
    notice: Option<String>,
    #[serde(default)]
    script: Option<Scripted>,
}

impl Serialize for WebDocument {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        SnapshotRef {
            doc: &self.doc,
            url: &self.url,
            base: &self.base,
            title: &self.title,
            sheets: &self.sheets,
            images: &self.images,
            image_errors: &self.image_errors,
            refs: &self.refs,
            defaults: self.defaults.iter().map(|(n, v)| (*n, *v)).collect(),
            scrolls: &self.scrolls,
            hover: self.hover,
            target: &self.target,
            caret: self.caret,
            notice: &self.notice,
            script: &self.script,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for WebDocument {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = Snapshot::deserialize(d)?;
        Ok(WebDocument {
            doc: s.doc,
            url: s.url,
            base: s.base,
            title: s.title,
            sheets: s.sheets,
            images: s.images,
            image_errors: s.image_errors,
            refs: s.refs,
            defaults: s.defaults.into_iter().collect(),
            scrolls: s.scrolls,
            hover: s.hover,
            target: s.target,
            caret: s.caret,
            notice: s.notice,
            generation: 0,
            script: s.script,
            cache: Mutex::new(SendCache(Cache::default())),
        })
    }
}

impl Clone for WebDocument {
    fn clone(&self) -> Self {
        WebDocument {
            doc: self.doc.clone(),
            url: self.url.clone(),
            base: self.base.clone(),
            title: self.title.clone(),
            sheets: self.sheets.clone(),
            images: self.images.clone(),
            image_errors: self.image_errors.clone(),
            refs: self.refs.clone(),
            defaults: self.defaults.clone(),
            scrolls: self.scrolls.clone(),
            hover: self.hover,
            target: self.target.clone(),
            caret: self.caret,
            notice: self.notice.clone(),
            generation: self.generation,
            script: self.script.clone(),
            cache: Mutex::new(SendCache(Cache::default())),
        }
    }
}

impl PartialEq for WebDocument {
    fn eq(&self, o: &Self) -> bool {
        self.doc == o.doc
            && self.url == o.url
            && self.base == o.base
            && self.title == o.title
            && self.sheets == o.sheets
            && self.images == o.images
            && self.image_errors == o.image_errors
            && self.refs == o.refs
            && self.defaults == o.defaults
            && self.scrolls == o.scrolls
            && self.hover == o.hover
            && self.target == o.target
            && self.caret == o.caret
            && self.notice == o.notice
            && self.script == o.script
    }
}
impl Eq for WebDocument {}

impl std::fmt::Debug for WebDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebDocument")
            .field("url", &self.url)
            .field("title", &self.title)
            .field("nodes", &self.doc.len())
            .field("sheets", &self.sheets.len())
            .field("images", &self.images.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------------
// Construction and loading
// ---------------------------------------------------------------------------------

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// The document a browser shows for a `text/plain` response.
pub fn text_document(text: &str, url: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{}</title></head><body><pre style=\"word-wrap: break-word; white-space: pre-wrap;\">{}</pre></body></html>",
        escape_html(url),
        escape_html(text)
    )
}

/// The document a browser shows for an `image/*` response: the picture, centred.
pub fn image_document(url: &str, size: Option<(u32, u32)>) -> String {
    let name = url.rsplit('/').next().unwrap_or(url);
    let title = match size {
        Some((w, h)) => format!("{name} ({w}\u{D7}{h})"),
        None => name.to_owned(),
    };
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{}</title><style>html,body{{height:100%;margin:0}}body{{background:#0e0e0e}}table{{width:100%;height:100%;border-collapse:collapse}}td{{text-align:center;vertical-align:middle}}img{{display:inline-block}}</style></head><body><table><tr><td><img src=\"{}\" alt=\"{}\"></td></tr></table></body></html>",
        escape_html(&title),
        escape_html(url),
        escape_html(name)
    )
}

impl WebDocument {
    /// Parses `html` fetched from `url`. Inline `<style>` sheets are registered; the
    /// caller fetches `<link rel=stylesheet>` and pictures (`stylesheet_links`,
    /// `image_references`) and hands them in with `add_linked_sheet` and `add_image`.
    pub fn parse(html: &str, url: &str) -> WebDocument {
        let doc = cw_web::html::parse_with_url(html, url);
        let mut web = WebDocument {
            doc,
            url: url.to_owned(),
            base: url.to_owned(),
            title: String::new(),
            sheets: Vec::new(),
            images: BTreeMap::new(),
            image_errors: BTreeMap::new(),
            refs: BTreeMap::new(),
            defaults: BTreeMap::new(),
            scrolls: BTreeMap::new(),
            hover: None,
            target: None,
            caret: None,
            notice: None,
            generation: 0,
            script: None,
            cache: Mutex::new(SendCache(Cache::default())),
        };
        web.base = web.find_base();
        web.title = web.find_title();
        web.target = Url::parse(url)
            .ok()
            .and_then(|u| u.fragment().map(str::to_owned))
            .filter(|f| !f.is_empty());
        for node in web.doc.descendants(Document::ROOT) {
            let d = &web.doc;
            if d.is(node, "input") {
                let t = input_type(d, node);
                if t == "checkbox" || t == "radio" {
                    web.defaults.insert(node, d.has_attr(node, "checked"));
                }
            } else if d.is(node, "option") {
                web.defaults.insert(node, d.has_attr(node, "selected"));
            } else if d.is(node, "details") {
                web.defaults.insert(node, d.has_attr(node, "open"));
            }
        }
        web
    }

    fn find_base(&self) -> String {
        let Some(head) = self.doc.head() else {
            return self.url.clone();
        };
        for n in self.doc.descendants(head) {
            if self.doc.is(n, "base") {
                if let Some(href) = self.doc.attr(n, "href") {
                    if let Some(u) = Url::parse(&self.url)
                        .ok()
                        .and_then(|u| u.join(href.trim()).ok())
                    {
                        if matches!(u.scheme(), "http" | "https") {
                            return u.to_string();
                        }
                    }
                }
                break;
            }
        }
        self.url.clone()
    }

    fn find_title(&self) -> String {
        self.doc
            .descendants(Document::ROOT)
            .find(|n| self.doc.is(*n, "title"))
            .map(|n| semantics::collapse(&self.doc.text_content(n)))
            .unwrap_or_default()
    }

    /// Resolves a reference against the document base; `None` for what the browser
    /// cannot fetch (non-http schemes, credentials).
    pub fn resolve(&self, reference: &str) -> Option<Url> {
        let base = Url::parse(&self.base).ok()?;
        let u = base.join(reference.trim()).ok()?;
        (matches!(u.scheme(), "http" | "https")
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none())
        .then_some(u)
    }

    /// `<meta http-equiv=refresh content="5; url=/x">`, as the `Refresh` header value.
    pub fn meta_refresh(&self) -> Option<String> {
        let head = self.doc.head()?;
        self.doc.descendants(head).find_map(|n| {
            if !self.doc.is(n, "meta") {
                return None;
            }
            let equiv = self.doc.attr(n, "http-equiv")?;
            if !equiv.trim().eq_ignore_ascii_case("refresh") {
                return None;
            }
            self.doc.attr(n, "content").map(|c| c.replace(',', ";"))
        })
    }

    /// The sheets the document links, in document order, as `(absolute url, media)`,
    /// interleaved with the inline `<style>` elements the caller need not fetch. The
    /// caller walks this list and calls `add_inline_sheet` or `add_linked_sheet` for
    /// each entry in order, so the cascade order is the document's.
    pub fn sheet_plan(&self) -> Vec<SheetPlan> {
        let mut out = Vec::new();
        for n in self.doc.descendants(Document::ROOT) {
            let d = &self.doc;
            if d.is(n, "style") {
                let media = d.attr(n, "media").unwrap_or("").trim().to_owned();
                out.push(SheetPlan::Inline {
                    media,
                    source: d.text_content(n),
                });
            } else if d.is(n, "link") {
                let rel = d.attr(n, "rel").unwrap_or("");
                let is_sheet = rel
                    .split_ascii_whitespace()
                    .any(|r| r.eq_ignore_ascii_case("stylesheet"));
                let is_alternate = rel
                    .split_ascii_whitespace()
                    .any(|r| r.eq_ignore_ascii_case("alternate"));
                if !is_sheet || is_alternate || d.has_attr(n, "disabled") {
                    continue;
                }
                let Some(href) = d.attr(n, "href") else {
                    continue;
                };
                let Some(url) = self.resolve(href) else {
                    continue;
                };
                let media = d.attr(n, "media").unwrap_or("").trim().to_owned();
                out.push(SheetPlan::Linked {
                    url: url.to_string(),
                    media,
                });
            }
        }
        out
    }

    /// Registers a sheet's text, fetching what it `@import`s first (through `fetch`,
    /// which returns the text of a URL or `None`) so the cascade order is right.
    pub fn add_sheet(
        &mut self,
        url: &str,
        media: &str,
        source: String,
        fetch: &mut dyn FnMut(&str) -> Option<String>,
    ) {
        self.add_sheet_at(url, media, source, 0, fetch);
    }

    fn add_sheet_at(
        &mut self,
        url: &str,
        media: &str,
        source: String,
        depth: u32,
        fetch: &mut dyn FnMut(&str) -> Option<String>,
    ) {
        if self.sheets.len() >= MAX_SHEETS {
            return;
        }
        let parsed =
            css::parse_stylesheet(&source, Origin::Author, Strictness::Lenient).unwrap_or_default();
        if depth < MAX_IMPORT_DEPTH {
            let imports: Vec<(String, String)> = parsed
                .imports()
                .map(|(u, m)| (u.to_owned(), m.to_string()))
                .collect();
            for (reference, import_media) in imports {
                let Some(abs) = Url::parse(url)
                    .ok()
                    .and_then(|b| b.join(reference.trim()).ok())
                else {
                    continue;
                };
                if !matches!(abs.scheme(), "http" | "https")
                    || self.sheets.iter().any(|s| s.url == abs.as_str())
                {
                    continue;
                }
                if let Some(text) = fetch(abs.as_str()) {
                    let combined = if media.is_empty() {
                        import_media
                    } else if import_media.is_empty() {
                        media.to_owned()
                    } else {
                        format!("{media} and {import_media}")
                    };
                    self.add_sheet_at(abs.as_str(), &combined, text, depth + 1, fetch);
                }
            }
        }
        self.sheets.push(SheetSource {
            url: url.to_owned(),
            media: media.to_owned(),
            source,
        });
        self.invalidate_styles();
    }

    /// Every picture the document and its sheets reference, as `(reference, resolved
    /// URL)`: `<img src>`, `<input type=image src>`, and `url()` in stylesheets and
    /// `style` attributes. Deduplicated, in document order, capped at `MAX_IMAGES`.
    pub fn image_references(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut seen = BTreeSet::new();
        let mut push =
            |reference: String, resolved: Option<Url>, out: &mut Vec<(String, String)>| {
                if let Some(u) = resolved {
                    let mut u = u;
                    u.set_fragment(None);
                    if seen.insert(reference.clone()) && out.len() < MAX_IMAGES {
                        out.push((reference, u.to_string()));
                    }
                }
            };
        for n in self.doc.descendants(Document::ROOT) {
            let d = &self.doc;
            let is_img = d.is(n, "img") || (d.is(n, "input") && input_type(d, n) == "image");
            if is_img {
                if let Some(src) = d.attr(n, "src") {
                    if !src.trim().is_empty() {
                        push(src.to_owned(), self.resolve(src), &mut out);
                    }
                }
            }
            if let Some(style) = d.attr(n, "style") {
                for u in css_urls(
                    &css::parse_declarations(style)
                        .iter()
                        .flat_map(|d| d.value.iter())
                        .cloned()
                        .collect::<Vec<_>>(),
                ) {
                    push(u.clone(), self.resolve(&u), &mut out);
                }
            }
        }
        for sheet in &self.sheets {
            let parsed = css::parse_stylesheet(&sheet.source, Origin::Author, Strictness::Lenient)
                .unwrap_or_default();
            let base = Url::parse(&sheet.url).ok();
            for u in sheet_urls(&parsed.rules) {
                let resolved = base
                    .as_ref()
                    .and_then(|b| b.join(u.trim()).ok())
                    .filter(|r| {
                        matches!(r.scheme(), "http" | "https")
                            && r.username().is_empty()
                            && r.password().is_none()
                    });
                push(u, resolved, &mut out);
            }
        }
        out
    }

    /// Records a fetched picture under the reference the page used for it.
    pub fn add_image(&mut self, reference: &str, resolved: &str, asset: Arc<ImageAsset>) {
        self.refs.insert(reference.to_owned(), resolved.to_owned());
        self.images.insert(resolved.to_owned(), asset);
        self.image_errors.remove(resolved);
        self.invalidate_images();
    }

    pub fn add_image_error(&mut self, reference: &str, resolved: &str, code: &str) {
        self.refs.insert(reference.to_owned(), resolved.to_owned());
        self.image_errors
            .insert(resolved.to_owned(), code.to_owned());
    }

    /// The decoded pictures, by resolved URL.
    pub fn images(&self) -> &BTreeMap<String, Arc<ImageAsset>> {
        &self.images
    }

    /// The stylesheets in cascade order.
    pub fn sheets(&self) -> &[SheetSource] {
        &self.sheets
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    fn touch(&mut self) {
        self.generation += 1;
    }

    fn invalidate_styles(&mut self) {
        self.touch();
        if let Ok(mut c) = self.cache.lock() {
            c.0.sheets = None;
            c.0.render = None;
        }
    }

    fn invalidate_images(&mut self) {
        self.touch();
        if let Ok(mut c) = self.cache.lock() {
            c.0.decoded = None;
            c.0.render = None;
        }
    }

    // -----------------------------------------------------------------------------
    // Element lookup and form model
    // -----------------------------------------------------------------------------

    /// The element an interaction id names: an `id` attribute, or the path the paint
    /// emits for elements without one (`/html/body/div[2]/a[1]`).
    pub fn node_for(&self, id: &str) -> Option<NodeId> {
        if let Some(s) = &self.script {
            return s.read(|realm| node_in(&realm.document(), id));
        }
        node_in(&self.doc, id)
    }

    /// The interaction id of an element, as the paint emits it.
    pub fn id_of(&self, node: NodeId) -> String {
        if let Some(s) = &self.script {
            return s.read(|realm| semantics::interaction_id(&realm.document(), node));
        }
        semantics::interaction_id(&self.doc, node)
    }
}

/// The element an interaction id names in `doc`.
pub(crate) fn node_in(doc: &Document, id: &str) -> Option<NodeId> {
    Finder { doc }.node_for(id)
}

struct Finder<'a> {
    doc: &'a Document,
}

impl Finder<'_> {
    fn node_for(&self, id: &str) -> Option<NodeId> {
        if let Some(n) = self.doc.by_id(id).first() {
            if !self.doc.node(*n).detached {
                return Some(*n);
            }
        }
        let path = id.strip_prefix('/')?;
        let mut cur = Document::ROOT;
        for step in path.split('/') {
            let (tag, rest) = step.split_once('[')?;
            let index: usize = rest.strip_suffix(']')?.parse().ok()?;
            cur = self
                .doc
                .children(cur)
                .filter(|c| self.doc.tag(*c) == Some(tag))
                .nth(index.checked_sub(1)?)?;
        }
        Some(cur)
    }
}

impl WebDocument {
    /// The form an element belongs to: its `form` attribute, else the nearest ancestor.
    pub fn form_owner(&self, node: NodeId) -> Option<NodeId> {
        if let Some(id) = self.doc.attr(node, "form") {
            if let Some(f) = self.doc.by_id(id).first() {
                if self.doc.is(*f, "form") {
                    return Some(*f);
                }
            }
        }
        self.doc.ancestors(node).find(|a| self.doc.is(*a, "form"))
    }

    /// Text controls the tab's `fields` map holds: `(interaction id, default value)`.
    pub fn initial_fields(&self) -> BTreeMap<String, String> {
        if self.script.is_some() {
            return self.script_fields();
        }
        let mut out = BTreeMap::new();
        for n in self.doc.descendants(Document::ROOT) {
            if self.is_text_control(n) {
                out.insert(self.id_of(n), self.default_value(n));
            }
        }
        out
    }

    pub fn is_text_control(&self, node: NodeId) -> bool {
        if let Some(s) = &self.script {
            return s.read(|realm| realm.layout().is_text_control(node));
        }
        match self.doc.tag(node) {
            Some("textarea") => true,
            Some("input") => is_text_input_type(&input_type(&self.doc, node)),
            _ => false,
        }
    }

    fn default_value(&self, node: NodeId) -> String {
        if self.doc.is(node, "textarea") {
            let t = self.doc.text_content(node);
            t.strip_prefix('\n').unwrap_or(&t).to_owned()
        } else {
            self.doc.attr(node, "value").unwrap_or("").to_owned()
        }
    }

    fn value_of(&self, node: NodeId, fields: &BTreeMap<String, String>) -> String {
        fields
            .get(&self.id_of(node))
            .cloned()
            .unwrap_or_else(|| self.default_value(node))
    }

    /// `(interaction id, element)` of every focusable element in tab order.
    pub fn tab_order(&self) -> Vec<NodeId> {
        if self.script.is_some() {
            return self.projection().tab_order();
        }
        let mut positive: Vec<(i32, usize, NodeId)> = Vec::new();
        let mut rest: Vec<NodeId> = Vec::new();
        for (i, n) in self.doc.descendants(Document::ROOT).enumerate() {
            if !self.doc.is_element(n)
                || !semantics::is_focusable(&self.doc, n)
                || semantics::is_disabled(&self.doc, n)
            {
                continue;
            }
            if self
                .doc
                .ancestors(n)
                .chain(std::iter::once(n))
                .any(|a| self.doc.has_attr(a, "hidden"))
            {
                continue;
            }
            match self
                .doc
                .attr(n, "tabindex")
                .and_then(|t| t.trim().parse::<i32>().ok())
            {
                Some(t) if t > 0 => positive.push((t, i, n)),
                Some(t) if t < 0 => {}
                _ => rest.push(n),
            }
        }
        positive.sort();
        positive
            .into_iter()
            .map(|(_, _, n)| n)
            .chain(rest)
            .collect()
    }

    fn radio_group(&self, radio: NodeId) -> Vec<NodeId> {
        let name = self.doc.attr(radio, "name").unwrap_or("");
        let owner = self.form_owner(radio);
        if name.is_empty() {
            return vec![radio];
        }
        self.doc
            .descendants(Document::ROOT)
            .filter(|n| {
                self.doc.is(*n, "input")
                    && input_type(&self.doc, *n) == "radio"
                    && self.doc.attr(*n, "name") == Some(name)
                    && self.form_owner(*n) == owner
            })
            .collect()
    }

    fn set_flag(&mut self, node: NodeId, name: &str, on: bool) {
        let has = self.doc.has_attr(node, name);
        if on && !has {
            self.doc.set_attr(node, name, "");
        } else if !on && has {
            self.doc.remove_attr(node, name);
        }
        if on != has {
            self.touch();
        }
    }

    /// Whether a checkbox, radio or option is on.
    pub fn is_checked(&self, node: NodeId) -> bool {
        if self.doc.is(node, "option") {
            self.doc.has_attr(node, "selected")
        } else {
            self.doc.has_attr(node, "checked")
        }
    }

    fn select_option(&mut self, select: NodeId, option: NodeId) {
        let multiple = self.doc.has_attr(select, "multiple");
        let options: Vec<NodeId> = self
            .doc
            .descendants(select)
            .filter(|n| self.doc.is(*n, "option"))
            .collect();
        for o in options {
            if o == option {
                self.set_flag(o, "selected", true);
            } else if !multiple {
                self.set_flag(o, "selected", false);
            }
        }
    }

    /// Picks a `<select>`'s option by value, then by label; the `fill` of a select.
    pub fn choose(&mut self, select: NodeId, value: &str) -> Result<()> {
        let options: Vec<NodeId> = self
            .doc
            .descendants(select)
            .filter(|n| self.doc.is(*n, "option"))
            .collect();
        let wanted = value.trim();
        let by_value = options
            .iter()
            .copied()
            .find(|o| self.option_value(*o) == wanted);
        let by_label = || {
            options
                .iter()
                .copied()
                .find(|o| semantics::collapse(&self.doc.text_content(*o)) == wanted)
        };
        let by_label_ci = || {
            options.iter().copied().find(|o| {
                semantics::collapse(&self.doc.text_content(*o)).eq_ignore_ascii_case(wanted)
            })
        };
        let Some(option) = by_value.or_else(by_label).or_else(by_label_ci) else {
            return Err(SimError::not_found(format!("option {value}")));
        };
        self.select_option(select, option);
        Ok(())
    }

    fn option_value(&self, option: NodeId) -> String {
        match self.doc.attr(option, "value") {
            Some(v) => v.to_owned(),
            None => semantics::collapse(&self.doc.text_content(option)),
        }
    }

    /// Cycles a `<select>` to its next option (a click with no picker).
    fn cycle_select(&mut self, select: NodeId) {
        let options: Vec<NodeId> = self
            .doc
            .descendants(select)
            .filter(|n| self.doc.is(*n, "option") && !semantics::is_disabled(&self.doc, *n))
            .collect();
        if options.is_empty() {
            return;
        }
        let current = semantics::selected_option(&self.doc, select);
        let next = match current.and_then(|c| options.iter().position(|o| *o == c)) {
            Some(i) => options[(i + 1) % options.len()],
            None => options[0],
        };
        self.select_option(select, next);
    }

    fn step_select(&mut self, select: NodeId, delta: i32) {
        let options: Vec<NodeId> = self
            .doc
            .descendants(select)
            .filter(|n| self.doc.is(*n, "option") && !semantics::is_disabled(&self.doc, *n))
            .collect();
        if options.is_empty() {
            return;
        }
        let current = semantics::selected_option(&self.doc, select)
            .and_then(|c| options.iter().position(|o| *o == c))
            .unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, options.len() as i32 - 1) as usize;
        self.select_option(select, options[next]);
    }

    fn reset_form(&mut self, form: Option<NodeId>, fields: &mut BTreeMap<String, String>) {
        let nodes: Vec<NodeId> = self.doc.descendants(Document::ROOT).collect();
        for n in nodes {
            if self.form_owner(n) != form && !(form.is_none() && self.doc.is(n, "details")) {
                continue;
            }
            if self.is_text_control(n) {
                fields.insert(self.id_of(n), self.default_value(n));
            } else if let Some(default) = self.defaults.get(&n).copied() {
                let attr = if self.doc.is(n, "option") {
                    "selected"
                } else if self.doc.is(n, "details") {
                    continue;
                } else {
                    "checked"
                };
                self.set_flag(n, attr, default);
            }
        }
        self.caret = None;
        self.touch();
    }

    // -----------------------------------------------------------------------------
    // Actions
    // -----------------------------------------------------------------------------

    /// The element a click on `node` activates: the nearest interactive ancestor or
    /// self.
    fn activation_target(&self, node: NodeId) -> Option<NodeId> {
        std::iter::once(node)
            .chain(self.doc.ancestors(node))
            .find(|n| {
                let d = &self.doc;
                match d.tag(*n) {
                    Some("a" | "area") => d.has_attr(*n, "href"),
                    Some(
                        "button" | "input" | "select" | "textarea" | "label" | "summary" | "option",
                    ) => true,
                    Some(_) => semantics::is_focusable(d, *n) || semantics::is_interactive(d, *n),
                    None => false,
                }
            })
    }

    /// Clicks the element: its default action. `fields` and `focused` are the tab's.
    pub fn click(
        &mut self,
        node: NodeId,
        fields: &mut BTreeMap<String, String>,
        focused: &mut Option<String>,
    ) -> Result<Outcome> {
        self.notice = None;
        let Some(target) = self.activation_target(node) else {
            return Err(SimError::invalid("element is not interactive"));
        };
        if semantics::is_disabled(&self.doc, target) {
            return Err(SimError::invalid("element is disabled"));
        }
        self.touch();
        let d = &self.doc;
        let tag = d.tag(target).unwrap_or("").to_owned();
        match tag.as_str() {
            "a" | "area" => {
                self.focus(Some(target), focused);
                self.follow_link(target)
            }
            "button" => {
                let kind = d
                    .attr(target, "type")
                    .map(|t| t.trim().to_ascii_lowercase())
                    .unwrap_or_else(|| "submit".into());
                self.focus(Some(target), focused);
                match kind.as_str() {
                    "submit" => match self.form_owner(target) {
                        Some(form) => self
                            .submit_form(form, Some(target), fields)
                            .map(Outcome::Request),
                        None => Ok(Outcome::Nothing),
                    },
                    "reset" => {
                        let form = self.form_owner(target);
                        self.reset_form(form, fields);
                        Ok(Outcome::Nothing)
                    }
                    _ => Ok(Outcome::Nothing),
                }
            }
            "input" => {
                let kind = input_type(d, target);
                match kind.as_str() {
                    "checkbox" => {
                        self.focus(Some(target), focused);
                        let on = !self.is_checked(target);
                        self.set_flag(target, "checked", on);
                        Ok(Outcome::Nothing)
                    }
                    "radio" => {
                        self.focus(Some(target), focused);
                        self.check_radio(target);
                        Ok(Outcome::Nothing)
                    }
                    "submit" | "image" => {
                        self.focus(Some(target), focused);
                        match self.form_owner(target) {
                            Some(form) => self
                                .submit_form(form, Some(target), fields)
                                .map(Outcome::Request),
                            None => Ok(Outcome::Nothing),
                        }
                    }
                    "reset" => {
                        self.focus(Some(target), focused);
                        let form = self.form_owner(target);
                        self.reset_form(form, fields);
                        Ok(Outcome::Nothing)
                    }
                    "hidden" => Err(SimError::invalid("element is not interactive")),
                    _ => {
                        self.focus(Some(target), focused);
                        Ok(Outcome::Nothing)
                    }
                }
            }
            "select" => {
                self.focus(Some(target), focused);
                self.cycle_select(target);
                Ok(Outcome::Nothing)
            }
            "option" => {
                if let Some(select) = d.ancestors(target).find(|a| d.is(*a, "select")) {
                    self.focus(Some(select), focused);
                    self.select_option(select, target);
                }
                Ok(Outcome::Nothing)
            }
            "textarea" => {
                self.focus(Some(target), focused);
                Ok(Outcome::Nothing)
            }
            "label" => match self.labeled_control(target) {
                Some(control) if control != node => self.click(control, fields, focused),
                _ => Ok(Outcome::Nothing),
            },
            "summary" => {
                let details = d.parent(target).filter(|p| d.is(*p, "details"));
                let open = details.is_some_and(|x| !d.has_attr(x, "open"));
                self.focus(Some(target), focused);
                if let Some(details) = details {
                    self.set_flag(details, "open", open);
                }
                Ok(Outcome::Nothing)
            }
            _ => {
                if semantics::is_focusable(d, target) {
                    self.focus(Some(target), focused);
                } else if d.is(target, "details") {
                    let open = !d.has_attr(target, "open");
                    self.set_flag(target, "open", open);
                }
                Ok(Outcome::Nothing)
            }
        }
    }

    fn check_radio(&mut self, radio: NodeId) {
        for other in self.radio_group(radio) {
            self.set_flag(other, "checked", other == radio);
        }
    }

    /// The control a `<label>` is for: its `for`, else its first labelable descendant.
    fn labeled_control(&self, label: NodeId) -> Option<NodeId> {
        if let Some(id) = self.doc.attr(label, "for") {
            return self.doc.by_id(id).first().copied();
        }
        self.doc.descendants(label).find(|n| {
            *n != label
                && matches!(
                    self.doc.tag(*n),
                    Some("input" | "button" | "select" | "textarea")
                )
                && !(self.doc.is(*n, "input") && input_type(&self.doc, *n) == "hidden")
        })
    }

    fn focus(&mut self, node: Option<NodeId>, focused: &mut Option<String>) {
        let next = node.map(|n| self.id_of(n));
        if *focused != next {
            self.caret = None;
            self.touch();
        }
        *focused = next;
    }

    fn follow_link(&mut self, link: NodeId) -> Result<Outcome> {
        let href = self.doc.attr(link, "href").unwrap_or("").trim().to_owned();
        if let Some(fragment) = href.strip_prefix('#') {
            return Ok(self.go_to_fragment(fragment));
        }
        let Some(url) = self.resolve(&href) else {
            return Err(SimError::denied(
                "browser supports credential-free http/https URLs only",
            ));
        };
        let new_tab = self
            .doc
            .attr(link, "target")
            .is_some_and(|t| t.trim().eq_ignore_ascii_case("_blank"));
        // A link to this document with a fragment scrolls rather than reloads.
        if !new_tab {
            if let (Some(here), Some(fragment)) = (Url::parse(&self.url).ok(), url.fragment()) {
                let mut a = here.clone();
                a.set_fragment(None);
                let mut b = url.clone();
                b.set_fragment(None);
                if a == b {
                    return Ok(self.go_to_fragment(fragment));
                }
            }
        }
        Ok(Outcome::Navigate {
            url: url.to_string(),
            new_tab,
        })
    }

    /// Sets `:target` and finds where the fragment's element sits, in CSS px from the
    /// top of the document (0 for an empty or unknown fragment, as browsers do).
    fn go_to_fragment(&mut self, fragment: &str) -> Outcome {
        let fragment = percent_decode(fragment);
        if let Ok(mut u) = Url::parse(&self.url) {
            u.set_fragment((!fragment.is_empty()).then_some(fragment.as_str()));
            self.url = u.to_string();
        }
        self.target = (!fragment.is_empty()).then(|| fragment.clone());
        self.touch();
        if fragment.is_empty() || fragment == "top" && self.doc.by_id(&fragment).is_empty() {
            return Outcome::ScrollTo(0);
        }
        let Some(node) = self.doc.by_id(&fragment).first().copied().or_else(|| {
            self.doc.descendants(Document::ROOT).find(|n| {
                self.doc.is(*n, "a") && self.doc.attr(*n, "name") == Some(fragment.as_str())
            })
        }) else {
            return Outcome::Nothing;
        };
        let y = self
            .with_layout(|tree| {
                tree.rects_of(node)
                    .first()
                    .map(|r| r.origin.y.to_px_floor().max(0))
            })
            .flatten()
            .unwrap_or(0);
        Outcome::ScrollTo(y)
    }

    /// Goes to a fragment of this document (`#id`): sets `:target` and says where to
    /// scroll.
    pub fn jump_to(&mut self, fragment: &str) -> Outcome {
        self.go_to_fragment(fragment)
    }

    /// Puts the caret at the end of the focused control (after `fill`).
    pub fn set_caret_end(&mut self) {
        if self.caret.is_some() {
            self.caret = None;
            self.touch();
        }
    }

    /// The fragment tree at the last rendered viewport (or the default one), for
    /// geometry questions outside a render.
    fn with_layout<T>(&self, f: impl FnOnce(&FragmentTree) -> T) -> Option<T> {
        let guard = self.cache.lock().ok()?;
        if let Some(r) = &guard.0.render {
            if r.key.generation == self.generation {
                return Some(f(&r.tree));
            }
        }
        drop(guard);
        let (w, h, zoom) = self.last_viewport();
        let empty = BTreeMap::new();
        let guard = self.render(Inputs {
            width: w,
            height: h,
            zoom,
            fields: &empty,
            focused: None,
            scroll_y: 0,
        });
        guard.0.render.as_ref().map(|r| f(&r.tree))
    }

    /// The viewport of the last render, else the engine default.
    pub fn last_viewport(&self) -> (u32, u32, u16) {
        if self.script.is_some() {
            return self
                .cache
                .lock()
                .ok()
                .and_then(|c| {
                    c.0.script_render
                        .as_ref()
                        .map(|r| (r.width, r.height, r.zoom))
                })
                .unwrap_or((1280, 800, 100));
        }
        self.cache
            .lock()
            .ok()
            .and_then(|c| {
                c.0.render
                    .as_ref()
                    .map(|r| (r.key.width, r.key.height, r.key.zoom))
            })
            .unwrap_or((1280, 800, 100))
    }

    /// Types into the focused control at the caret.
    pub fn insert_text(
        &mut self,
        text: &str,
        fields: &mut BTreeMap<String, String>,
        focused: &Option<String>,
    ) -> Result<()> {
        let id = focused
            .clone()
            .ok_or_else(|| SimError::invalid("no focused input"))?;
        let node = self
            .node_for(&id)
            .ok_or_else(|| SimError::not_found("input"))?;
        if !self.is_text_control(node) {
            return Err(SimError::invalid("focused element is not a text control"));
        }
        if semantics::is_disabled(&self.doc, node) || self.doc.has_attr(node, "readonly") {
            return Err(SimError::invalid("input is read-only"));
        }
        let text = if self.doc.is(node, "textarea") {
            text.to_owned()
        } else {
            text.replace(['\n', '\r'], "")
        };
        let value = fields.entry(id).or_default();
        let chars: Vec<char> = value.chars().collect();
        let at = self.caret.map_or(chars.len(), |c| c.min(chars.len()));
        let mut next: String = chars[..at].iter().collect();
        next.push_str(&text);
        next.extend(chars[at..].iter());
        let max = self
            .doc
            .attr(node, "maxlength")
            .and_then(|m| m.trim().parse::<usize>().ok());
        if let Some(max) = max {
            next = next.chars().take(max).collect();
        }
        *value = next;
        self.caret = self.caret.map(|c| c + text.chars().count());
        self.touch();
        Ok(())
    }

    /// A key press: what browsers do with it when the page has no script.
    pub fn key(
        &mut self,
        key: &str,
        fields: &mut BTreeMap<String, String>,
        focused: &mut Option<String>,
    ) -> Result<Outcome> {
        let (name, shift) = match key.strip_prefix("Shift+") {
            Some(rest) => (rest, true),
            None => (key, false),
        };
        if name == "Tab" {
            let order = self.tab_order();
            if order.is_empty() {
                self.focus(None, focused);
                return Ok(Outcome::Nothing);
            }
            let current = focused
                .as_deref()
                .and_then(|id| self.node_for(id))
                .and_then(|n| order.iter().position(|o| *o == n));
            let next = match (current, shift) {
                (Some(i), false) => order[(i + 1) % order.len()],
                (Some(i), true) => order[(i + order.len() - 1) % order.len()],
                (None, false) => order[0],
                (None, true) => order[order.len() - 1],
            };
            self.focus(Some(next), focused);
            return Ok(Outcome::Nothing);
        }
        if name == "Escape" {
            self.notice = None;
            self.focus(None, focused);
            return Ok(Outcome::Nothing);
        }
        let id = focused
            .clone()
            .ok_or_else(|| SimError::invalid("no focused input"))?;
        let node = self
            .node_for(&id)
            .ok_or_else(|| SimError::not_found("input"))?;
        let text = self.is_text_control(node);
        let d = &self.doc;
        let tag = d.tag(node).unwrap_or("").to_owned();
        let kind = input_type(d, node);
        match name {
            "Enter" => {
                if tag == "textarea" {
                    return self
                        .insert_text("\n", fields, focused)
                        .map(|_| Outcome::Nothing);
                }
                if text {
                    // Implicit submission: the form's default button is the submitter.
                    let Some(form) = self.form_owner(node) else {
                        return Err(SimError::invalid("input has no form"));
                    };
                    let submitter = self.default_button(form);
                    return self
                        .submit_form(form, submitter, fields)
                        .map(Outcome::Request);
                }
                if tag == "a"
                    || tag == "button"
                    || tag == "summary"
                    || (tag == "input"
                        && matches!(
                            kind.as_str(),
                            "submit" | "image" | "reset" | "button" | "checkbox" | "radio"
                        ))
                {
                    return self.click(node, fields, focused);
                }
                Ok(Outcome::Nothing)
            }
            " " | "Space" => {
                if text {
                    return self
                        .insert_text(" ", fields, focused)
                        .map(|_| Outcome::Nothing);
                }
                if tag == "button"
                    || tag == "summary"
                    || (tag == "input"
                        && matches!(
                            kind.as_str(),
                            "submit" | "image" | "reset" | "button" | "checkbox" | "radio"
                        ))
                {
                    return self.click(node, fields, focused);
                }
                Ok(Outcome::Nothing)
            }
            "Backspace" | "Delete" => {
                if !text {
                    return Ok(Outcome::Nothing);
                }
                if d.has_attr(node, "readonly") {
                    return Err(SimError::invalid("input is read-only"));
                }
                let value = fields.entry(id).or_default();
                let mut chars: Vec<char> = value.chars().collect();
                let at = self.caret.map_or(chars.len(), |c| c.min(chars.len()));
                if name == "Backspace" {
                    if at > 0 {
                        chars.remove(at - 1);
                        self.caret = self.caret.map(|c| c.saturating_sub(1));
                    }
                } else if at < chars.len() {
                    chars.remove(at);
                }
                *value = chars.into_iter().collect();
                self.touch();
                Ok(Outcome::Nothing)
            }
            "Home" => {
                if text {
                    self.caret = Some(0);
                    self.touch();
                }
                Ok(Outcome::Nothing)
            }
            "End" => {
                if text {
                    self.caret = None;
                    self.touch();
                }
                Ok(Outcome::Nothing)
            }
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" => {
                let forward = matches!(name, "ArrowRight" | "ArrowDown");
                if tag == "input" && kind == "radio" {
                    let group = self.radio_group(node);
                    let i = group.iter().position(|g| *g == node).unwrap_or(0);
                    let next = if forward {
                        group[(i + 1) % group.len()]
                    } else {
                        group[(i + group.len() - 1) % group.len()]
                    };
                    self.check_radio(next);
                    self.focus(Some(next), focused);
                    return Ok(Outcome::Nothing);
                }
                if tag == "select" {
                    self.step_select(node, if forward { 1 } else { -1 });
                    return Ok(Outcome::Nothing);
                }
                if text && matches!(name, "ArrowLeft" | "ArrowRight") {
                    let len = fields.get(&id).map_or(0, |v| v.chars().count());
                    let at = self.caret.map_or(len, |c| c.min(len));
                    self.caret = if forward {
                        if at + 1 >= len {
                            None
                        } else {
                            Some(at + 1)
                        }
                    } else {
                        Some(at.saturating_sub(1))
                    };
                    self.touch();
                }
                Ok(Outcome::Nothing)
            }
            other => Err(SimError::invalid(format!(
                "unsupported browser key {other}"
            ))),
        }
    }

    /// The form's default button: the first submit button in tree order.
    fn default_button(&self, form: NodeId) -> Option<NodeId> {
        self.doc.descendants(Document::ROOT).find(|n| {
            let d = &self.doc;
            let is_submit = match d.tag(*n) {
                Some("button") => d
                    .attr(*n, "type")
                    .is_none_or(|t| t.trim().eq_ignore_ascii_case("submit")),
                Some("input") => matches!(input_type(d, *n).as_str(), "submit" | "image"),
                _ => false,
            };
            is_submit && self.form_owner(*n) == Some(form)
        })
    }

    /// Submits a form (or the form of the element `id` names), with no submitter
    /// unless `id` is a submit button.
    pub fn submit(&mut self, id: &str, fields: &BTreeMap<String, String>) -> Result<HttpRequest> {
        self.notice = None;
        let node = self
            .node_for(id)
            .ok_or_else(|| SimError::not_found("form"))?;
        let (form, submitter) = if self.doc.is(node, "form") {
            (node, None)
        } else {
            let form = self
                .form_owner(node)
                .ok_or_else(|| SimError::not_found("form"))?;
            let is_button = matches!(self.doc.tag(node), Some("button"))
                || (self.doc.is(node, "input")
                    && matches!(input_type(&self.doc, node).as_str(), "submit" | "image"));
            (form, is_button.then_some(node))
        };
        self.submit_form(form, submitter, fields)
    }

    /// The form data set and the request that carries it (HTML §4.10.21).
    fn submit_form(
        &mut self,
        form: NodeId,
        submitter: Option<NodeId>,
        fields: &BTreeMap<String, String>,
    ) -> Result<HttpRequest> {
        let d = &self.doc;
        let attr = |name: &str| -> Option<String> {
            submitter
                .and_then(|s| d.attr(s, &format!("form{name}")))
                .or_else(|| d.attr(form, name))
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let method = attr("method")
            .map(|m| m.to_ascii_uppercase())
            .filter(|m| m == "POST" || m == "GET" || m == "DIALOG")
            .unwrap_or_else(|| "GET".into());
        let enctype = attr("enctype")
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        let action = attr("action").unwrap_or_default();
        let url = if action.is_empty() {
            let mut u = Url::parse(&self.url).map_err(|e| SimError::invalid(e.to_string()))?;
            u.set_fragment(None);
            u
        } else {
            self.resolve(&action).ok_or_else(|| {
                SimError::denied("browser supports credential-free http/https URLs only")
            })?
        };
        // Constraint validation: a required field left empty blocks the submission.
        for n in d.descendants(Document::ROOT) {
            if self.form_owner(n) != Some(form)
                || !d.has_attr(n, "required")
                || semantics::is_disabled(d, n)
            {
                continue;
            }
            let kind = input_type(d, n);
            let missing = if self.is_text_control(n) {
                self.value_of(n, fields).trim().is_empty()
            } else if d.is(n, "input") && kind == "checkbox" {
                !self.is_checked(n)
            } else if d.is(n, "input") && kind == "radio" {
                !self.radio_group(n).iter().any(|r| self.is_checked(*r))
            } else if d.is(n, "select") {
                semantics::selected_option(d, n).is_none_or(|o| self.option_value(o).is_empty())
            } else {
                false
            };
            if missing {
                let label = semantics::label_of(d, &semantics::Tables::build(d), n);
                let message = if label.is_empty() {
                    "Please fill out this field.".to_owned()
                } else {
                    format!("Please fill out this field: {label}")
                };
                self.notice = Some(message.clone());
                self.generation += 1;
                return Err(SimError::invalid(message));
            }
        }
        // The form data set, in tree order.
        let mut entries: Vec<(String, String, bool)> = Vec::new();
        for n in d.descendants(Document::ROOT) {
            if self.form_owner(n) != Some(form) || semantics::is_disabled(d, n) {
                continue;
            }
            let name = d.attr(n, "name").unwrap_or("").to_owned();
            match d.tag(n) {
                Some("input") => {
                    let kind = input_type(d, n);
                    match kind.as_str() {
                        "submit" => {
                            if Some(n) == submitter && !name.is_empty() {
                                entries.push((
                                    name,
                                    d.attr(n, "value").unwrap_or("").to_owned(),
                                    false,
                                ));
                            }
                        }
                        "image" => {
                            if Some(n) == submitter {
                                let prefix = if name.is_empty() {
                                    String::new()
                                } else {
                                    format!("{name}.")
                                };
                                entries.push((format!("{prefix}x"), "0".into(), false));
                                entries.push((format!("{prefix}y"), "0".into(), false));
                            }
                        }
                        "button" | "reset" => {}
                        "checkbox" | "radio" => {
                            if !name.is_empty() && self.is_checked(n) {
                                entries.push((
                                    name,
                                    d.attr(n, "value").unwrap_or("on").to_owned(),
                                    false,
                                ));
                            }
                        }
                        "file" => {
                            if !name.is_empty() {
                                entries.push((name, String::new(), true));
                            }
                        }
                        _ => {
                            if !name.is_empty() {
                                let value = if kind == "hidden" {
                                    d.attr(n, "value").unwrap_or("").to_owned()
                                } else {
                                    self.value_of(n, fields)
                                };
                                entries.push((name, value, false));
                            }
                        }
                    }
                }
                Some("button") => {
                    let kind = d
                        .attr(n, "type")
                        .map(|t| t.trim().to_ascii_lowercase())
                        .unwrap_or_else(|| "submit".into());
                    if kind == "submit" && Some(n) == submitter && !name.is_empty() {
                        entries.push((name, d.attr(n, "value").unwrap_or("").to_owned(), false));
                    }
                }
                Some("select") => {
                    if name.is_empty() {
                        continue;
                    }
                    let options: Vec<NodeId> =
                        d.descendants(n).filter(|o| d.is(*o, "option")).collect();
                    let selected: Vec<NodeId> = if d.has_attr(n, "multiple") {
                        options
                            .iter()
                            .copied()
                            .filter(|o| {
                                d.has_attr(*o, "selected") && !semantics::is_disabled(d, *o)
                            })
                            .collect()
                    } else {
                        semantics::selected_option(d, n).into_iter().collect()
                    };
                    for o in selected {
                        entries.push((name.clone(), self.option_value(o), false));
                    }
                }
                Some("textarea") if !name.is_empty() => {
                    let value = self
                        .value_of(n, fields)
                        .replace("\r\n", "\n")
                        .replace('\r', "\n")
                        .replace('\n', "\r\n");
                    entries.push((name, value, false));
                }
                _ => {}
            }
        }
        Ok(encode_submission(url, &method, &enctype, &entries))
    }

    // -----------------------------------------------------------------------------
    // Hover, scroll, cursor
    // -----------------------------------------------------------------------------

    /// Moves the pointer to `(x, y)` scene px of the last rendered viewport: updates
    /// `:hover` and returns the CSS cursor name for that point.
    pub fn hover_at(&mut self, x: i32, y: i32, inputs: Inputs<'_>) -> &'static str {
        let hit = self.hit(x, y, inputs);
        if hit != self.hover {
            self.hover = hit;
            self.touch();
        }
        self.cursor_at(x, y, inputs)
    }

    pub fn hovered(&self) -> Option<NodeId> {
        self.hover
    }

    /// The element under `(x, y)` scene px, through the painting order.
    pub fn hit(&self, x: i32, y: i32, inputs: Inputs<'_>) -> Option<NodeId> {
        if self.script.is_some() {
            return self.script_hit(x, y, inputs);
        }
        let guard = self.render(inputs);
        let r = guard.0.render.as_ref()?;
        let zoom = inputs.zoom.max(1) as i64;
        let (cx, cy) = (
            (i64::from(x) * 100 / zoom) as i32,
            (i64::from(y) * 100 / zoom) as i32,
        );
        let none = paint::NoImages;
        let mut ctx = PaintContext::new(&none);
        ctx.scroll = r.scroll;
        ctx.scroll_offsets = r.scroll_offsets.clone();
        let viewport = Viewport {
            width: r.css_width,
            height: r.css_height,
            scale: 1,
            zoom: 100,
        };
        paint::hit::hit_test_with(&r.tree, &r.styles, viewport, &ctx, cx, cy)
    }

    /// The CSS cursor over `(x, y)`: the hit element's computed `cursor`, with `auto`
    /// resolved to `pointer` over links, `text` over text runs and text controls,
    /// and `default` elsewhere.
    pub fn cursor_at(&self, x: i32, y: i32, inputs: Inputs<'_>) -> &'static str {
        if self.script.is_some() {
            return self.script_cursor_at(x, y, inputs);
        }
        let Some(node) = self.hit(x, y, inputs) else {
            return "default";
        };
        let guard = self.render(inputs);
        let Some(r) = guard.0.render.as_ref() else {
            return "default";
        };
        let explicit = std::iter::once(node)
            .chain(self.doc.ancestors(node))
            .find_map(|n| {
                r.styles
                    .get(n)
                    .map(|s| s.cursor)
                    .filter(|c| *c != Cursor::Auto)
            });
        if let Some(c) = explicit {
            return cursor_name(c);
        }
        if semantics::is_disabled(&self.doc, node) && semantics::is_interactive(&self.doc, node) {
            return "default";
        }
        if std::iter::once(node)
            .chain(self.doc.ancestors(node))
            .any(|n| {
                (self.doc.is(n, "a") || self.doc.is(n, "area")) && self.doc.has_attr(n, "href")
            })
        {
            return "pointer";
        }
        if self.is_text_control(node) {
            return "text";
        }
        if matches!(
            self.doc.tag(node),
            Some("button" | "select" | "summary" | "label")
        ) || (self.doc.is(node, "input") && !self.is_text_control(node))
        {
            return "default";
        }
        // Over a text run the pointer is a beam.
        let zoom = inputs.zoom.max(1) as i64;
        let cx = Au::from_px_i32((i64::from(x) * 100 / zoom) as i32) + r.scroll.x;
        let cy = Au::from_px_i32((i64::from(y) * 100 / zoom) as i32) + r.scroll.y;
        let on_text = r
            .tree
            .hit(cx, cy)
            .last()
            .is_some_and(|(f, _)| matches!(f.kind, FragmentKind::Text { .. }));
        if on_text {
            "text"
        } else {
            "default"
        }
    }

    /// Scrolls an inner scroll container (by interaction id) to `offset` along one
    /// axis. Returns whether it moved.
    pub fn scroll_pane(&mut self, id: &str, offset: i32, horizontal: bool) -> bool {
        let entry = self.scrolls.entry(id.to_owned()).or_insert((0, 0));
        let slot = if horizontal {
            &mut entry.0
        } else {
            &mut entry.1
        };
        let offset = offset.max(0);
        if *slot == offset {
            return false;
        }
        *slot = offset;
        self.touch();
        true
    }

    pub fn inner_scrolls(&self) -> &BTreeMap<String, (i32, i32)> {
        &self.scrolls
    }

    /// The document's scrollable height in CSS px at the last render.
    pub fn content_height(&self) -> Option<u32> {
        if let Some(s) = &self.script {
            return Some(
                s.read(|realm| realm.fragment_tree().content_height.to_px_ceil().max(0) as u32),
            );
        }
        self.cache
            .lock()
            .ok()?
            .0
            .render
            .as_ref()
            .map(|r| r.tree.content_height.to_px_ceil().max(0) as u32)
    }

    // -----------------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------------

    /// The painted scene for `inputs`, in scene px (`width` x `height`).
    pub fn scene(&self, inputs: Inputs<'_>) -> Scene {
        if self.script.is_some() {
            return self.script_scene(inputs);
        }
        let guard = self.render(inputs);
        guard
            .0
            .render
            .as_ref()
            .map(|r| r.scene.clone())
            .unwrap_or_else(|| Scene::new(inputs.width, inputs.height))
    }

    fn render(&self, inputs: Inputs<'_>) -> MutexGuard<'_, SendCache> {
        let mut guard = match self.cache.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let key = RenderKey {
            generation: self.generation,
            width: inputs.width.max(1),
            height: inputs.height.max(1),
            zoom: inputs.zoom.clamp(10, 500),
            fields: inputs.fields.clone(),
            focused: inputs.focused.map(str::to_owned),
            scroll_y: inputs.scroll_y.max(0),
        };
        if guard.0.render.as_ref().is_some_and(|r| r.key == key) {
            return guard;
        }
        let cache = &mut guard.0;
        if cache.sheets.is_none() {
            cache.sheets = Some(self.parse_sheets());
        }
        if cache.decoded.is_none() {
            cache.decoded = Some(self.decode_images());
        }
        let zoom = i64::from(key.zoom);
        let css_width = ((i64::from(key.width) * 100 + zoom - 1) / zoom).max(1) as u32;
        let css_height = ((i64::from(key.height) * 100 + zoom - 1) / zoom).max(1) as u32;
        let media = Media::with_size(css_width as i32, css_height as i32);
        let focused_node = key
            .focused
            .as_deref()
            .and_then(|id| self.node_for(id))
            .filter(|n| semantics::is_focusable(&self.doc, *n) || self.doc.is(*n, "select"));
        let values = self.live_values(&key.fields);
        let form = LiveForm { values: &values };
        let mut ctx = MatchContext::new();
        ctx.set_hovered(&self.doc, self.hover);
        ctx.focused = focused_node;
        ctx.focus_visible = focused_node.is_some();
        ctx.target_id = self.target.clone();
        ctx.form = Some(&form);
        let sheets = cache.sheets.as_deref().unwrap_or(&[]);
        let previous = cache.render.take();
        let styles = match previous {
            Some(mut prev)
                if prev.key.generation == key.generation
                    && prev.css_width == css_width
                    && prev.css_height == css_height
                    && prev.target == self.target =>
            {
                // Only interaction state moved: restyle what flipped.
                let mut changed: Vec<NodeId> = prev
                    .hovered
                    .symmetric_difference(&ctx.hovered)
                    .copied()
                    .collect();
                for n in [prev.focused, focused_node].into_iter().flatten() {
                    if !changed.contains(&n) {
                        changed.push(n);
                    }
                }
                for (id, v) in &key.fields {
                    if prev.key.fields.get(id) != Some(v) {
                        if let Some(n) = self.node_for(id) {
                            if !changed.contains(&n) {
                                changed.push(n);
                            }
                        }
                    }
                }
                if !changed.is_empty() {
                    let _ = style::restyle_state(
                        &self.doc,
                        &mut prev.styles,
                        &changed,
                        sheets,
                        &media,
                        &ctx,
                        Strictness::Lenient,
                    );
                }
                prev.styles
            }
            _ => style::cascade(&self.doc, sheets, &media, &ctx, Strictness::Lenient)
                .unwrap_or_default(),
        };
        // Layout, with the scroll offsets (sticky positioning follows them).
        let mut scroll_state = ScrollState::new();
        let root_x = self.scrolls.get("page").map_or(0, |s| s.0);
        scroll_state.insert(
            Document::ROOT,
            (Au::from_px_i32(root_x), Au::from_px_i32(key.scroll_y)),
        );
        let mut scroll_offsets: BTreeMap<NodeId, Point> = BTreeMap::new();
        for (id, (sx, sy)) in &self.scrolls {
            if id == "page" {
                continue;
            }
            if let Some(n) = self.node_for(id) {
                let p = (Au::from_px_i32(*sx), Au::from_px_i32(*sy));
                scroll_state.insert(n, p);
                scroll_offsets.insert(n, Point { x: p.0, y: p.1 });
            }
        }
        let viewport = Viewport {
            width: key.width,
            height: key.height,
            scale: 1,
            zoom: key.zoom,
        };
        let sizes = ImageRefs {
            refs: &self.refs,
            images: &self.images,
        };
        let tree = layout::layout_with(
            &self.doc,
            &styles,
            viewport,
            LayoutOptions {
                images: &sizes,
                scroll: &scroll_state,
            },
            &mut cache.layout,
        );
        let max_y = (tree.content_height - tree.viewport_height).max(Au::ZERO);
        let max_x = (tree.content_width - tree.viewport_width).max(Au::ZERO);
        let scroll = Point {
            x: Au::from_px_i32(root_x).clamp(Au::ZERO, max_x),
            y: Au::from_px_i32(key.scroll_y).clamp(Au::ZERO, max_y),
        };
        let decoded = cache.decoded.as_ref().expect("decoded above");
        let pixels = DecodedRefs {
            refs: &self.refs,
            decoded,
        };
        let mut pctx = PaintContext::new(&pixels);
        pctx.scroll = scroll;
        pctx.scroll_offsets = scroll_offsets.clone();
        pctx.focused = focused_node;
        pctx.hovered = self.hover;
        pctx.caret = self.caret;
        pctx.values = values.clone();
        let mut scene = paint::paint(&self.doc, &styles, &tree, viewport, &pctx);
        // The document's own scroll is `pane:page`. The engine may also publish the root
        // box's scroll container under an empty id (`pane:`), with the same bounds and
        // extent; a wheel would be routed to that twin and the page would never move.
        scene.scrolls.retain(|area| area.target != "pane:");
        if key.zoom != 100 {
            zoom_scene(&mut scene, u32::from(key.zoom), key.width, key.height);
        }
        if let Some(notice) = &self.notice {
            paint_notice(&mut scene, notice);
        }
        cache.render = Some(Render {
            key,
            hovered: ctx.hovered.clone(),
            focused: focused_node,
            target: self.target.clone(),
            css_width,
            css_height,
            styles,
            tree,
            scroll,
            scroll_offsets,
            scene,
        });
        guard
    }

    fn parse_sheets(&self) -> Vec<Stylesheet> {
        self.sheets
            .iter()
            .map(|s| {
                let mut sheet =
                    css::parse_stylesheet(&s.source, Origin::Author, Strictness::Lenient)
                        .unwrap_or_default();
                if !s.media.is_empty() {
                    let query = MediaQueryList::parse(&s.media);
                    let rules = std::mem::take(&mut sheet.rules);
                    sheet.rules = vec![Rule::Media { query, rules }];
                }
                sheet
            })
            .collect()
    }

    fn decode_images(&self) -> BTreeMap<String, RgbaImage> {
        self.images
            .iter()
            .map(|(url, a)| {
                (
                    url.clone(),
                    RgbaImage {
                        width: a.width,
                        height: a.height,
                        rgba: a.rgba.clone(),
                    },
                )
            })
            .collect()
    }

    /// Typed values by node, for paint and `:placeholder-shown`.
    fn live_values(&self, fields: &BTreeMap<String, String>) -> BTreeMap<NodeId, String> {
        let mut out = BTreeMap::new();
        for (id, v) in fields {
            if let Some(n) = self.node_for(id) {
                if self.is_text_control(n) {
                    out.insert(n, v.clone());
                }
            }
        }
        out
    }

    // -----------------------------------------------------------------------------
    // Observations
    // -----------------------------------------------------------------------------

    /// The accessibility view of the painted page: one entry per element with a role
    /// (links, buttons, inputs, headings, landmarks) and per text run, with its
    /// interaction id (empty for text), label, value, state and bounds, in paint
    /// order.
    pub fn semantics(&self, inputs: Inputs<'_>) -> Vec<AxNode> {
        let scene = self.scene(inputs);
        let focused = scene.focus.as_ref().and_then(|f| f.interaction.clone());
        let mut out: Vec<AxNode> = Vec::new();
        let mut by_id: BTreeMap<String, usize> = BTreeMap::new();
        for (index, n) in scene.nodes.iter().enumerate() {
            let Some(sem) = &n.semantic else { continue };
            if let Some(id) = &n.interaction {
                if let Some(&i) = by_id.get(id) {
                    let ax = &mut out[i];
                    ax.bounds = union(ax.bounds, n.bounds);
                    ax.nodes.push(n.id);
                    if ax.value.is_none() {
                        ax.value = sem.value.clone();
                    }
                    continue;
                }
                by_id.insert(id.clone(), out.len());
            }
            let state = n.state.unwrap_or_default();
            out.push(AxNode {
                id: n.interaction.clone().unwrap_or_default(),
                role: sem.role.clone(),
                name: sem.label.clone(),
                value: sem.value.clone(),
                enabled: !sem.disabled,
                focusable: sem.focusable,
                focused: n.interaction.is_some() && n.interaction == focused,
                checked: state.checked,
                selected: state.selected,
                expanded: state.expanded,
                window: None,
                bounds: n.bounds,
                nodes: vec![n.id],
                z: n.z,
                order: index,
                hit: None,
                ..AxNode::default()
            });
        }
        out
    }

    /// The document as a `Page`, the shape the semantic observation and the page-text
    /// readers take: headings, text runs, links (resolved), forms with their inputs
    /// and buttons, and pictures with a size.
    pub fn to_page(&self, fields: &BTreeMap<String, String>) -> Page {
        if self.script.is_some() {
            return self.projection().to_page(fields);
        }
        let mut page = Page::new(if self.title.is_empty() {
            self.url.as_str()
        } else {
            self.title.as_str()
        });
        page.lang = self
            .doc
            .document_element()
            .and_then(|h| self.doc.attr(h, "lang"))
            .map(str::to_owned)
            .filter(|l| !l.is_empty());
        let mut p = Projector {
            web: self,
            fields,
            tables: semantics::Tables::build(&self.doc),
            ids: BTreeSet::new(),
            texts: 0,
            out: Vec::new(),
            run: String::new(),
        };
        if let Some(body) = self.doc.body() {
            p.walk_children(body);
        }
        p.flush();
        page.elements = p.out;
        page
    }

    /// The visible text of the page, one block per line.
    pub fn text(&self) -> String {
        let empty = BTreeMap::new();
        let page = self.to_page(&empty);
        fn walk(elements: &[PageElement], out: &mut Vec<String>) {
            for e in elements {
                match e {
                    PageElement::Heading { text, .. }
                    | PageElement::Text { text, .. }
                    | PageElement::Link { text, .. }
                    | PageElement::Button { text, .. } => out.push(text.clone()),
                    PageElement::Input { label, value, .. } => out.push(if value.is_empty() {
                        label.clone()
                    } else {
                        format!("{label}: {value}")
                    }),
                    PageElement::Form { children, .. } => walk(children, out),
                    PageElement::Image { alt, .. } => out.push(alt.clone()),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&page.elements, &mut out);
        out.retain(|s| !s.trim().is_empty());
        out.join("\n")
    }
}

/// The request that carries a form data set (`(name, value, is a file)` entries) to
/// `url`: a query string for GET, else a body in the form's encoding (HTML §4.10.21).
pub(crate) fn encode_submission(
    mut url: Url,
    method: &str,
    enctype: &str,
    entries: &[(String, String, bool)],
) -> HttpRequest {
    let mut request = HttpRequest::get(url.as_str());
    if method == "GET" || method == "DIALOG" {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(entries.iter().map(|(k, v, _)| (k.as_str(), v.as_str())))
            .finish();
        url.set_query(if entries.is_empty() {
            None
        } else {
            Some(&query)
        });
        request.url = url.to_string();
        return request;
    }
    request.method = "POST".into();
    match enctype {
        "multipart/form-data" => {
            let boundary = format!("----computerworld{:016x}", cw_scene::digest(&entries));
            let mut body = Vec::new();
            for (name, value, file) in entries {
                body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
                if *file {
                    body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{}\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n", escape_disposition(name)).as_bytes());
                } else {
                    body.extend_from_slice(
                        format!(
                            "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                            escape_disposition(name)
                        )
                        .as_bytes(),
                    );
                    body.extend_from_slice(value.as_bytes());
                }
                body.extend_from_slice(b"\r\n");
            }
            body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
            request.headers.insert(
                "content-type".into(),
                format!("multipart/form-data; boundary={boundary}"),
            );
            request.body = body;
        }
        "text/plain" => {
            let mut body = String::new();
            for (name, value, _) in entries {
                body.push_str(name);
                body.push('=');
                body.push_str(value);
                body.push_str("\r\n");
            }
            request
                .headers
                .insert("content-type".into(), "text/plain".into());
            request.body = body.into_bytes();
        }
        _ => {
            request.headers.insert(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            );
            request.body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(entries.iter().map(|(k, v, _)| (k.as_str(), v.as_str())))
                .finish()
                .into_bytes();
        }
    }
    request
}

/// One entry of the stylesheet plan: what to fetch, or what is already in the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SheetPlan {
    Inline { media: String, source: String },
    Linked { url: String, media: String },
}

fn union(a: cw_scene::Rect, b: cw_scene::Rect) -> cw_scene::Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width as i32).max(b.x + b.width as i32);
    let y1 = (a.y + a.height as i32).max(b.y + b.height as i32);
    cw_scene::Rect::new(x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32)
}

/// Projects the DOM into page elements: block boundaries become text runs, and the
/// elements the page reader knows about (headings, links, controls, forms, pictures)
/// keep their own entries and interaction ids.
struct Projector<'a> {
    web: &'a WebDocument,
    fields: &'a BTreeMap<String, String>,
    tables: semantics::Tables,
    ids: BTreeSet<String>,
    texts: u32,
    out: Vec<PageElement>,
    run: String,
}

impl Projector<'_> {
    fn unique(&mut self, id: String) -> String {
        if self.ids.insert(id.clone()) {
            return id;
        }
        let mut n = 2;
        loop {
            let candidate = format!("{id}#{n}");
            if self.ids.insert(candidate.clone()) {
                return candidate;
            }
            n += 1;
        }
    }
    /// The element's own `id` attribute, when it has a usable one.
    fn own_id(&self, node: NodeId) -> Option<String> {
        self.web
            .doc
            .attr(node, "id")
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    }
    fn text_id(&mut self) -> String {
        self.texts += 1;
        let id = format!("text:{}", self.texts);
        self.unique(id)
    }
    fn flush(&mut self) {
        self.flush_as(None);
    }
    /// Emits the run collected so far. `id` is the id of the element the text came
    /// straight out of, when it has one: an author who writes `<td id=sheet-A2>` or
    /// `<span id=title>` means that text to be addressable by that id, so it is not
    /// given a `text:N` of its own.
    fn flush_as(&mut self, id: Option<String>) {
        let text = semantics::collapse(&std::mem::take(&mut self.run));
        if !text.is_empty() {
            let id = match id {
                Some(id) => self.unique(id),
                None => self.text_id(),
            };
            self.out.push(PageElement::Text { id, text });
        }
    }
    fn walk_children(&mut self, node: NodeId) {
        let web = self.web;
        let children: Vec<NodeId> = web.doc.children(node).collect();
        for c in children {
            self.walk(c);
        }
    }
    fn walk(&mut self, node: NodeId) {
        let web = self.web;
        let d = &web.doc;
        match d.kind(node) {
            NodeKind::Text(t) => self.run.push_str(t),
            NodeKind::Element { tag, .. } => {
                let tag = tag.clone();
                if d.has_attr(node, "hidden") {
                    return;
                }
                match tag.as_str() {
                    "script" | "style" | "template" | "noscript" | "head" | "title" | "meta"
                    | "link" | "svg" | "math" => {}
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        self.flush();
                        let level = tag[1..].parse().unwrap_or(1);
                        let text = semantics::collapse(&d.text_content(node));
                        let id = self.unique(self.web.id_of(node));
                        self.out.push(PageElement::Heading { id, text, level });
                    }
                    "a" if d.has_attr(node, "href") => {
                        self.flush();
                        let mut text = semantics::collapse(&d.text_content(node));
                        if text.is_empty() {
                            text = d
                                .descendants(node)
                                .find_map(|n| d.attr(n, "alt").map(semantics::collapse))
                                .unwrap_or_default();
                        }
                        let url = self
                            .web
                            .resolve(d.attr(node, "href").unwrap_or(""))
                            .map(|u| u.to_string())
                            .unwrap_or_else(|| d.attr(node, "href").unwrap_or("").to_owned());
                        let id = self.unique(self.web.id_of(node));
                        self.out.push(PageElement::Link {
                            id,
                            text,
                            url,
                            style: None,
                        });
                    }
                    "button" => {
                        self.flush();
                        let id = self.unique(self.web.id_of(node));
                        let text = semantics::label_of(d, &self.tables, node);
                        self.out.push(PageElement::Button {
                            id,
                            text,
                            action: self.form_action(node),
                            style: None,
                        });
                    }
                    "input" => {
                        self.flush();
                        let kind = input_type(d, node);
                        let id = self.unique(self.web.id_of(node));
                        let label = semantics::label_of(d, &self.tables, node);
                        match kind.as_str() {
                            "hidden" => {}
                            "submit" | "button" | "reset" | "image" => {
                                self.out.push(PageElement::Button {
                                    id,
                                    text: label,
                                    action: self.form_action(node),
                                    style: None,
                                });
                            }
                            "checkbox" | "radio" => {
                                let mark = if self.web.is_checked(node) {
                                    if kind == "radio" {
                                        "(o)"
                                    } else {
                                        "[x]"
                                    }
                                } else if kind == "radio" {
                                    "( )"
                                } else {
                                    "[ ]"
                                };
                                self.out.push(PageElement::Text {
                                    id,
                                    text: format!("{mark} {label}").trim().to_owned(),
                                });
                            }
                            _ => {
                                let value = self.web.value_of(node, self.fields);
                                let placeholder =
                                    d.attr(node, "placeholder").unwrap_or("").to_owned();
                                self.out.push(PageElement::Input {
                                    id,
                                    label,
                                    value,
                                    placeholder,
                                });
                            }
                        }
                    }
                    "textarea" => {
                        self.flush();
                        let id = self.unique(self.web.id_of(node));
                        let label = semantics::label_of(d, &self.tables, node);
                        let value = self.web.value_of(node, self.fields);
                        let placeholder = d.attr(node, "placeholder").unwrap_or("").to_owned();
                        self.out.push(PageElement::Input {
                            id,
                            label,
                            value,
                            placeholder,
                        });
                    }
                    "select" => {
                        self.flush();
                        let id = self.unique(self.web.id_of(node));
                        let label = semantics::label_of(d, &self.tables, node);
                        let value = semantics::selected_option(d, node)
                            .map(|o| semantics::collapse(&d.text_content(o)))
                            .unwrap_or_default();
                        self.out.push(PageElement::Input {
                            id,
                            label,
                            value,
                            placeholder: String::new(),
                        });
                    }
                    "form" => {
                        self.flush();
                        let id = self.unique(self.web.id_of(node));
                        let action = self.form_action(node);
                        let outer = std::mem::take(&mut self.out);
                        self.walk_children(node);
                        self.flush();
                        let children = std::mem::replace(&mut self.out, outer);
                        self.out.push(PageElement::Form {
                            id,
                            action,
                            children,
                        });
                    }
                    "img" => {
                        let alt = semantics::collapse(d.attr(node, "alt").unwrap_or(""));
                        let size = |a: &str| {
                            d.attr(node, a)
                                .and_then(|v| v.trim().parse::<u32>().ok())
                                .filter(|v| *v > 0)
                        };
                        let source = d.attr(node, "src").unwrap_or("").to_owned();
                        let resolved = self.web.resolve(&source).map(|u| u.to_string());
                        let known = resolved
                            .as_ref()
                            .and_then(|u| self.web.images.get(u))
                            .map(|a| (a.width, a.height));
                        match size("width").zip(size("height")).or(known) {
                            Some((width, height)) if !source.is_empty() => {
                                self.flush();
                                let id = self.unique(self.web.id_of(node));
                                self.out.push(PageElement::Image {
                                    id,
                                    source: resolved.unwrap_or(source),
                                    alt,
                                    width: width.min(8192),
                                    height: height.min(8192),
                                    style: None,
                                    action: None,
                                });
                            }
                            _ => {
                                if !alt.is_empty() {
                                    self.run.push(' ');
                                    self.run.push_str(&alt);
                                    self.run.push(' ');
                                }
                            }
                        }
                    }
                    "br" => self.run.push('\n'),
                    "li" => {
                        self.flush();
                        self.run.push_str("\u{2022} ");
                        self.walk_children(node);
                        self.flush();
                    }
                    "p" | "div" | "section" | "article" | "header" | "footer" | "nav" | "main"
                    | "aside" | "ul" | "ol" | "menu" | "table" | "thead" | "tbody" | "tfoot"
                    | "tr" | "td" | "th" | "caption" | "pre" | "blockquote" | "dl" | "dt"
                    | "dd" | "figure" | "figcaption" | "details" | "summary" | "fieldset"
                    | "legend" | "address" | "hr" | "center" | "body" | "html" | "label"
                    | "option" | "optgroup" | "dialog" => {
                        let own = self.own_id(node);
                        self.flush();
                        self.walk_children(node);
                        self.flush_as(own);
                    }
                    _ => {
                        if d.text_content(node).is_empty()
                            && !d.descendants(node).any(|n| d.is(n, "img"))
                        {
                            return;
                        }
                        match self.own_id(node) {
                            // Inline chrome with an id is broken out of the run it
                            // sits in so the id survives into the projection.
                            Some(id) => {
                                self.flush();
                                self.walk_children(node);
                                self.flush_as(Some(id));
                            }
                            None => self.walk_children(node),
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn form_action(&self, node: NodeId) -> PageAction {
        let d = &self.web.doc;
        let form = if d.is(node, "form") {
            Some(node)
        } else {
            self.web.form_owner(node)
        };
        let attr = |name: &str| -> Option<String> {
            let own = if d.is(node, "form") {
                None
            } else {
                d.attr(node, &format!("form{name}"))
            };
            own.or_else(|| form.and_then(|f| d.attr(f, name)))
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let method = attr("method")
            .map(|m| m.to_ascii_uppercase())
            .filter(|m| m == "POST")
            .unwrap_or_else(|| "GET".into());
        let url = attr("action")
            .and_then(|a| self.web.resolve(&a))
            .map(|u| u.to_string())
            .unwrap_or_else(|| {
                let mut u = self.web.url.clone();
                if let Some(i) = u.find('#') {
                    u.truncate(i);
                }
                u
            });
        PageAction {
            method,
            url,
            fields: BTreeMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------------
// Engine adapters
// ---------------------------------------------------------------------------------

/// Live form state for the cascade: typed values over the attributes.
struct LiveForm<'a> {
    values: &'a BTreeMap<NodeId, String>,
}

impl FormState for LiveForm<'_> {
    fn checked(&self, doc: &Document, element: NodeId) -> bool {
        css::AttributeFormState.checked(doc, element)
    }
    fn indeterminate(&self, _doc: &Document, _element: NodeId) -> bool {
        false
    }
    fn value(&self, doc: &Document, element: NodeId) -> String {
        match self.values.get(&element) {
            Some(v) => v.clone(),
            None => css::AttributeFormState.value(doc, element),
        }
    }
}

/// Picture sizes for layout, by the reference the page used.
struct ImageRefs<'a> {
    refs: &'a BTreeMap<String, String>,
    images: &'a BTreeMap<String, Arc<ImageAsset>>,
}

impl ImageSizes for ImageRefs<'_> {
    fn size(&self, src: &str) -> Option<(u32, u32)> {
        let url = self.refs.get(src)?;
        self.images.get(url).map(|a| (a.width, a.height))
    }
}

/// Pixels for paint, by the reference the page used.
struct DecodedRefs<'a> {
    refs: &'a BTreeMap<String, String>,
    decoded: &'a BTreeMap<String, RgbaImage>,
}

impl ImageCache for DecodedRefs<'_> {
    fn image(&self, url: &str) -> Option<&RgbaImage> {
        let resolved = self.refs.get(url)?;
        self.decoded.get(resolved)
    }
}

pub(crate) fn input_type(doc: &Document, node: NodeId) -> String {
    doc.attr(node, "type")
        .unwrap_or("text")
        .trim()
        .to_ascii_lowercase()
}

fn is_text_input_type(t: &str) -> bool {
    !matches!(
        t,
        "checkbox"
            | "radio"
            | "submit"
            | "button"
            | "reset"
            | "hidden"
            | "image"
            | "file"
            | "color"
            | "range"
    )
}

fn escape_disposition(name: &str) -> String {
    name.replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace('"', "%22")
}

fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("k={}", s.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| s.to_owned())
}

/// The CSS name of a computed cursor.
pub fn cursor_name(c: Cursor) -> &'static str {
    match c {
        Cursor::Auto | Cursor::Default => "default",
        Cursor::Pointer => "pointer",
        Cursor::Text => "text",
        Cursor::Move => "move",
        Cursor::NotAllowed => "not-allowed",
        Cursor::Grab => "grab",
        Cursor::Grabbing => "grabbing",
        Cursor::Crosshair => "crosshair",
        Cursor::Wait => "wait",
        Cursor::Progress => "progress",
        Cursor::Help => "help",
        Cursor::ColResize => "col-resize",
        Cursor::RowResize => "row-resize",
        Cursor::NsResize => "ns-resize",
        Cursor::EwResize => "ew-resize",
        Cursor::NeswResize => "nesw-resize",
        Cursor::NwseResize => "nwse-resize",
        Cursor::None => "none",
    }
}

/// Every `url()` among component values (`url(x)`, `url("x")`, `src("x")`).
fn css_urls(values: &[ComponentValue]) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(values: &[ComponentValue], out: &mut Vec<String>) {
        for v in values {
            match v {
                ComponentValue::Token(Token::Url(u)) => out.push(u.clone()),
                ComponentValue::Function { name, args } => {
                    if name.eq_ignore_ascii_case("url") || name.eq_ignore_ascii_case("src") {
                        if let Some(ComponentValue::Token(Token::String(s))) =
                            args.iter().find(|a| !a.is_whitespace())
                        {
                            out.push(s.clone());
                        }
                    } else {
                        walk(args, out);
                    }
                }
                ComponentValue::Block { contents, .. } => walk(contents, out),
                _ => {}
            }
        }
    }
    walk(values, &mut out);
    out.retain(|u| !u.trim().is_empty() && !u.trim_start().starts_with("data:"));
    out
}

/// Every `url()` in a sheet's style rules (`@font-face` sources excluded: faces come
/// from the bundle).
fn sheet_urls(rules: &[Rule]) -> Vec<String> {
    let mut out = Vec::new();
    for r in rules {
        match r {
            Rule::Style { declarations, .. } => {
                for d in declarations {
                    out.extend(css_urls(&d.value));
                }
            }
            Rule::Media { rules, .. }
            | Rule::Supports { rules, .. }
            | Rule::Layer { rules, .. } => out.extend(sheet_urls(rules)),
            _ => {}
        }
    }
    out
}

/// Draws a scene laid out in CSS px at `percent` zoom into a `width` x `height`
/// viewport. Scroll areas keep their CSS-px offsets and extents (the units the scroll
/// actions take) and scale their bounds with the content.
fn zoom_scene(scene: &mut Scene, percent: u32, width: u32, height: u32) {
    let areas = scene.scrolls.clone();
    crate::page_scene::scale(scene, percent, width, height);
    let p = |v: i32| (i64::from(v) * i64::from(percent) / 100) as i32;
    let q = |v: u32| (u64::from(v) * u64::from(percent)).div_ceil(100) as u32;
    for (area, original) in scene.scrolls.iter_mut().zip(areas) {
        area.bounds = if original.target == "pane:page" {
            cw_scene::Rect::new(0, 0, width, height)
        } else {
            cw_scene::Rect::new(
                p(original.bounds.x),
                p(original.bounds.y),
                q(original.bounds.width),
                q(original.bounds.height),
            )
        };
    }
}

/// A one-line message strip at the bottom of the page (a blocked submission).
fn paint_notice(scene: &mut Scene, text: &str) {
    use cw_scene::{Color, Node, Primitive, Rect};
    let h = 28u32.min(scene.height);
    let y = scene.height.saturating_sub(h) as i32;
    let base = 1u64 << 62;
    let mut bar = Node::new(
        base,
        Rect::new(0, y, scene.width, h),
        Primitive::Box {
            fill: Color::rgb(255, 244, 229),
            border: Some(Color::rgb(230, 190, 120)),
            border_width: 1,
        },
    );
    bar.z = i32::MAX - 2;
    scene.nodes.push(bar);
    let mut label = Node::new(
        base + 1,
        Rect::new(
            10,
            y + 6,
            scene.width.saturating_sub(20),
            h.saturating_sub(12),
        ),
        Primitive::UiText {
            text: text.to_owned(),
            size: 13,
            color: Color::rgb(90, 60, 10),
            italic: false,
            lang: Default::default(),
            typeface: None,
            web: false,
        },
    );
    label.z = i32::MAX - 1;
    label.semantic = Some(cw_scene::Semantic {
        role: "status".into(),
        label: text.to_owned(),
        value: None,
        disabled: false,
        focusable: false,
    });
    scene.nodes.push(label);
}
