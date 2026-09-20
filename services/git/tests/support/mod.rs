//! Shared by the integration tests: a minimal HTML reader over `cw_web::html::parse`,
//! the strict validator, and the engine render the stills use.
#![allow(dead_code)]
use cw_web::dom::{Document, NodeId};

pub struct Page {
    pub doc: Document,
    pub html: String,
}
impl Page {
    /// Parses a page and runs it through the strict validator: unsupported CSS, a
    /// repeated id or a linked sheet fails the test that fetched it.
    pub fn parse(context: &str, html: String) -> Page {
        cw_service_common::html::validate_strict(&html).unwrap_or_else(|e| panic!("{context}: {e:?}"));
        Page { doc: cw_web::html::parse(&html), html }
    }
    pub fn title(&self) -> String {
        let node = self.doc.descendants(Document::ROOT).find(|n| self.doc.is(*n, "title")).expect("title");
        self.doc.text_content(node)
    }
    pub fn find(&self, id: &str) -> Option<NodeId> {
        self.doc.by_id(id).first().copied()
    }
    pub fn node(&self, id: &str) -> NodeId {
        self.find(id).unwrap_or_else(|| panic!("no element #{id}"))
    }
    pub fn has(&self, id: &str) -> bool {
        self.find(id).is_some()
    }
    /// Text content with runs of whitespace collapsed.
    pub fn text(&self, id: &str) -> String {
        self.doc.text_content(self.node(id)).split_whitespace().collect::<Vec<_>>().join(" ")
    }
    pub fn raw_text(&self, id: &str) -> String {
        self.doc.text_content(self.node(id))
    }
    pub fn attr(&self, id: &str, name: &str) -> String {
        self.doc.attr(self.node(id), name).unwrap_or_else(|| panic!("#{id} has no {name}")).to_owned()
    }
    pub fn tag(&self, id: &str) -> String {
        self.doc.tag(self.node(id)).unwrap_or("").to_owned()
    }
    pub fn has_class(&self, id: &str, class: &str) -> bool {
        self.doc.attr(self.node(id), "class").is_some_and(|c| c.split_ascii_whitespace().any(|k| k == class))
    }
    /// Ids of every element whose id starts with `prefix`, in document order.
    pub fn ids_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.doc
            .descendants(Document::ROOT)
            .filter_map(|n| self.doc.attr(n, "id"))
            .filter(|id| id.starts_with(prefix))
            .map(str::to_owned)
            .collect()
    }
    /// The whole page's text, whitespace collapsed.
    pub fn all_text(&self) -> String {
        let body = self.doc.descendants(Document::ROOT).find(|n| self.doc.is(*n, "body")).expect("body");
        self.doc.text_content(body).split_whitespace().collect::<Vec<_>>().join(" ")
    }
    /// The form an element belongs to: `(action, method, fields)` with hidden and text
    /// fields by name.
    pub fn form(&self, id: &str) -> (String, String, Vec<(String, String)>) {
        let form = self.node(id);
        assert!(self.doc.is(form, "form"), "#{id} is not a form");
        let fields = self
            .doc
            .descendants(form)
            .filter(|n| matches!(self.doc.tag(*n), Some("input" | "textarea" | "select")))
            .filter_map(|n| Some((self.doc.attr(n, "name")?.to_owned(), self.doc.attr(n, "value").unwrap_or("").to_owned())))
            .collect();
        (
            self.doc.attr(form, "action").unwrap_or("").to_owned(),
            self.doc.attr(form, "method").unwrap_or("get").to_owned(),
            fields,
        )
    }
    /// The id of the form that owns `id`.
    pub fn form_of(&self, id: &str) -> String {
        let node = self.node(id);
        let form = self.doc.ancestors(node).find(|a| self.doc.is(*a, "form")).unwrap_or_else(|| panic!("#{id} is in no form"));
        self.doc.attr(form, "id").unwrap_or("").to_owned()
    }
}

/// Renders a page through the engine to `research/site-stills/<file>`.
pub fn still(html: &str, file: &str, width: u32, height: u32) {
    use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
    use cw_web::{Strictness, Viewport};
    let viewport = Viewport { width, height, scale: 1, zoom: 100 };
    let doc = cw_web::html::parse(html);
    let mut sheets = Vec::new();
    for node in doc.descendants(Document::ROOT) {
        if doc.is(node, "style") {
            sheets.push(parse_stylesheet(&doc.text_content(node), Origin::Author, Strictness::Strict).unwrap());
        }
    }
    let media = Media::with_size(width as i32, height as i32);
    let styles = cw_web::style::cascade(&doc, &sheets, &media, &MatchContext::new(), Strictness::Strict).unwrap();
    let tree = cw_web::layout::layout(&doc, &styles, viewport);
    let scene = cw_web::paint::paint(&doc, &styles, &tree, viewport, &cw_web::paint::PaintContext::default());
    let frame = cw_render::Renderer::new().render(&scene);
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&frame.rgba).unwrap();
    }
    let target = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../research/site-stills").join(file);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("write {}: {e}", target.display()));
    println!("wrote {}", target.display());
}
