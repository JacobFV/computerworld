//! Inline `<svg>`: the element is a replaced box (see `layout::boxes`), and this
//! module turns its subtree into what layout and paint need from it.
//!
//! [`build`] walks the subtree once for a given content-box size: it maps the
//! `viewBox` into the box (`preserveAspectRatio`), follows `transform`s and the
//! inherited presentation properties (`fill`, `stroke`, their opacities, stroke
//! width, caps, joins, dashes, `opacity`, `text-anchor`), flattens every shape to
//! device-pixel polylines, and records each element's bounding box as
//! `getBoundingClientRect` reports it (the fill geometry, without the stroke,
//! mapped to the box; a group is the union of its children; text is the run's
//! advance by the font's rounded ascent and descent). Layout hangs those boxes on
//! the svg's fragment as unpainted child fragments, so the DOM's client rects and
//! the parity dumps see them; paint rasterises the shapes ([`raster`]) and sets the
//! text with the page's own text painter.
//!
//! Supported: `svg` (the outermost), `g`, `path`, `rect`, `circle`, `ellipse`,
//! `line`, `polyline`, `polygon`, `text` (its character data as one run),
//! `linearGradient` and `radialGradient` paint servers referenced by `url(#id)`.
//! Presentation attributes and the `style` attribute's declarations of the same
//! properties are read; `currentColor` is the element's computed `color`.
//! Not supported: `use`, `clipPath`, `mask`, `pattern`, `marker`, `image`,
//! `foreignObject`, filters, `tspan` positioning and nested `svg` viewports.

pub mod geom;
pub mod raster;

use crate::dom::{Document, Namespace, NodeId, NodeKind};
use crate::style::computed::{Font, StyleSet};
use crate::style::values::{parse_color, ColorSpec, Parser};
use cw_scene::Color;
use geom::{Affine, Flattener, Poly, Pt};
use raster::{Cap, FillRule, Join, Paint, Stroke};

/// Whether `node` is an element in the SVG namespace.
pub fn is_svg(doc: &Document, node: NodeId) -> bool {
    matches!(
        doc.kind(node),
        NodeKind::Element {
            ns: Namespace::Svg,
            ..
        }
    )
}

/// An axis-aligned box in content-box pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl BoxF {
    fn of_points<'a>(pts: impl Iterator<Item = &'a Pt>) -> Option<BoxF> {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        (x0 <= x1).then_some(BoxF {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    }

    fn union(self, o: BoxF) -> BoxF {
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = (self.x + self.w).max(o.x + o.w);
        let y1 = (self.y + self.h).max(o.y + o.h);
        BoxF {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }
}

/// A filled and/or stroked shape, flattened into device pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub polys: Vec<Poly>,
    pub fill: Option<(Paint, FillRule, f64)>,
    pub stroke: Option<(Stroke, Paint, f64)>,
}

/// A `<text>` element's run: `x` is its left edge and `baseline` its baseline, in
/// content-box pixels; `font` is already scaled to device pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub element: NodeId,
    pub text: String,
    pub x: f64,
    pub baseline: f64,
    pub font: Font,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Shape(Box<Shape>),
    Text(TextRun),
}

/// Everything an `<svg>` element draws and reports, for one content-box size.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Built {
    /// In paint order.
    pub items: Vec<Item>,
    /// Descendant elements' bounding boxes, in document order.
    pub boxes: Vec<(NodeId, BoxF)>,
    /// Each text node under a `<text>`: its box.
    pub text_boxes: Vec<(NodeId, BoxF)>,
}

#[derive(Clone, Debug, PartialEq)]
enum PaintSpec {
    None,
    Color(Color),
    Current,
    Url(String, Option<Box<PaintSpec>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    Start,
    Middle,
    End,
}

/// The inherited SVG properties at a point in the tree.
#[derive(Clone, Debug)]
struct Ctx {
    m: Affine,
    fill: PaintSpec,
    fill_opacity: f64,
    fill_rule: FillRule,
    stroke: PaintSpec,
    stroke_opacity: f64,
    stroke_width: f64,
    cap: Cap,
    join: Join,
    miter_limit: f64,
    dashes: Vec<f64>,
    dash_offset: f64,
    /// `opacity` down the tree, multiplied (groups are not composited separately).
    opacity: f64,
    anchor: Anchor,
    visible: bool,
}

impl Ctx {
    fn initial(m: Affine) -> Ctx {
        Ctx {
            m,
            fill: PaintSpec::Color(Color(0, 0, 0, 255)),
            fill_opacity: 1.0,
            fill_rule: FillRule::NonZero,
            stroke: PaintSpec::None,
            stroke_opacity: 1.0,
            stroke_width: 1.0,
            cap: Cap::Butt,
            join: Join::Miter,
            miter_limit: 4.0,
            dashes: Vec::new(),
            dash_offset: 0.0,
            opacity: 1.0,
            anchor: Anchor::Start,
            visible: true,
        }
    }
}

/// A presentation property of `node`: its `style` attribute's declaration, else its
/// attribute.
fn prop(doc: &Document, node: NodeId, name: &str) -> Option<String> {
    if let Some(style) = doc.attr(node, "style") {
        for decl in style.split(';') {
            if let Some((k, v)) = decl.split_once(':') {
                if k.trim().eq_ignore_ascii_case(name) {
                    return Some(v.trim().to_owned());
                }
            }
        }
    }
    doc.attr(node, name).map(|v| v.trim().to_owned())
}

fn parse_paint(s: &str) -> Option<PaintSpec> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("none") {
        return Some(PaintSpec::None);
    }
    if s.eq_ignore_ascii_case("currentcolor") {
        return Some(PaintSpec::Current);
    }
    if let Some(rest) = s.strip_prefix("url(") {
        let close = rest.find(')')?;
        let id = rest[..close].trim().trim_matches(|c| c == '"' || c == '\'');
        let id = id.strip_prefix('#')?.to_owned();
        let fallback = parse_paint(&rest[close + 1..]).map(Box::new);
        return Some(PaintSpec::Url(id, fallback));
    }
    parse_css_color(s).map(PaintSpec::Color)
}

fn parse_css_color(s: &str) -> Option<Color> {
    let values = crate::css::parser::parse_component_value_list(s.trim());
    let mut p = Parser::new(&values);
    match parse_color(&mut p)? {
        ColorSpec::Rgba(c) => Some(c),
        _ => None,
    }
}

fn opacity_value(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        return geom::number(p).map(|(v, _)| (v / 100.0).clamp(0.0, 1.0));
    }
    geom::number(s).map(|(v, _)| v.clamp(0.0, 1.0))
}

/// The CSS `color` of `node` (for `currentColor`).
fn current_color(styles: &StyleSet, node: NodeId) -> Color {
    styles
        .get(node)
        .map(|s| s.color)
        .unwrap_or(Color(0, 0, 0, 255))
}

/// Applies `node`'s own presentation properties to the inherited `ctx`.
fn cascade(doc: &Document, node: NodeId, ctx: &mut Ctx) {
    let p = |n: &str| prop(doc, node, n);
    if let Some(t) = doc.attr(node, "transform") {
        ctx.m = ctx.m.then(&geom::parse_transform(t));
    }
    if let Some(v) = p("fill").and_then(|v| parse_paint(&v)) {
        ctx.fill = v;
    }
    if let Some(v) = p("stroke").and_then(|v| parse_paint(&v)) {
        ctx.stroke = v;
    }
    if let Some(v) = p("fill-opacity").and_then(|v| opacity_value(&v)) {
        ctx.fill_opacity = v;
    }
    if let Some(v) = p("stroke-opacity").and_then(|v| opacity_value(&v)) {
        ctx.stroke_opacity = v;
    }
    if let Some(v) = p("opacity").and_then(|v| opacity_value(&v)) {
        ctx.opacity *= v;
    }
    if let Some(v) = p("stroke-width").and_then(|v| geom::length(&v, 0.0)) {
        ctx.stroke_width = v.max(0.0);
    }
    match p("fill-rule").as_deref() {
        Some("evenodd") => ctx.fill_rule = FillRule::EvenOdd,
        Some("nonzero") => ctx.fill_rule = FillRule::NonZero,
        _ => {}
    }
    match p("stroke-linecap").as_deref() {
        Some("round") => ctx.cap = Cap::Round,
        Some("square") => ctx.cap = Cap::Square,
        Some("butt") => ctx.cap = Cap::Butt,
        _ => {}
    }
    match p("stroke-linejoin").as_deref() {
        Some("round") => ctx.join = Join::Round,
        Some("bevel") => ctx.join = Join::Bevel,
        Some("miter") | Some("miter-clip") | Some("arcs") => ctx.join = Join::Miter,
        _ => {}
    }
    if let Some(v) = p("stroke-miterlimit").and_then(|v| geom::number(&v).map(|x| x.0)) {
        ctx.miter_limit = v.max(1.0);
    }
    if let Some(v) = p("stroke-dasharray") {
        if v == "none" {
            ctx.dashes.clear();
        } else {
            let mut d = geom::numbers(&v);
            if d.len() % 2 == 1 {
                d.extend(d.clone());
            }
            ctx.dashes = d;
        }
    }
    if let Some(v) = p("stroke-dashoffset").and_then(|v| geom::length(&v, 0.0)) {
        ctx.dash_offset = v;
    }
    match p("text-anchor").as_deref() {
        Some("middle") => ctx.anchor = Anchor::Middle,
        Some("end") => ctx.anchor = Anchor::End,
        Some("start") => ctx.anchor = Anchor::Start,
        _ => {}
    }
    match p("visibility").as_deref() {
        Some("hidden") | Some("collapse") => ctx.visible = false,
        Some("visible") => ctx.visible = true,
        _ => {}
    }
}

/// `viewBox` and `preserveAspectRatio` mapped into a `w × h` viewport.
fn viewbox_transform(doc: &Document, svg: NodeId, w: f64, h: f64) -> Affine {
    let Some(vb) = doc.attr(svg, "viewBox").map(geom::numbers) else {
        return Affine::IDENTITY;
    };
    let [vx, vy, vw, vh] = vb[..] else {
        return Affine::IDENTITY;
    };
    if vw <= 0.0 || vh <= 0.0 {
        return Affine::IDENTITY;
    }
    let par = doc
        .attr(svg, "preserveAspectRatio")
        .unwrap_or("xMidYMid meet");
    let mut words = par.split_whitespace();
    let align = words.next().unwrap_or("xMidYMid");
    let slice = words.next() == Some("slice");
    let (sx, sy) = (w / vw, h / vh);
    if align == "none" {
        return Affine::scale(sx, sy).then(&Affine::translate(-vx, -vy));
    }
    let s = if slice { sx.max(sy) } else { sx.min(sy) };
    let (ax, ay) = (
        if align.contains("xMin") {
            0.0
        } else if align.contains("xMax") {
            1.0
        } else {
            0.5
        },
        if align.contains("YMin") {
            0.0
        } else if align.contains("YMax") {
            1.0
        } else {
            0.5
        },
    );
    let tx = (w - vw * s) * ax;
    let ty = (h - vh * s) * ay;
    Affine::translate(tx, ty)
        .then(&Affine::scale(s, s))
        .then(&Affine::translate(-vx, -vy))
}

/// The natural size and ratio of an `<svg>` from its attributes: `width` and
/// `height` in px when both are lengths, and the `viewBox` ratio.
pub fn natural_size(doc: &Document, svg: NodeId) -> (Option<f64>, Option<f64>, Option<f64>) {
    let len = |n: &str| {
        doc.attr(svg, n)
            .filter(|v| !v.trim().ends_with('%'))
            .and_then(|v| geom::length(v, 0.0))
    };
    let ratio = doc
        .attr(svg, "viewBox")
        .map(geom::numbers)
        .and_then(|v| match v[..] {
            [_, _, w, h] if w > 0.0 && h > 0.0 => Some(w / h),
            _ => None,
        });
    (len("width"), len("height"), ratio)
}

struct Builder<'a> {
    doc: &'a Document,
    styles: &'a StyleSet,
    viewport: (f64, f64),
    out: Built,
}

/// Builds `svg`'s drawing and boxes for a content box of `w × h` pixels.
pub fn build(doc: &Document, styles: &StyleSet, svg: NodeId, w: f64, h: f64) -> Built {
    build_at(doc, styles, svg, w, h, Pt::default())
}

/// [`build`] with the content box's origin at `origin` in the output's pixels.
pub fn build_at(
    doc: &Document,
    styles: &StyleSet,
    svg: NodeId,
    w: f64,
    h: f64,
    origin: Pt,
) -> Built {
    let mut b = Builder {
        doc,
        styles,
        viewport: (w, h),
        out: Built::default(),
    };
    let base = Affine::translate(origin.x, origin.y).then(&viewbox_transform(doc, svg, w, h));
    let mut ctx = Ctx::initial(base);
    // The root's own presentation attributes (Lucide puts fill, stroke and the
    // stroke style there); its `transform` does not map its own viewport.
    let t = ctx.m;
    cascade(doc, svg, &mut ctx);
    ctx.m = t;
    let vb = doc.attr(svg, "viewBox").map(geom::numbers);
    let user_viewport = match vb.as_deref() {
        Some([_, _, vw, vh]) => (*vw, *vh),
        _ => (w, h),
    };
    b.viewport = user_viewport;
    for c in doc.children(svg) {
        b.node(c, &ctx);
    }
    b.out
}

impl<'a> Builder<'a> {
    fn num(&self, node: NodeId, name: &str, base: f64) -> f64 {
        prop(self.doc, node, name)
            .and_then(|v| geom::length(&v, base))
            .unwrap_or(0.0)
    }

    /// Adds the element's geometry to `f`; `false` for an element that has none.
    fn geometry(&self, node: NodeId, tag: &str, f: &mut Flattener) -> bool {
        let (vw, vh) = self.viewport;
        let diag = ((vw * vw + vh * vh) / 2.0).sqrt();
        match tag {
            "path" => {
                if let Some(d) = self.doc.attr(node, "d") {
                    geom::path_data(d, f);
                }
            }
            "rect" => {
                let (x, y) = (self.num(node, "x", vw), self.num(node, "y", vh));
                let (w, h) = (self.num(node, "width", vw), self.num(node, "height", vh));
                let rx = prop(self.doc, node, "rx").and_then(|v| geom::length(&v, vw));
                let ry = prop(self.doc, node, "ry").and_then(|v| geom::length(&v, vh));
                let (rx, ry) = match (rx, ry) {
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) => (a, a),
                    (None, Some(b)) => (b, b),
                    (None, None) => (0.0, 0.0),
                };
                if w > 0.0 && h > 0.0 {
                    geom::rect(f, x, y, w, h, rx, ry);
                }
            }
            "circle" => {
                let r = self.num(node, "r", diag);
                if r > 0.0 {
                    geom::ellipse(f, self.num(node, "cx", vw), self.num(node, "cy", vh), r, r);
                }
            }
            "ellipse" => {
                let (rx, ry) = (self.num(node, "rx", vw), self.num(node, "ry", vh));
                if rx > 0.0 && ry > 0.0 {
                    geom::ellipse(
                        f,
                        self.num(node, "cx", vw),
                        self.num(node, "cy", vh),
                        rx,
                        ry,
                    );
                }
            }
            "line" => {
                f.move_to(Pt::new(self.num(node, "x1", vw), self.num(node, "y1", vh)));
                f.line_to(Pt::new(self.num(node, "x2", vw), self.num(node, "y2", vh)));
            }
            "polyline" | "polygon" => {
                let v = self
                    .doc
                    .attr(node, "points")
                    .map(geom::numbers)
                    .unwrap_or_default();
                for (i, p) in v.as_chunks::<2>().0.iter().enumerate() {
                    let pt = Pt::new(p[0], p[1]);
                    if i == 0 {
                        f.move_to(pt);
                    } else {
                        f.line_to(pt);
                    }
                }
                if tag == "polygon" && v.len() >= 4 {
                    f.close();
                }
            }
            _ => return false,
        }
        true
    }

    /// Returns the element's box (for its parent group's union).
    fn node(&mut self, node: NodeId, parent: &Ctx) -> Option<BoxF> {
        let NodeKind::Element { tag, ns, .. } = self.doc.kind(node) else {
            return None;
        };
        if *ns != Namespace::Svg {
            return None;
        }
        let display_none = self.styles.get(node).is_some_and(|s| s.display.is_none())
            || prop(self.doc, node, "display").as_deref() == Some("none");
        if display_none {
            return None;
        }
        let mut ctx = parent.clone();
        cascade(self.doc, node, &mut ctx);
        match tag.as_str() {
            "g" | "a" | "switch" => {
                let mut bbox: Option<BoxF> = None;
                let index = self.out.boxes.len();
                for c in self.doc.children(node) {
                    if let Some(b) = self.node(c, &ctx) {
                        bbox = Some(match bbox {
                            Some(a) => a.union(b),
                            None => b,
                        });
                    }
                }
                if let Some(b) = bbox {
                    self.out.boxes.insert(index, (node, b));
                }
                bbox
            }
            "text" => self.text(node, &ctx),
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {
                let mut f = Flattener::new(ctx.m);
                let drawn = self.geometry(node, tag, &mut f);
                let polys = f.finish();
                if !drawn {
                    return None;
                }
                let bbox = BoxF::of_points(polys.iter().flat_map(|p| p.pts.iter()));
                if let Some(b) = bbox {
                    self.out.boxes.push((node, b));
                }
                if ctx.visible && ctx.opacity > 0.0 {
                    // The bounding box in user space, for objectBoundingBox paint.
                    let user = {
                        let mut uf = Flattener::new(Affine::IDENTITY);
                        self.geometry(node, tag, &mut uf);
                        let up = uf.finish();
                        BoxF::of_points(up.iter().flat_map(|p| p.pts.iter()))
                    };
                    // A line encloses nothing to fill.
                    let fill = if tag == "line" {
                        None
                    } else {
                        self.paint(node, &ctx.fill, &ctx, user)
                            .map(|p| (p, ctx.fill_rule, ctx.fill_opacity * ctx.opacity))
                    };
                    let scale = ctx.m.mean_scale();
                    let stroke = if ctx.stroke_width > 0.0 {
                        self.paint(node, &ctx.stroke, &ctx, user).map(|p| {
                            (
                                Stroke {
                                    width: ctx.stroke_width * scale,
                                    cap: ctx.cap,
                                    join: ctx.join,
                                    miter_limit: ctx.miter_limit,
                                    dashes: ctx.dashes.iter().map(|d| d * scale).collect(),
                                    dash_offset: ctx.dash_offset * scale,
                                },
                                p,
                                ctx.stroke_opacity * ctx.opacity,
                            )
                        })
                    } else {
                        None
                    };
                    // Open subpaths are filled as if closed.
                    let fill_polys: Vec<Poly> = polys
                        .iter()
                        .map(|p| Poly {
                            pts: p.pts.clone(),
                            closed: true,
                        })
                        .collect();
                    if fill.is_some() || stroke.is_some() {
                        if let Some(fl) = fill {
                            self.out.items.push(Item::Shape(Box::new(Shape {
                                polys: fill_polys,
                                fill: Some(fl),
                                stroke: None,
                            })));
                        }
                        if let Some(st) = stroke {
                            self.out.items.push(Item::Shape(Box::new(Shape {
                                polys,
                                fill: None,
                                stroke: Some(st),
                            })));
                        }
                    }
                }
                bbox
            }
            // Paint servers, definitions and metadata draw nothing and have no box.
            _ => None,
        }
    }

    /// Resolves a paint for a shape whose user-space bounding box is `user`.
    fn paint(
        &self,
        node: NodeId,
        spec: &PaintSpec,
        ctx: &Ctx,
        user: Option<BoxF>,
    ) -> Option<Paint> {
        match spec {
            PaintSpec::None => None,
            PaintSpec::Color(c) => Some(Paint::Solid(*c)),
            PaintSpec::Current => Some(Paint::Solid(current_color(self.styles, node))),
            PaintSpec::Url(id, fallback) => self.gradient(id, ctx, user).or_else(|| {
                fallback
                    .as_deref()
                    .and_then(|f| self.paint(node, f, ctx, user))
            }),
        }
    }

    fn gradient(&self, id: &str, ctx: &Ctx, user: Option<BoxF>) -> Option<Paint> {
        let g = *self.doc.by_id(id).first()?;
        let tag = self.doc.tag(g)?;
        if !matches!(tag, "linearGradient" | "radialGradient") {
            return None;
        }
        // Stops, possibly inherited through `href`.
        let mut src = g;
        let mut stops = Vec::new();
        for _ in 0..8 {
            for c in self.doc.children(src) {
                if self.doc.tag(c) == Some("stop") {
                    let off = prop(self.doc, c, "offset")
                        .and_then(|v| opacity_value(&v))
                        .unwrap_or(0.0);
                    let col = prop(self.doc, c, "stop-color")
                        .and_then(|v| {
                            if v.eq_ignore_ascii_case("currentcolor") {
                                Some(current_color(self.styles, c))
                            } else {
                                parse_css_color(&v)
                            }
                        })
                        .unwrap_or(Color(0, 0, 0, 255));
                    let op = prop(self.doc, c, "stop-opacity")
                        .and_then(|v| opacity_value(&v))
                        .unwrap_or(1.0);
                    let last = stops.last().map(|(o, _)| *o).unwrap_or(0.0f64);
                    stops.push((
                        off.max(last),
                        [
                            col.0 as f64 / 255.0,
                            col.1 as f64 / 255.0,
                            col.2 as f64 / 255.0,
                            col.3 as f64 / 255.0 * op,
                        ],
                    ));
                }
            }
            if !stops.is_empty() {
                break;
            }
            let href = self
                .doc
                .attr(src, "href")
                .or_else(|| self.doc.attr(src, "xlink:href"))?;
            src = *self.doc.by_id(href.strip_prefix('#')?).first()?;
        }
        if stops.is_empty() {
            return None;
        }
        let user_units = self.doc.attr(g, "gradientUnits") == Some("userSpaceOnUse");
        let unit = |name: &str, default: f64| -> f64 {
            match self.doc.attr(g, name) {
                Some(v) => {
                    let v = v.trim();
                    if let Some(p) = v.strip_suffix('%') {
                        geom::number(p).map(|(n, _)| n / 100.0).unwrap_or(default)
                    } else {
                        geom::number(v).map(|(n, _)| n).unwrap_or(default)
                    }
                }
                None => default,
            }
        };
        // Gradient space → user space → device.
        let to_user = if user_units {
            Affine::IDENTITY
        } else {
            let b = user?;
            if b.w <= 0.0 || b.h <= 0.0 {
                return None;
            }
            Affine::translate(b.x, b.y).then(&Affine::scale(b.w, b.h))
        };
        let gt = self
            .doc
            .attr(g, "gradientTransform")
            .map(geom::parse_transform)
            .unwrap_or(Affine::IDENTITY);
        let inverse = ctx.m.then(&to_user).then(&gt).invert()?;
        Some(if tag == "linearGradient" {
            Paint::Linear {
                inverse,
                from: Pt::new(unit("x1", 0.0), unit("y1", 0.0)),
                to: Pt::new(unit("x2", 1.0), unit("y2", 0.0)),
                stops,
            }
        } else {
            let cx = unit("cx", 0.5);
            let cy = unit("cy", 0.5);
            Paint::Radial {
                inverse,
                centre: Pt::new(cx, cy),
                radius: unit("r", 0.5),
                stops,
            }
        })
    }

    /// A `<text>` element: its character data as one run at `x`, `y`.
    fn text(&mut self, node: NodeId, ctx: &Ctx) -> Option<BoxF> {
        let style = self.styles.get(node)?;
        let (vw, vh) = self.viewport;
        // The first of each position list; percentages of the viewport.
        let first = |name: &str, base: f64| {
            prop(self.doc, node, name)
                .and_then(|v| {
                    v.split([' ', ','])
                        .find(|s| !s.is_empty())
                        .map(str::to_owned)
                })
                .and_then(|v| geom::length(&v, base))
                .unwrap_or(0.0)
        };
        let (x, y) = (first("x", vw), first("y", vh));
        let (dx, dy) = (first("dx", vw), first("dy", vh));
        // Character data of the text node children (tspans flattened in order),
        // whitespace collapsed as `xml:space="default"` does.
        let mut pieces: Vec<(NodeId, String)> = Vec::new();
        fn gather(doc: &Document, n: NodeId, out: &mut Vec<(NodeId, String)>) {
            for c in doc.children(n) {
                match doc.kind(c) {
                    NodeKind::Text(t) => out.push((c, t.clone())),
                    NodeKind::Element { .. } => gather(doc, c, out),
                    _ => {}
                }
            }
        }
        gather(self.doc, node, &mut pieces);
        let mut collapsed: Vec<(NodeId, String)> = Vec::new();
        let mut prev_space = true;
        for (n, t) in pieces {
            let mut s = String::new();
            for ch in t.chars() {
                let ch = if matches!(ch, '\n' | '\t' | '\r') {
                    ' '
                } else {
                    ch
                };
                if ch == ' ' {
                    if prev_space {
                        continue;
                    }
                    prev_space = true;
                } else {
                    prev_space = false;
                }
                s.push(ch);
            }
            collapsed.push((n, s));
        }
        if let Some(last) = collapsed.iter_mut().rev().find(|(_, s)| !s.is_empty()) {
            while last.1.ends_with(' ') {
                last.1.pop();
            }
        }
        let scale = ctx.m.mean_scale();
        let mut font = style.font.clone();
        font.size = crate::geom::Au((font.size.0 as f64 * scale).round() as i32);
        let measure = |t: &str| {
            crate::layout::text::measure(&font, t, style.letter_spacing, style.word_spacing).0
                as f64
                / 64.0
        };
        let widths: Vec<f64> = collapsed.iter().map(|(_, t)| measure(t)).collect();
        let total: f64 = widths.iter().sum();
        let origin = ctx.m.apply(Pt::new(x + dx, y + dy));
        let left = match ctx.anchor {
            Anchor::Start => origin.x,
            Anchor::Middle => origin.x - total / 2.0,
            Anchor::End => origin.x - total,
        };
        let fm = crate::layout::text::font_metrics(&font);
        let ascent = (fm.ascent.0 as f64 / 64.0).round();
        let descent = (fm.descent.0 as f64 / 64.0).round();
        let top = origin.y - ascent;
        let mut cx = left;
        for ((n, _), w) in collapsed.iter().zip(&widths) {
            self.out.text_boxes.push((
                *n,
                BoxF {
                    x: cx,
                    y: top,
                    w: *w,
                    h: ascent + descent,
                },
            ));
            cx += w;
        }
        let text: String = collapsed.iter().map(|(_, t)| t.as_str()).collect();
        let bbox = BoxF {
            x: left,
            y: top,
            w: total,
            h: ascent + descent,
        };
        self.out.boxes.push((node, bbox));
        if ctx.visible && !text.is_empty() {
            let color = match self.paint(node, &ctx.fill, ctx, Some(bbox)) {
                Some(Paint::Solid(c)) => Some(c),
                Some(_) => Some(style.color),
                None => None,
            };
            if let Some(c) = color {
                let a = (c.3 as f64 * ctx.fill_opacity * ctx.opacity).round() as u8;
                self.out.items.push(Item::Text(TextRun {
                    element: node,
                    text,
                    x: left,
                    baseline: origin.y,
                    font,
                    color: Color(c.0, c.1, c.2, a),
                }));
            }
        }
        Some(bbox)
    }
}

/// Rasterises consecutive shapes of `built.items` into `w × h` straight-alpha RGBA
/// layers, interleaved with the text runs in paint order. A layer with nothing
/// drawn is left out.
pub fn layers(built: &Built, w: usize, h: usize) -> Vec<Layer> {
    let mut out = Vec::new();
    let mut canvas: Option<raster::Canvas> = None;
    let flush = |canvas: &mut Option<raster::Canvas>, out: &mut Vec<Layer>| {
        if let Some(c) = canvas.take() {
            if !c.is_blank() {
                out.push(Layer::Raster(c.into_rgba()));
            }
        }
    };
    for item in &built.items {
        match item {
            Item::Shape(s) => {
                let c = canvas.get_or_insert_with(|| raster::Canvas::new(w, h));
                if let Some((paint, rule, op)) = &s.fill {
                    c.fill(&s.polys, *rule, paint, *op);
                }
                if let Some((stroke, paint, op)) = &s.stroke {
                    c.stroke(&s.polys, stroke, paint, *op);
                }
            }
            Item::Text(t) => {
                flush(&mut canvas, &mut out);
                out.push(Layer::Text(t.clone()));
            }
        }
    }
    flush(&mut canvas, &mut out);
    out
}

pub enum Layer {
    Raster(Vec<u8>),
    Text(TextRun),
}
