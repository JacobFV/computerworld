//! The scripted half of `WebDocument`: everything that reads or drives the realm.
//!
//! The realm owns the document, so paint, hit testing, the cursor and the page
//! projection all read `Realm::document()`, `styles()` and `fragment_tree()`. The
//! only derived copies are caches keyed by the realm's epoch: the painted scene and
//! the projection the page readers take (the DOM with live checkedness written back
//! as attributes and `display: none` subtrees marked hidden, so `to_page` lists what
//! the person can see).

use cw_web::script::{Realm, UiEvent};
use cw_web::style::computed::Display;

use super::*;
use crate::scripted::HostEnv;

pub(super) struct ScriptRender {
    epoch: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) zoom: u16,
    scene: Scene,
}

/// Pixels for a scripted page: fetched pictures by reference, canvases by node.
struct ScriptImages<'a> {
    refs: &'a BTreeMap<String, String>,
    decoded: &'a BTreeMap<String, RgbaImage>,
    canvases: &'a BTreeMap<String, RgbaImage>,
}

impl ImageCache for ScriptImages<'_> {
    fn image(&self, url: &str) -> Option<&RgbaImage> {
        if url.starts_with("canvas:") {
            return self.canvases.get(url);
        }
        self.decoded.get(self.refs.get(url)?)
    }
}

fn css_size(inputs: Inputs<'_>) -> (u32, u32, u16) {
    let zoom = inputs.zoom.clamp(10, 500);
    let z = i64::from(zoom);
    let w = ((i64::from(inputs.width.max(1)) * 100 + z - 1) / z).max(1) as u32;
    let h = ((i64::from(inputs.height.max(1)) * 100 + z - 1) / z).max(1) as u32;
    (w, h, zoom)
}

/// Whether a parsed document has anything to run: a `<script>` element or an inline
/// `on*` handler.
fn document_has_script(doc: &Document) -> bool {
    doc.descendants(Document::ROOT).any(|n| match doc.kind(n) {
        NodeKind::Element { tag, attrs, .. } => tag == "script" || attrs.iter().any(|a| a.name.len() > 2 && a.name.starts_with("on")),
        _ => false,
    })
}

impl WebDocument {
    /// Whether the parsed page has script: such a page is shown through a realm.
    pub fn has_script(&self) -> bool {
        document_has_script(&self.doc)
    }

    pub fn is_scripted(&self) -> bool {
        self.script.is_some()
    }

    /// The realm handle of a scripted document.
    pub fn scripted(&self) -> Option<&Scripted> {
        self.script.as_ref()
    }

    /// A document shown through a realm. Nothing has run: the caller enters it with
    /// `script_enter(.., |realm| realm.run_document())` once the host is ready.
    pub fn new_scripted(html: &str, url: &str, css_viewport: (u32, u32), now: u64) -> WebDocument {
        let viewport = Viewport { width: css_viewport.0.max(1), height: css_viewport.1.max(1), scale: 1, zoom: 100 };
        let mut web = WebDocument::parse("", url);
        web.script = Some(Scripted::new(html, url, viewport, now));
        web
    }

    /// Reads the document, wherever it lives.
    pub fn with_document<T>(&self, f: impl FnOnce(&Document) -> T) -> T {
        match &self.script {
            Some(s) => s.read(|realm| f(&realm.document())),
            None => f(&self.doc),
        }
    }

    /// Enters the realm for one browser event: `f`, then the event loop until idle,
    /// then layout and the layout observers, so the next paint needs nothing but
    /// reading. `None` for a document without script.
    pub(crate) fn script_enter<T>(&mut self, env: &mut HostEnv, f: impl FnOnce(&mut Realm) -> T) -> Option<T> {
        let s = self.script.as_ref()?;
        let (out, title, url) = s.enter(env, |realm| {
            let out = f(realm);
            realm.run_until_idle(crate::scripted::SETTLE_MS);
            realm.after_layout();
            realm.run_until_idle(0);
            let _ = realm.layout();
            (out, realm.title(), realm.url())
        });
        self.title = semantics::collapse(&title);
        self.target = url.split_once('#').map(|(_, f)| f.to_owned()).filter(|f| !f.is_empty());
        self.url = url;
        self.base = self.with_document(|d| {
            let head = d.head()?;
            let href = d.descendants(head).find(|n| d.is(*n, "base")).and_then(|n| d.attr(n, "href").map(str::to_owned))?;
            let u = Url::parse(&self.url).ok()?.join(href.trim()).ok()?;
            matches!(u.scheme(), "http" | "https").then(|| u.to_string())
        })
        .unwrap_or_else(|| self.url.clone());
        self.generation += 1;
        Some(out)
    }

    /// `<meta http-equiv=refresh>` of a scripted document.
    pub(crate) fn script_meta_refresh(&self) -> Option<String> {
        self.with_document(|d| {
            let head = d.head()?;
            d.descendants(head).find_map(|n| {
                if !d.is(n, "meta") || !d.attr(n, "http-equiv")?.trim().eq_ignore_ascii_case("refresh") {
                    return None;
                }
                d.attr(n, "content").map(|c| c.replace(',', ";"))
            })
        })
    }

    /// Pictures the realm's document and sheets reference that were not fetched yet,
    /// as `(reference, resolved URL)`.
    pub(crate) fn script_image_references(&self) -> Vec<(String, String)> {
        let Some(s) = &self.script else { return Vec::new() };
        let mut out: Vec<(String, String)> = Vec::new();
        let known = self.refs.len();
        s.read(|realm| {
            let inner = realm.layout();
            let d = &inner.doc;
            let push = |reference: String, base: Option<&Url>, out: &mut Vec<(String, String)>| {
                if self.refs.contains_key(&reference) || out.iter().any(|(r, _)| *r == reference) || known + out.len() >= MAX_IMAGES {
                    return;
                }
                let resolved = match base {
                    Some(b) => b.join(reference.trim()).ok(),
                    None => self.resolve(&reference),
                };
                if let Some(mut u) = resolved.filter(|u| matches!(u.scheme(), "http" | "https") && u.username().is_empty() && u.password().is_none()) {
                    u.set_fragment(None);
                    out.push((reference, u.to_string()));
                }
            };
            for n in d.descendants(Document::ROOT) {
                let is_img = d.is(n, "img") || (d.is(n, "input") && input_type(d, n) == "image");
                if is_img {
                    if let Some(src) = d.attr(n, "src").filter(|s| !s.trim().is_empty() && !s.starts_with("data:")) {
                        push(src.to_owned(), None, &mut out);
                    }
                }
                if let Some(style) = d.attr(n, "style").filter(|s| s.contains("url(")) {
                    for u in css_urls(&css::parse_declarations(style).iter().flat_map(|d| d.value.iter()).cloned().collect::<Vec<_>>()) {
                        push(u, None, &mut out);
                    }
                }
            }
            for sheet in &inner.sheets {
                let base = sheet.href.as_deref().and_then(|h| Url::parse(h).ok());
                for u in sheet_urls(&sheet.sheet.rules) {
                    push(u, base.as_ref(), &mut out);
                }
            }
        });
        out
    }

    /// Tells the realm's layout the sizes of the pictures fetched since it last
    /// heard (`references` are the ones just added).
    pub(crate) fn script_set_image_sizes(&mut self, env: &mut HostEnv, references: &[String]) {
        let sizes: Vec<(String, u32, u32)> = references.iter().filter_map(|r| self.images.get(self.refs.get(r)?).map(|a| (r.clone(), a.width, a.height))).collect();
        if sizes.is_empty() {
            return;
        }
        self.script_enter(env, |realm| realm.set_image_sizes(sizes));
    }

    /// The realm's text controls and their live values, by interaction id: what the
    /// tab's `fields` map mirrors.
    pub(crate) fn script_fields(&self) -> BTreeMap<String, String> {
        let Some(s) = &self.script else { return BTreeMap::new() };
        s.read(|realm| {
            let inner = realm.layout();
            let d = &inner.doc;
            d.descendants(Document::ROOT).filter(|n| inner.is_text_control(*n)).map(|n| (semantics::interaction_id(d, n), inner.control_value(n))).collect()
        })
    }

    /// `(focused element's interaction id, document scroll y in CSS px)`.
    pub(crate) fn script_focus_and_scroll(&self) -> (Option<String>, i32) {
        let Some(s) = &self.script else { return (None, 0) };
        s.read(|realm| {
            let inner = realm.layout();
            let focused = inner.focused.map(|n| semantics::interaction_id(&inner.doc, n));
            (focused, inner.window_scroll().1.to_px_floor().max(0))
        })
    }

    /// Brings the realm's viewport to the one being asked for. The environment sets
    /// the viewport before it acts, so this only fires for a caller that paints at a
    /// size it never announced: the page then sees a `resize` with no network.
    fn sync_viewport(&self, s: &Scripted, inputs: Inputs<'_>) {
        let (w, h, _) = css_size(inputs);
        let current = s.read(|realm| {
            let inner = realm.layout();
            (inner.viewport.width, inner.viewport.height)
        });
        if current == (w, h) {
            return;
        }
        let mut env = HostEnv { url: self.url.clone(), viewport: Viewport { width: w, height: h, scale: 1, zoom: 100 }, ..HostEnv::default() };
        s.enter(&mut env, |realm| {
            realm.dispatch(UiEvent::Resize { width: w, height: h });
            realm.run_until_idle(0);
            realm.after_layout();
            let _ = realm.layout();
        });
    }

    pub(super) fn script_scene(&self, inputs: Inputs<'_>) -> Scene {
        let Some(s) = &self.script else { return Scene::new(inputs.width, inputs.height) };
        self.sync_viewport(s, inputs);
        let (css_w, css_h, zoom) = css_size(inputs);
        let (width, height) = (inputs.width.max(1), inputs.height.max(1));
        let mut guard = match self.cache.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let epoch = s.epoch();
        if let Some(r) = &guard.0.script_render {
            if (r.epoch, r.width, r.height, r.zoom) == (epoch, width, height, zoom) {
                return r.scene.clone();
            }
        }
        if guard.0.decoded.is_none() {
            guard.0.decoded = Some(self.decode_images());
        }
        let decoded = guard.0.decoded.as_ref().expect("decoded above");
        let mut scene = s.read(|realm| {
            let canvases: BTreeMap<String, RgbaImage> = realm.canvases().into_iter().map(|(n, image)| (format!("canvas:{}", n.0), image)).collect();
            let inner = realm.layout();
            let Some(tree) = inner.tree.as_ref() else { return Scene::new(width, height) };
            let images = ScriptImages { refs: &self.refs, decoded, canvases: &canvases };
            let mut pctx = PaintContext::new(&images);
            let (sx, sy) = inner.window_scroll();
            let max_y = (tree.content_height - tree.viewport_height).max(Au::ZERO);
            let max_x = (tree.content_width - tree.viewport_width).max(Au::ZERO);
            pctx.scroll = Point { x: sx.clamp(Au::ZERO, max_x), y: sy.clamp(Au::ZERO, max_y) };
            pctx.scroll_offsets = inner.scroll.iter().filter(|(n, _)| **n != Document::ROOT).map(|(n, (x, y))| (*n, Point { x: *x, y: *y })).collect();
            pctx.focused = inner.focused;
            pctx.hovered = inner.hovered;
            pctx.caret = inner.focused.and_then(|f| {
                let (_, end) = *inner.form.selection.get(&f)?;
                (end < inner.control_value(f).chars().count()).then_some(end)
            });
            pctx.values = inner.form.values.clone();
            let viewport = Viewport { width: css_w, height: css_h, scale: 1, zoom: 100 };
            let viewport = if zoom == 100 { viewport } else { Viewport { width, height, scale: 1, zoom } };
            paint::paint(&inner.doc, &inner.styles, tree, viewport, &pctx)
        });
        scene.scrolls.retain(|area| area.target != "pane:");
        if zoom != 100 {
            zoom_scene(&mut scene, u32::from(zoom), width, height);
        }
        if let Some(notice) = &self.notice {
            paint_notice(&mut scene, notice);
        }
        guard.0.script_render = Some(ScriptRender { epoch, width, height, zoom, scene: scene.clone() });
        scene
    }

    /// `(x, y)` scene px as CSS px of the realm's viewport.
    pub(crate) fn css_point(x: i32, y: i32, zoom: u16) -> (i32, i32) {
        let z = i64::from(zoom.max(1));
        ((i64::from(x) * 100 / z) as i32, (i64::from(y) * 100 / z) as i32)
    }

    pub(super) fn script_hit(&self, x: i32, y: i32, inputs: Inputs<'_>) -> Option<NodeId> {
        let s = self.script.as_ref()?;
        self.sync_viewport(s, inputs);
        let (cx, cy) = Self::css_point(x, y, inputs.zoom);
        s.read(|realm| {
            let inner = realm.layout();
            let tree = inner.tree.as_ref()?;
            let none = paint::NoImages;
            let mut ctx = PaintContext::new(&none);
            let (sx, sy) = inner.window_scroll();
            ctx.scroll = Point { x: sx, y: sy };
            ctx.scroll_offsets = inner.scroll.iter().filter(|(n, _)| **n != Document::ROOT).map(|(n, (x, y))| (*n, Point { x: *x, y: *y })).collect();
            paint::hit::hit_test_with(tree, &inner.styles, inner.viewport, &ctx, cx, cy)
        })
    }

    pub(super) fn script_cursor_at(&self, x: i32, y: i32, inputs: Inputs<'_>) -> &'static str {
        let Some(node) = self.script_hit(x, y, inputs) else { return "default" };
        let Some(s) = &self.script else { return "default" };
        let (cx, cy) = Self::css_point(x, y, inputs.zoom);
        s.read(|realm| {
            let inner = realm.layout();
            let d = &inner.doc;
            let explicit = std::iter::once(node).chain(d.ancestors(node)).find_map(|n| inner.styles.get(n).map(|s| s.cursor).filter(|c| *c != Cursor::Auto));
            if let Some(c) = explicit {
                return cursor_name(c);
            }
            if semantics::is_disabled(d, node) && semantics::is_interactive(d, node) {
                return "default";
            }
            if std::iter::once(node).chain(d.ancestors(node)).any(|n| (d.is(n, "a") || d.is(n, "area")) && d.has_attr(n, "href")) {
                return "pointer";
            }
            if inner.is_text_control(node) {
                return "text";
            }
            if matches!(d.tag(node), Some("button" | "select" | "summary" | "label" | "input")) {
                return "default";
            }
            let (sx, sy) = inner.window_scroll();
            let on_text = inner.tree.as_ref().is_some_and(|t| t.hit(Au::from_px_i32(cx) + sx, Au::from_px_i32(cy) + sy).last().is_some_and(|(f, _)| matches!(f.kind, FragmentKind::Text { .. })));
            if on_text {
                "text"
            } else {
                "default"
            }
        })
    }

    /// The scripted document as a plain one, for the page readers. Cached per epoch.
    pub(super) fn projection(&self) -> Arc<WebDocument> {
        let s = self.script.as_ref().expect("a scripted document");
        let epoch = s.epoch();
        let mut guard = match self.cache.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some((e, p)) = &guard.0.projection {
            if *e == epoch {
                return p.clone();
            }
        }
        let doc = s.read(|realm| {
            let inner = realm.layout();
            let mut d = inner.doc.clone();
            for (n, on) in &inner.form.checked {
                let name = if d.is(*n, "option") { "selected" } else { "checked" };
                if *on && !d.has_attr(*n, name) {
                    d.set_attr(*n, name, "");
                } else if !*on && d.has_attr(*n, name) {
                    d.remove_attr(*n, name);
                }
            }
            let hidden: Vec<NodeId> = d.descendants(Document::ROOT).filter(|n| d.is_element(*n) && !d.has_attr(*n, "hidden") && inner.styles.get(*n).is_some_and(|s| s.display == Display::None) && !matches!(d.tag(*n), Some("head" | "script" | "style" | "title" | "meta" | "link" | "template"))).collect();
            for n in hidden {
                d.set_attr(n, "hidden", "");
            }
            let _ = d.drain_mutations();
            d
        });
        let mut plain = WebDocument::parse("", &self.url);
        plain.doc = doc;
        plain.base = self.base.clone();
        plain.title = self.title.clone();
        plain.images = self.images.clone();
        plain.refs = self.refs.clone();
        plain.image_errors = self.image_errors.clone();
        let plain = Arc::new(plain);
        guard.0.projection = Some((epoch, plain.clone()));
        plain
    }
}
