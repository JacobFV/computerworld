//! Shared by the parity and reftest runners: the engine pipeline (parse, cascade, lay
//! out, paint), the dump format both Chromium and the engine write, the comparison
//! rules, and a PNG writer for the rasterised scene. See `scripts/web-parity/README.md`.
#![allow(dead_code)]

use cw_scene::Scene;
use cw_web::dom::{Document, NodeId, NodeKind};
use cw_web::geom::{Au, Edges, Point, Rect};
use cw_web::layout::{Fragment, FragmentKind, FragmentTree, StyleSource};
use cw_web::style::{
    BoxSizing, ComputedStyle, Display, Float, LengthPercentage, LengthPercentageAuto, LineHeight, Overflow, Position, Sizing, StyleSet,
    TextAlign, VerticalAlign, WhiteSpace, ZIndex,
};
use cw_web::Viewport;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 800;

/// The computed properties both sides report, in this order (see `common.mjs`).
pub const PROPERTIES: &[&str] = &[
    "display",
    "position",
    "float",
    "width",
    "height",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "font-family",
    "font-size",
    "font-weight",
    "line-height",
    "color",
    "background-color",
    "text-align",
    "white-space",
    "vertical-align",
    "overflow-x",
    "overflow-y",
    "z-index",
    "box-sizing",
];

pub const LENGTH_PROPERTIES: &[&str] = &[
    "width",
    "height",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "font-size",
    "line-height",
];

/// Reported, never compared: the two engines have different font stacks.
pub const INFORMATIONAL: &[&str] = &["font-family"];

/// Tolerances in CSS px: every rect edge within `RECT_PX`; widths and heights of boxes
/// whose size follows their text within `TEXT_PX` (see `text_dependent`).
pub const RECT_PX: f64 = 1.0;
pub const TEXT_PX: f64 = 2.0;

pub fn viewport() -> Viewport {
    Viewport { width: WIDTH, height: HEIGHT, scale: 1, zoom: 100 }
}

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
pub fn parity_dir() -> PathBuf {
    crate_dir().join("tests/parity")
}
pub fn ref_dir() -> PathBuf {
    crate_dir().join("tests/ref")
}
pub fn out_dir() -> PathBuf {
    let d = crate_dir().join("target-parity");
    std::fs::create_dir_all(&d).expect("create target-parity");
    d
}

// ---------------------------------------------------------------------------------
// The pipeline
// ---------------------------------------------------------------------------------

/// Everything one run of the engine produces for a document.
pub struct Rendered {
    pub doc: Document,
    pub styles: StyleSet,
    pub tree: FragmentTree,
    pub scene: Scene,
}

/// Parses, cascades, lays out and paints `html` at `viewport`. Author stylesheets are
/// the document's `<style>` elements, in order; the user-agent sheet is the cascade's.
#[cfg(feature = "pipeline")]
pub fn run(html: &str, viewport: Viewport) -> Rendered {
    use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
    use cw_web::Strictness;
    let doc = cw_web::html::parse(html);
    let mut sheets = Vec::new();
    for node in doc.descendants(Document::ROOT) {
        if doc.is(node, "style") {
            match parse_stylesheet(&doc.text_content(node), Origin::Author, Strictness::Lenient) {
                Ok(sheet) => sheets.push(sheet),
                Err(e) => panic!("stylesheet: {e}"),
            }
        }
    }
    let media = Media::with_size(viewport.width as i32, viewport.height as i32);
    let styles = cw_web::style::cascade(&doc, &sheets, &media, &MatchContext::new(), Strictness::Lenient).unwrap_or_else(|e| panic!("cascade: {e}"));
    let tree = cw_web::layout::layout(&doc, &styles, viewport);
    let scene = cw_web::paint::paint(&doc, &styles, &tree, viewport, &cw_web::paint::PaintContext::default());
    Rendered { doc, styles, tree, scene }
}

#[cfg(not(feature = "pipeline"))]
pub fn run(_html: &str, _viewport: Viewport) -> Rendered {
    panic!("the engine pipeline is not wired up yet: run with `--features pipeline` once html::parse, css::parse_stylesheet and style::cascade exist")
}

// ---------------------------------------------------------------------------------
// The dump format (mirrors scripts/web-parity/dump.mjs)
// ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DumpRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DumpNode {
    Element { path: String, tag: String, id: String, rect: DumpRect, computed: BTreeMap<String, String> },
    Text { path: String, parent: String, text: String, rects: Vec<DumpRect> },
}

impl DumpNode {
    pub fn path(&self) -> &str {
        match self {
            DumpNode::Element { path, .. } | DumpNode::Text { path, .. } => path,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct DumpViewport {
    pub width: u32,
    pub height: u32,
    pub dpr: f64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct DumpSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontPick {
    pub family: String,
    pub engine: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dump {
    pub fixture: String,
    pub engine: String,
    #[serde(default)]
    pub version: String,
    pub viewport: DumpViewport,
    pub properties: Vec<String>,
    pub document: DumpSize,
    pub nodes: Vec<DumpNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<FontPick>,
}

pub fn read_dump(path: &Path) -> Dump {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn q64(au: Au) -> f64 {
    au.0 as f64 / 64.0
}

fn dump_rect(r: Rect) -> DumpRect {
    DumpRect { x: q64(r.origin.x), y: q64(r.origin.y), width: q64(r.size.width), height: q64(r.size.height) }
}

/// A CSS px string the way `getComputedStyle` prints one: up to four decimals, no
/// trailing zeros.
pub fn px(au: Au) -> String {
    let v = q64(au);
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{}px", if s == "-0" { "0" } else { s })
}

pub fn colour(c: cw_scene::Color) -> String {
    if c.3 == 255 {
        format!("rgb({}, {}, {})", c.0, c.1, c.2)
    } else {
        let a = (c.3 as f64 / 255.0 * 100.0).round() / 100.0;
        format!("rgba({}, {}, {}, {})", c.0, c.1, c.2, a)
    }
}

/// What layout produced for one DOM node: the union of its box fragments and the
/// edges of the first one.
#[derive(Clone, Copy, Debug, Default)]
pub struct BoxInfo {
    rect: Option<Rect>,
    padding: Edges,
    border: Edges,
    /// The parent fragment's content box, for resolving auto margins and percentages.
    container: Option<Rect>,
}

struct FragIndex {
    boxes: BTreeMap<NodeId, BoxInfo>,
    texts: BTreeMap<NodeId, Vec<Rect>>,
}

fn index_fragments(doc: &Document, tree: &FragmentTree) -> FragIndex {
    let mut ix = FragIndex { boxes: BTreeMap::new(), texts: BTreeMap::new() };
    fn visit(doc: &Document, f: &Fragment, origin: Point, container: Option<Rect>, ix: &mut FragIndex) {
        let abs = f.rect.translate(origin.x, origin.y);
        let mut inner = container;
        match &f.kind {
            FragmentKind::Box { source, padding, border, .. } | FragmentKind::InlineBox { source, padding, border, .. } => {
                // Pseudo-element boxes (markers, ::before, ::after) are not part of
                // the element's own rect, as `getBoundingClientRect` reports it. A
                // table element's box in Chromium is the wrapper, captions included,
                // so its anonymous wrapper fragment counts as the element's.
                let counts = match source {
                    StyleSource::Element(_) => true,
                    StyleSource::Anonymous(n) => doc.is(*n, "table"),
                    _ => false,
                };
                if counts {
                    let e = ix.boxes.entry(source.node()).or_default();
                    e.rect = Some(match e.rect {
                        Some(r) => r.union(abs),
                        None => abs,
                    });
                    if e.container.is_none() {
                        e.padding = *padding;
                        e.border = *border;
                        e.container = container;
                    }
                }
                let content = border.inset(padding.inset(abs));
                inner = Some(content);
            }
            FragmentKind::Text { node: Some(n), .. } => {
                ix.texts.entry(*n).or_default().push(abs);
            }
            _ => {}
        }
        for c in &f.children {
            visit(doc, c, abs.origin, inner, ix);
        }
    }
    visit(doc, &tree.root, Point::default(), None, &mut ix);
    ix
}

fn display_str(d: Display) -> &'static str {
    match d {
        Display::Inline => "inline",
        Display::Block => "block",
        Display::InlineBlock => "inline-block",
        Display::ListItem => "list-item",
        Display::Flex => "flex",
        Display::InlineFlex => "inline-flex",
        Display::Grid => "grid",
        Display::InlineGrid => "inline-grid",
        Display::Table => "table",
        Display::InlineTable => "inline-table",
        Display::TableRowGroup => "table-row-group",
        Display::TableHeaderGroup => "table-header-group",
        Display::TableFooterGroup => "table-footer-group",
        Display::TableRow => "table-row",
        Display::TableCell => "table-cell",
        Display::TableColumnGroup => "table-column-group",
        Display::TableColumn => "table-column",
        Display::TableCaption => "table-caption",
        Display::FlowRoot => "flow-root",
        Display::Contents => "contents",
        Display::None => "none",
    }
}

fn overflow_str(o: Overflow) -> &'static str {
    match o {
        Overflow::Visible => "visible",
        Overflow::Hidden => "hidden",
        Overflow::Clip => "clip",
        Overflow::Scroll => "scroll",
        Overflow::Auto => "auto",
    }
}

fn lp_str(v: LengthPercentage, base: Option<Au>) -> String {
    match v {
        LengthPercentage::Length(l) => px(l),
        LengthPercentage::Percent(p) => match base {
            Some(b) => px(b.percent_of(p)),
            None => format!("{}%", p as f64 / 100.0),
        },
        LengthPercentage::Calc(l, p) => match base {
            Some(b) => px(l + b.percent_of(p)),
            None => format!("calc({} + {}%)", px(l), p as f64 / 100.0),
        },
    }
}

fn sizing_str(s: Sizing) -> String {
    match s {
        Sizing::Auto => "auto".into(),
        Sizing::Set(v) => lp_str(v, None),
        Sizing::MinContent => "min-content".into(),
        Sizing::MaxContent => "max-content".into(),
        Sizing::FitContent => "fit-content".into(),
        Sizing::None => "none".into(),
    }
}

/// The computed strings for one element, the way `getComputedStyle` reports them:
/// used values for the box when it was laid out, computed values when it was not.
pub fn computed_strings(style: &ComputedStyle, info: &BoxInfo) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        m.insert(k.to_owned(), v);
    };
    put("display", display_str(style.display).into());
    put(
        "position",
        match style.position {
            Position::Static => "static",
            Position::Relative => "relative",
            Position::Absolute => "absolute",
            Position::Fixed => "fixed",
            Position::Sticky => "sticky",
        }
        .into(),
    );
    put(
        "float",
        match style.float {
            Float::None => "none",
            Float::Left => "left",
            Float::Right => "right",
        }
        .into(),
    );
    let cb_width = info.container.map(|c| c.size.width);
    let inline_nonreplaced = matches!(style.display, Display::Inline);
    match info.rect {
        Some(r) if !inline_nonreplaced => {
            let (w, h) = match style.box_sizing {
                BoxSizing::BorderBox => (r.size.width, r.size.height),
                BoxSizing::ContentBox => (
                    r.size.width - info.padding.horizontal() - info.border.horizontal(),
                    r.size.height - info.padding.vertical() - info.border.vertical(),
                ),
            };
            put("width", px(w));
            put("height", px(h));
        }
        Some(_) => {
            put("width", "auto".into());
            put("height", "auto".into());
        }
        None => {
            put("width", sizing_str(style.width));
            put("height", sizing_str(style.height));
        }
    }
    let margin = |v: LengthPercentageAuto, side: usize| -> String {
        match v {
            LengthPercentageAuto::Set(l) => lp_str(l, cb_width),
            LengthPercentageAuto::Auto => match (info.rect, info.container) {
                (Some(r), Some(c)) if !inline_nonreplaced && side % 2 == 1 => {
                    // Horizontal auto margins resolve from where the box landed.
                    if side == 3 {
                        px((r.origin.x - c.origin.x).max(Au::ZERO))
                    } else {
                        px((c.right() - r.right()).max(Au::ZERO))
                    }
                }
                _ => "0px".into(),
            },
        }
    };
    put("margin-top", margin(style.margin.top, 0));
    put("margin-right", margin(style.margin.right, 1));
    put("margin-bottom", margin(style.margin.bottom, 2));
    put("margin-left", margin(style.margin.left, 3));
    put("padding-top", lp_str(style.padding.top, cb_width));
    put("padding-right", lp_str(style.padding.right, cb_width));
    put("padding-bottom", lp_str(style.padding.bottom, cb_width));
    put("padding-left", lp_str(style.padding.left, cb_width));
    let bw = style.used_border_widths();
    put("border-top-width", px(bw.top));
    put("border-right-width", px(bw.right));
    put("border-bottom-width", px(bw.bottom));
    put("border-left-width", px(bw.left));
    put("font-family", style.font.family.clone());
    put("font-size", px(style.font.size));
    put("font-weight", style.font.weight.to_string());
    put(
        "line-height",
        match style.line_height {
            LineHeight::Normal => "normal".into(),
            LineHeight::Number(_) => px(style.line_height_au(Au::ZERO)),
            LineHeight::Length(l) => px(l),
        },
    );
    put("color", colour(style.color));
    put("background-color", colour(style.background_color));
    put(
        "text-align",
        match style.text_align {
            TextAlign::Start => "start",
            TextAlign::End => "end",
            TextAlign::Left => "left",
            TextAlign::Right => "right",
            TextAlign::Center => "center",
            TextAlign::Justify => "justify",
            TextAlign::WebkitCenter => "-webkit-center",
        }
        .into(),
    );
    put(
        "white-space",
        match style.white_space {
            WhiteSpace::Normal => "normal",
            WhiteSpace::NoWrap => "nowrap",
            WhiteSpace::Pre => "pre",
            WhiteSpace::PreWrap => "pre-wrap",
            WhiteSpace::PreLine => "pre-line",
            WhiteSpace::BreakSpaces => "break-spaces",
        }
        .into(),
    );
    put(
        "vertical-align",
        match style.vertical_align {
            VerticalAlign::Baseline => "baseline".into(),
            VerticalAlign::Sub => "sub".into(),
            VerticalAlign::Super => "super".into(),
            VerticalAlign::TextTop => "text-top".into(),
            VerticalAlign::TextBottom => "text-bottom".into(),
            VerticalAlign::Middle => "middle".into(),
            VerticalAlign::Top => "top".into(),
            VerticalAlign::Bottom => "bottom".into(),
            VerticalAlign::Length(l) => lp_str(l, None),
        },
    );
    put("overflow-x", overflow_str(style.overflow_x).into());
    put("overflow-y", overflow_str(style.overflow_y).into());
    put(
        "z-index",
        match style.z_index {
            ZIndex::Auto => "auto".into(),
            ZIndex::Int(i) => i.to_string(),
        },
    );
    put(
        "box-sizing",
        match style.box_sizing {
            BoxSizing::ContentBox => "content-box",
            BoxSizing::BorderBox => "border-box",
        }
        .into(),
    );
    m
}

fn union_rect(a: DumpRect, b: DumpRect) -> DumpRect {
    if a.width <= 0.0 && a.height <= 0.0 {
        return b;
    }
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    DumpRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
}

/// The engine's dump of a rendered document, in the shape `dump.mjs` writes. Paths
/// follow the same scheme: `html`, `html>body`, then `tag:nth-child(n)` per element
/// (counting element siblings), `#text:nth(i)` per non-blank text child. `<head>` and
/// its subtree are skipped, as they are in Chromium's dump.
pub fn engine_dump(fixture: &str, r: &Rendered, viewport: Viewport) -> Dump {
    let ix = index_fragments(&r.doc, &r.tree);
    let mut nodes = Vec::new();
    let mut families: Vec<String> = Vec::new();
    fn walk(doc: &Document, styles: &StyleSet, ix: &FragIndex, node: NodeId, path: String, out: &mut Vec<DumpNode>, fams: &mut Vec<String>) {
        let tag = doc.tag(node).unwrap_or("").to_owned();
        let info = ix.boxes.get(&node).copied().unwrap_or_default();
        let computed = match styles.get(node) {
            Some(s) => computed_strings(s, &info),
            None => BTreeMap::new(),
        };
        if let Some(f) = computed.get("font-family") {
            if !fams.contains(f) {
                fams.push(f.clone());
            }
        }
        out.push(DumpNode::Element {
            path: path.clone(),
            tag: tag.clone(),
            id: doc.attr(node, "id").unwrap_or("").to_owned(),
            rect: info.rect.map(dump_rect).unwrap_or_default(),
            computed,
        });
        let my_index = out.len() - 1;
        let mut n = 0;
        let mut text_index = 0;
        let inline_parent = matches!(styles.get(node).map(|s| s.display), Some(Display::Inline));
        for child in doc.children(node) {
            match doc.kind(child) {
                NodeKind::Element { tag: ct, .. } => {
                    n += 1;
                    if ct == "head" {
                        continue;
                    }
                    let seg = if ct == "body" && tag == "html" { "body".to_owned() } else { format!("{ct}:nth-child({n})") };
                    let child_index = out.len();
                    walk(doc, styles, ix, child, format!("{path}>{seg}"), out, fams);
                    // Block-in-inline: Blink keeps the block children of an inline
                    // element inside its fragments (an anonymous block-in-inline
                    // box), so the inline's client rect covers them; the engine lays
                    // them out as siblings of the split inline pieces, so union them
                    // here.
                    if inline_parent {
                        if let Some(cs) = styles.get(child) {
                            let in_flow_block = !cs.display.is_inline_level() && !cs.display.is_none() && cs.float == Float::None && !matches!(cs.position, Position::Absolute | Position::Fixed);
                            if in_flow_block {
                                let child_rect = match &out[child_index] {
                                    DumpNode::Element { rect, .. } => *rect,
                                    _ => DumpRect::default(),
                                };
                                if child_rect.width > 0.0 || child_rect.height > 0.0 {
                                    if let DumpNode::Element { rect, .. } = &mut out[my_index] {
                                        *rect = union_rect(*rect, child_rect);
                                    }
                                }
                            }
                        }
                    }
                }
                NodeKind::Text(t) => {
                    if t.trim().is_empty() {
                        continue;
                    }
                    let rects = ix.texts.get(&child).map(|v| v.iter().map(|r| dump_rect(*r)).collect()).unwrap_or_default();
                    out.push(DumpNode::Text { path: format!("{path}>#text:nth({text_index})"), parent: path.clone(), text: t.clone(), rects });
                    text_index += 1;
                }
                _ => {}
            }
        }
    }
    if let Some(html) = r.doc.document_element() {
        walk(&r.doc, &r.styles, &ix, html, "html".into(), &mut nodes, &mut families);
    }
    let fonts = families
        .into_iter()
        .map(|family| FontPick { engine: cw_scene::fonts::resolve_family(&family).family_name().to_owned(), family })
        .collect();
    Dump {
        fixture: fixture.to_owned(),
        engine: "cw-web".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        viewport: DumpViewport { width: viewport.width, height: viewport.height, dpr: viewport.scale as f64 },
        properties: PROPERTIES.iter().map(|s| s.to_string()).collect(),
        document: DumpSize { width: q64(r.tree.content_width), height: q64(r.tree.content_height) },
        nodes,
        fonts,
    }
}

// ---------------------------------------------------------------------------------
// Comparison (mirrors scripts/web-parity/compare.mjs)
// ---------------------------------------------------------------------------------

pub fn normalise(property: &str, value: &str) -> String {
    let v = value.trim();
    match property {
        "text-align" => v.strip_prefix("-webkit-").unwrap_or(v).to_owned(),
        "font-weight" => match v {
            "normal" => "400".into(),
            "bold" => "700".into(),
            _ => v.into(),
        },
        "color" | "background-color" => normalise_colour(v),
        _ => v.to_owned(),
    }
}

fn normalise_colour(v: &str) -> String {
    let inner = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")).and_then(|s| s.strip_suffix(')'));
    let Some(inner) = inner else { return v.to_owned() };
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() < 3 {
        return v.to_owned();
    }
    let a: f64 = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    if a >= 1.0 {
        format!("rgb({}, {}, {})", parts[0], parts[1], parts[2])
    } else {
        format!("rgba({}, {}, {}, {})", parts[0], parts[1], parts[2], (a * 100.0).round() / 100.0)
    }
}

pub fn parse_px(v: &str) -> Option<f64> {
    v.trim().strip_suffix("px").and_then(|n| n.parse().ok())
}

/// Whether an element's width and height depend on the text it contains.
pub fn text_dependent(computed: &BTreeMap<String, String>) -> bool {
    let g = |k: &str| computed.get(k).map(String::as_str).unwrap_or("");
    let d = g("display");
    g("float") != "none" && !g("float").is_empty()
        || matches!(g("position"), "absolute" | "fixed")
        || d.starts_with("inline")
        || d.starts_with("table")
        || (d == "list-item" && g("width") == "auto")
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Mismatch {
    pub what: String,
    pub expected: String,
    pub got: String,
    pub delta: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct NodeResult {
    pub path: String,
    pub passed: bool,
    pub mismatches: Vec<Mismatch>,
    /// Sum of numeric deltas, for ranking offenders.
    pub score: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub fixture: String,
    pub total: usize,
    pub passed: usize,
    pub missing: usize,
    pub by_property: BTreeMap<String, usize>,
    pub nodes: Vec<NodeResult>,
}

impl Report {
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.passed as f64 / self.total as f64
        }
    }
    pub fn worst(&self, n: usize) -> Vec<&NodeResult> {
        let mut v: Vec<&NodeResult> = self.nodes.iter().filter(|r| !r.passed).collect();
        v.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.path.cmp(&b.path)));
        v.truncate(n);
        v
    }
    pub fn to_markdown(&self, fonts: &[FontPick], threshold: f64) -> String {
        let mut s = String::new();
        s.push_str(&format!("# Parity: {}\n\n", self.fixture));
        s.push_str(&format!(
            "- nodes: {} (elements and text nodes in Chromium's dump)\n- passed: {} ({:.1}%)\n- missing from the engine's dump: {}\n- threshold: {:.1}%\n- verdict: {}\n\n",
            self.total,
            self.passed,
            self.pass_rate() * 100.0,
            self.missing,
            threshold * 100.0,
            if self.pass_rate() >= threshold { "pass" } else { "FAIL" }
        ));
        s.push_str("Tolerances: rect edges within 1 px; widths and heights of text-dependent boxes (inline, table parts, floats, absolutes) within 2 px; keyword properties exact; `font-family` informational.\n\n");
        if !fonts.is_empty() {
            s.push_str("## Fonts\n\n| font-family | engine face |\n|---|---|\n");
            for f in fonts {
                s.push_str(&format!("| `{}` | {} |\n", f.family.replace('|', "\\|"), f.engine));
            }
            s.push('\n');
        }
        s.push_str("## Mismatches by property\n\n| property | nodes |\n|---|---|\n");
        for (k, v) in &self.by_property {
            s.push_str(&format!("| {k} | {v} |\n"));
        }
        s.push_str("\n## Worst offenders\n\n");
        for r in self.worst(40) {
            s.push_str(&format!("### `{}` (score {:.2})\n\n", r.path, r.score));
            for m in &r.mismatches {
                s.push_str(&format!("- {}: expected `{}`, got `{}`\n", m.what, m.expected, m.got));
            }
            s.push('\n');
        }
        s
    }
}

fn rect_mismatches(expected: &DumpRect, got: &DumpRect, size_tolerance: f64, prefix: &str, out: &mut Vec<Mismatch>) {
    let checks = [("x", expected.x, got.x, RECT_PX), ("y", expected.y, got.y, RECT_PX), ("width", expected.width, got.width, size_tolerance), ("height", expected.height, got.height, size_tolerance)];
    for (name, e, g, tol) in checks {
        let d = (e - g).abs();
        if d > tol + 1e-9 {
            out.push(Mismatch { what: format!("{prefix}{name}"), expected: format!("{e}"), got: format!("{g}"), delta: d });
        }
    }
}

/// Compares Chromium's dump (`expected`) against the engine's (`got`).
pub fn compare(expected: &Dump, got: &Dump) -> Report {
    let by_path: BTreeMap<&str, &DumpNode> = got.nodes.iter().map(|n| (n.path(), n)).collect();
    let mut report = Report { fixture: expected.fixture.clone(), total: expected.nodes.len(), ..Default::default() };
    for node in &expected.nodes {
        let mut result = NodeResult { path: node.path().to_owned(), ..Default::default() };
        match (node, by_path.get(node.path())) {
            (_, None) => {
                report.missing += 1;
                result.mismatches.push(Mismatch { what: "node".into(), expected: "present".into(), got: "missing".into(), delta: 1000.0 });
            }
            (DumpNode::Element { tag, rect, computed, .. }, Some(DumpNode::Element { rect: got_rect, computed: got_computed, .. })) => {
                let text_dep = text_dependent(computed);
                let size_tol = if text_dep { TEXT_PX } else { RECT_PX };
                // A `<br>` generates no box; Chromium reports the line break's position,
                // the engine nothing. Its computed values are still compared.
                if tag != "br" {
                    rect_mismatches(rect, got_rect, size_tol, "rect.", &mut result.mismatches);
                }
                for p in PROPERTIES {
                    if INFORMATIONAL.contains(p) {
                        continue;
                    }
                    let e = normalise(p, computed.get(*p).map(String::as_str).unwrap_or(""));
                    let g = normalise(p, got_computed.get(*p).map(String::as_str).unwrap_or(""));
                    if e == g {
                        continue;
                    }
                    if LENGTH_PROPERTIES.contains(p) {
                        if let (Some(ep), Some(gp)) = (parse_px(&e), parse_px(&g)) {
                            let tol = if text_dep && (*p == "width" || *p == "height") { TEXT_PX } else { RECT_PX };
                            let d = (ep - gp).abs();
                            if d <= tol + 1e-9 {
                                continue;
                            }
                            result.mismatches.push(Mismatch { what: p.to_string(), expected: e, got: g, delta: d });
                            continue;
                        }
                    }
                    result.mismatches.push(Mismatch { what: p.to_string(), expected: e, got: g, delta: 1.0 });
                }
            }
            (DumpNode::Text { rects, .. }, Some(DumpNode::Text { rects: got_rects, .. })) => {
                if rects.len() != got_rects.len() {
                    result.mismatches.push(Mismatch {
                        what: "lines".into(),
                        expected: rects.len().to_string(),
                        got: got_rects.len().to_string(),
                        delta: (rects.len() as f64 - got_rects.len() as f64).abs() * 10.0,
                    });
                } else {
                    for (i, (e, g)) in rects.iter().zip(got_rects).enumerate() {
                        rect_mismatches(e, g, TEXT_PX, &format!("line[{i}]."), &mut result.mismatches);
                    }
                }
            }
            (_, Some(_)) => {
                result.mismatches.push(Mismatch { what: "kind".into(), expected: "same kind".into(), got: "other kind".into(), delta: 1000.0 });
            }
        }
        result.passed = result.mismatches.is_empty();
        result.score = result.mismatches.iter().map(|m| m.delta).sum();
        for m in &result.mismatches {
            let key = m.what.split('.').next().unwrap_or(&m.what).to_owned();
            *report.by_property.entry(key).or_default() += 1;
        }
        if result.passed {
            report.passed += 1;
        }
        report.nodes.push(result);
    }
    report
}

// ---------------------------------------------------------------------------------
// Thresholds
// ---------------------------------------------------------------------------------

pub fn thresholds() -> BTreeMap<String, f64> {
    let p = parity_dir().join("thresholds.json");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", p.display()))
}

// ---------------------------------------------------------------------------------
// Scenes: rasterising and normalising
// ---------------------------------------------------------------------------------

/// Rasterises a scene through the shared renderer to RGBA.
pub fn rasterise(scene: &Scene) -> cw_render::Frame {
    let mut r = cw_render::Renderer::new();
    r.render(scene)
}

/// The digest of a scene with node ids erased, so two documents that paint the same
/// content with different DOM node ids compare equal. Reftests compare these.
/// Accessibility regions (`Primitive::Region`: an `<hr>`'s "separator", a table's
/// "table" and "cell" roles, a `<pre>`'s "code") paint nothing and follow the
/// element's tag, so a test and its reference written with different elements
/// legitimately differ in them; they are dropped before hashing.
/// The paint ordinal `z` is renumbered after the regions go, and scroll areas
/// drop their `target`, which names the element's DOM path.
pub fn content_digest(scene: &Scene) -> u64 {
    let mut s = scene.clone();
    s.nodes.retain(|n| !matches!(n.primitive, cw_scene::Primitive::Region));
    for (i, n) in s.nodes.iter_mut().enumerate() {
        n.id = 0;
        n.revision = 0;
        n.z = i as i32;
    }
    for a in &mut s.scrolls {
        a.target = String::new();
    }
    s.revision = 0;
    s.stamp();
    s.digest
}

// ---------------------------------------------------------------------------------
// PNG (stored deflate: no compression library, no dependency)
// ---------------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, slot) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = Vec::with_capacity(4 + data.len());
    body.extend_from_slice(kind);
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// Encodes RGBA pixels as a PNG with stored (uncompressed) deflate blocks.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    assert_eq!(rgba.len(), (width * height * 4) as usize);
    let stride = (width * 4) as usize;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in rgba.chunks(stride) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    if blocks.is_empty() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    for (i, b) in blocks.iter().enumerate() {
        z.push(if i + 1 == blocks.len() { 1 } else { 0 });
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

pub fn write_png(path: &Path, frame: &cw_render::Frame) {
    std::fs::write(path, encode_png(frame.width, frame.height, &frame.rgba)).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}
