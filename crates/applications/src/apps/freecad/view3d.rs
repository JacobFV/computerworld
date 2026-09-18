//! The 3D view: navigation with the pointer and wheel, picking, the navigation cube and
//! axis cross, and drawing — a z-buffered raster of the solids with vector overlays for
//! sketches being edited.
use super::layout::{Layout, CUBE};
use super::sketcher::{to_screen, Tool};
use super::*;
use crate::desktop_scene::shared::Align;
use crate::PointerPhase;
use cw_cad::document::Shape;
use cw_cad::math::{self, fmt_num, v2};
use cw_cad::sketch::{ConstraintType as T, Geom, H_AXIS, V_AXIS};
use cw_cad::view::{self, Drawable, Pick, Polyline, Rgb, Style};
use cw_scene::{Color, Primitive, Rect};
use std::hash::{Hash, Hasher};

// FreeCAD Light's colours (from its preference pack).
pub const SHAPE: Rgb = Rgb(173, 181, 189);
pub const LINE: Rgb = Rgb(25, 25, 25);
pub const PRESELECT: Rgb = Rgb(120, 190, 240);
pub const SELECT: Rgb = Rgb(66, 104, 133);
pub const MESH: Rgb = Rgb(173, 181, 189);
const SK_EDGE: Color = Color::rgb(73, 80, 87);
const SK_VERTEX: Color = Color::rgb(33, 37, 41);
const SK_FULL: Color = Color::rgb(47, 158, 68);
const SK_CONSTRUCTION: Color = Color::rgb(59, 91, 219);
const SK_CONSTRAINT: Color = Color::rgb(224, 49, 49);
const SK_REFERENCE: Color = Color::rgb(134, 142, 150);
const SK_SELECT: Color = Color::rgb(28, 173, 28);
const SK_PRESELECT: Color = Color::rgb(225, 225, 20);
const SK_CREATE: Color = Color::rgb(33, 37, 41);
const CUBE_BASE: Color = Color::rgb(208, 235, 255);
const CUBE_HILITE: Color = Color::rgb(170, 226, 255);
const INK: Color = Color::rgb(33, 37, 41);

/// Parse the painted size out of a view target (`view:<w>:<h>`).
pub fn view_size_of(target: &str) -> Option<(u32, u32)> {
    let rest = target.strip_prefix("freecad:view")?.strip_prefix(':')?;
    let (w, h) = rest.split_once(':')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

impl Cad {
    /// The shapes the 3D view shows and picks, with the object each belongs to.
    pub(crate) fn pickables(&self) -> Vec<(String, Arc<Shape>)> {
        let model = self.model();
        let mut out = Vec::new();
        for o in &self.doc.objects {
            if !o.visible {
                continue;
            }
            let shape = match &o.feature {
                Feature::Body { .. } => model.body_shape.get(&o.name),
                Feature::Mesh { .. } => model.shapes.get(&o.name),
                _ => None,
            };
            if let Some(s) = shape {
                // A body's shape is picked as the feature it came from (its tip), so
                // selection names match FreeCAD's `Body.Pad.Face6`.
                let owner = match &o.feature {
                    Feature::Body { .. } => {
                        self.shape_owner(&o.name).unwrap_or_else(|| o.name.clone())
                    }
                    _ => o.name.clone(),
                };
                out.push((owner, s.clone()));
            }
        }
        out
    }
    /// The feature whose result a body shows.
    pub(crate) fn shape_owner(&self, body: &str) -> Option<String> {
        let model = self.model();
        let Some(Feature::Body { group, tip }) = self.doc.get(body).map(|o| &o.feature) else {
            return None;
        };
        let end = tip
            .as_ref()
            .and_then(|t| group.iter().position(|g| g == t))
            .map(|p| p + 1)
            .unwrap_or(group.len());
        group[..end]
            .iter()
            .rev()
            .find(|g| model.shapes.contains_key(*g))
            .cloned()
    }

    fn pick_at(&self, x: f64, y: f64) -> Option<(String, Pick)> {
        let shapes = self.pickables();
        let refs: Vec<(&cw_cad::mesh::Mesh, &cw_cad::solid::Topology)> =
            shapes.iter().map(|(_, s)| (&s.mesh, &s.topo)).collect();
        let (w, h) = self.view_px();
        let p = view::pick(&self.camera, w, h, &refs, x, y, 6.0)?;
        Some((shapes[p.shape].0.clone(), p))
    }

    pub(crate) fn pointer(
        &mut self,
        window: u64,
        phase: PointerPhase,
        x: i32,
        y: i32,
    ) -> Result<Vec<AppEffect>, String> {
        match phase {
            PointerPhase::Down => {
                let grab = if self.button == 0 {
                    self.sketch_grab(f64::from(x), f64::from(y))
                } else {
                    None
                };
                self.press = Some(Press {
                    x,
                    y,
                    last: (x, y),
                    button: self.button,
                    moved: false,
                    grab,
                    on_cube: false,
                });
                self.menu = None;
                Ok(vec![])
            }
            PointerPhase::Move => {
                let Some(mut press) = self.press.clone() else {
                    return Ok(vec![]);
                };
                let (dx0, dy0) = (x - press.x, y - press.y);
                if !press.moved && dx0.abs() < 3 && dy0.abs() < 3 {
                    return Ok(vec![]);
                }
                if !press.moved {
                    if press.grab.is_some() {
                        self.checkpoint("Move");
                    }
                    press.moved = true;
                }
                let (dx, dy) = (f64::from(x - press.last.0), f64::from(y - press.last.1));
                press.last = (x, y);
                if let Some(grab) = press.grab {
                    self.sketch_drag(grab, f64::from(x), f64::from(y));
                } else {
                    let (_, h) = self.view_px();
                    let pan = match self.nav {
                        NavStyle::Gesture => press.button != 0,
                        NavStyle::OpenInventor => press.button == 1,
                    };
                    if pan {
                        self.camera.pan(dx, dy, h);
                    } else if press.button == 0 || self.nav == NavStyle::Gesture {
                        let k = math::PI / f64::from(h.max(1));
                        self.camera.orbit(dx * k, dy * k);
                    }
                }
                self.press = Some(press);
                Ok(vec![])
            }
            PointerPhase::Up => {
                let Some(press) = self.press.take() else {
                    return Ok(vec![]);
                };
                self.button = 0;
                if press.moved {
                    if press.grab.is_some() {
                        self.solve_after_drag();
                    }
                    return Ok(vec![]);
                }
                if press.button != 0 {
                    return Ok(vec![]);
                }
                self.view_click(window, (x, y))
            }
            PointerPhase::Cancel => {
                if let Some(press) = self.press.take() {
                    if press.moved && press.grab.is_some() {
                        let _ = self.undo();
                        self.redo.pop();
                    }
                }
                self.button = 0;
                Ok(vec![])
            }
        }
    }

    fn solve_after_drag(&mut self) {
        self.refresh_sketch_report();
        self.modified = true;
        let msg = self
            .sketch_edit()
            .map(|e| e.report.message())
            .unwrap_or_default();
        self.status = msg;
    }

    pub(crate) fn wheel(&mut self, x: i32, y: i32, delta: i32) {
        let (w, h) = self.view_px();
        // FreeCAD inverts the wheel by default: rolling towards you zooms out.
        let steps = f64::from(delta.clamp(-2400, 2400)) / 120.0;
        let mut factor = 1.0;
        let step = if steps > 0.0 { 1.15 } else { 1.0 / 1.15 };
        for _ in 0..(steps.abs().ceil() as i32).max(1) {
            factor *= step;
        }
        self.camera
            .zoom_at(factor, f64::from(x), f64::from(y), w, h);
    }

    /// A click (press and release in place) on the 3D view.
    pub(crate) fn view_click(
        &mut self,
        _window: u64,
        (x, y): (i32, i32),
    ) -> Result<Vec<AppEffect>, String> {
        if self.sketch_edit().is_some() {
            return self.sketch_click(f64::from(x), f64::from(y));
        }
        let hit = self.pick_at(f64::from(x), f64::from(y));
        let Some((object, pick)) = hit else {
            if !matches!(self.task, Some(Task::Measure)) {
                self.selection.clear();
            }
            self.status = String::new();
            return Ok(vec![]);
        };
        if self.dressup_pick(&object, pick.element)? {
            return Ok(vec![]);
        }
        let sel = Sel {
            object: object.clone(),
            sub: pick.element.name(),
            point: pick.point,
        };
        if matches!(self.task, Some(Task::Measure)) {
            // The Measure tool collects up to two picks.
            if self
                .selection
                .iter()
                .any(|s| s.object == sel.object && s.sub == sel.sub)
            {
                self.selection
                    .retain(|s| !(s.object == sel.object && s.sub == sel.sub));
            } else {
                if self.selection.len() >= 2 {
                    self.selection.clear();
                }
                self.selection.push(sel);
            }
        } else {
            self.selection = vec![sel];
        }
        self.status = format!(
            "Selected {}.{} ({}, {}, {})",
            self.label_of(&object),
            pick.element.name(),
            fmt_num(pick.point.x, 2),
            fmt_num(pick.point.y, 2),
            fmt_num(pick.point.z, 2)
        );
        Ok(vec![])
    }

    pub(crate) fn navcube_click(&mut self, (dx, dy): (i32, i32)) -> Result<Vec<AppEffect>, String> {
        let facets = view::nav_cube();
        let projected = view::nav_cube_projected(&self.camera, f64::from(CUBE));
        let p = v2(f64::from(dx), f64::from(dy));
        let hit = projected
            .iter()
            .rev()
            .find(|(_, pts, _)| cw_cad::sketch::profile::point_in_polygon(p, pts))
            .map(|(i, _, _)| *i)
            .ok_or("Click on the navigation cube")?;
        let f = &facets[hit];
        let std = match f.name.as_str() {
            "front" => Some(StdView::Front),
            "rear" => Some(StdView::Rear),
            "right" => Some(StdView::Right),
            "left" => Some(StdView::Left),
            "top" => Some(StdView::Top),
            "bottom" => Some(StdView::Bottom),
            _ => None,
        };
        match std {
            Some(v) => self.set_view(v),
            None => {
                let up = if f.dir.z.abs() > 0.99 { V3::Y } else { V3::Z };
                self.camera.look_from(f.dir, up);
            }
        }
        self.status = format!(
            "View: {}",
            f.label
                .map(str::to_owned)
                .unwrap_or_else(|| f.name.replace(':', " "))
        );
        Ok(vec![])
    }

    pub(crate) fn navcube_arrow(&mut self, dir: &str) -> Result<Vec<AppEffect>, String> {
        let step = math::radians(15.0);
        match dir {
            "left" => self.camera.orbit(-step, 0.0),
            "right" => self.camera.orbit(step, 0.0),
            "up" => self.camera.orbit(0.0, -step),
            "down" => self.camera.orbit(0.0, step),
            "ccw" => self.camera.roll(step),
            "cw" => self.camera.roll(-step),
            // The small round button: turn to look from behind.
            "reverse" => self.camera.orbit(math::PI, 0.0),
            _ => return Err("unknown navigation arrow".into()),
        }
        Ok(vec![])
    }

    fn visible_bounds(&self) -> Option<(V3, V3)> {
        let mut lo = V3 {
            x: f64::INFINITY,
            y: f64::INFINITY,
            z: f64::INFINITY,
        };
        let mut hi = V3 {
            x: f64::NEG_INFINITY,
            y: f64::NEG_INFINITY,
            z: f64::NEG_INFINITY,
        };
        for (_, s) in self.pickables() {
            if let Some(b) = s.mesh.bounds() {
                lo = lo.min(b.min);
                hi = hi.max(b.max);
            }
        }
        let model = self.model();
        for o in &self.doc.objects {
            if let Feature::Sketch { sketch, .. } = &o.feature {
                let editing = self.sketch_edit().is_some_and(|e| e.name == o.name);
                if !(o.visible || editing) {
                    continue;
                }
                if let (Some((a, b)), Some(f)) = (sketch.bounds(), model.frames.get(&o.name)) {
                    for p in [a, b, v2(a.x, b.y), v2(b.x, a.y)] {
                        let q = f.to_world(p);
                        lo = lo.min(q);
                        hi = hi.max(q);
                    }
                }
            }
        }
        lo.x.is_finite().then_some((lo, hi))
    }
    pub(crate) fn fit_all(&mut self) {
        let (w, h) = self.view_px();
        if let Some((lo, hi)) = self.visible_bounds() {
            self.camera.fit(lo, hi, w, h);
        } else {
            self.camera.target = V3::ZERO;
            self.camera.half_height = 60.0;
        }
    }
    pub(crate) fn fit_selection(&mut self) {
        let (w, h) = self.view_px();
        let model = self.model();
        let mut lo = V3 {
            x: f64::INFINITY,
            y: f64::INFINITY,
            z: f64::INFINITY,
        };
        let mut hi = V3 {
            x: f64::NEG_INFINITY,
            y: f64::NEG_INFINITY,
            z: f64::NEG_INFINITY,
        };
        for s in &self.selection {
            let shape = match self.doc.get(&s.object).map(|o| &o.feature) {
                Some(Feature::Body { .. }) => model.body_shape.get(&s.object),
                _ => model.shapes.get(&s.object),
            };
            if let Some(b) = shape.and_then(|sh| sh.mesh.bounds()) {
                lo = lo.min(b.min);
                hi = hi.max(b.max);
            }
        }
        if lo.x.is_finite() {
            self.camera.fit(lo, hi, w, h);
        }
    }
}

/// What the pointer is over in the view, for preselection and the status bar.
pub fn preselection(cad: &Cad, pointer: Option<(i32, i32)>, l: &Layout) -> Option<(String, Pick)> {
    let (px, py) = pointer?;
    let v = l.view;
    if !v.contains(px, py)
        || cad.sketch_edit().is_some()
        || cad.press.as_ref().is_some_and(|p| p.moved)
    {
        return None;
    }
    let (x, y) = (px - v.x, py - v.y);
    if l.cube().contains(x, y) {
        return None;
    }
    let shapes = cad.pickables();
    let refs: Vec<(&cw_cad::mesh::Mesh, &cw_cad::solid::Topology)> =
        shapes.iter().map(|(_, s)| (&s.mesh, &s.topo)).collect();
    let p = view::pick(
        &cad.camera,
        v.width,
        v.height,
        &refs,
        f64::from(x),
        f64::from(y),
        6.0,
    )?;
    Some((shapes[p.shape].0.clone(), p))
}

/// Colours by face, edge and vertex index.
type ElementColors = Vec<(usize, Rgb)>;

fn element_colors(
    sel: &[&Sel],
    pre: Option<&Pick>,
) -> (ElementColors, ElementColors, ElementColors) {
    let (mut f, mut e, mut v) = (vec![], vec![], vec![]);
    if let Some(p) = pre {
        match p.element {
            Element::Face(i) => f.push((i, PRESELECT)),
            Element::Edge(i) => e.push((i, PRESELECT)),
            Element::Vertex(i) => v.push((i, PRESELECT)),
        }
    }
    for s in sel {
        match Cad::element_of(&s.sub) {
            Some(Element::Face(i)) => f.push((i, SELECT)),
            Some(Element::Edge(i)) => e.push((i, SELECT)),
            Some(Element::Vertex(i)) => v.push((i, SELECT)),
            None => {}
        }
    }
    (f, e, v)
}

fn hash_of(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Draw the 3D view into `l.view`.
pub fn draw(cad: &Cad, p: &mut Painter, l: &Layout, pointer: Option<(i32, i32)>) {
    let v = l.view;
    let (w, h) = (v.width.max(1), v.height.max(1));
    let model = cad.model();
    let pre = preselection(cad, pointer, l);
    let editing = cad.sketch_edit().map(|e| e.name.clone());
    // Shapes.
    let shapes = cad.pickables();
    let mut drawables = Vec::new();
    for (owner, shape) in &shapes {
        let object = cad
            .doc
            .body_of(owner)
            .map(str::to_owned)
            .unwrap_or_else(|| owner.clone());
        let vp = cad.view_props(&object);
        let whole = cad
            .selection
            .iter()
            .any(|s| (s.object == *owner || s.object == object) && s.sub.is_empty());
        let sel: Vec<&Sel> = cad
            .selection
            .iter()
            .filter(|s| s.object == *owner)
            .collect();
        let pre_here = pre.as_ref().filter(|(o, _)| o == owner).map(|(_, p)| p);
        let (mut fc, ec, vc) = element_colors(&sel, pre_here);
        if whole {
            // The whole object in the selection colour, with preselection still showing
            // over it (later entries win).
            let mut all: Vec<(usize, Rgb)> =
                (0..shape.topo.faces.len()).map(|i| (i, SELECT)).collect();
            all.extend(fc.iter().filter(|(_, c)| *c == PRESELECT).copied());
            fc = all;
        }
        let is_mesh = matches!(
            cad.doc.get(owner).map(|o| &o.feature),
            Some(Feature::Mesh { .. })
        );
        let mut color = if is_mesh { MESH } else { SHAPE };
        if editing.is_some() {
            // The body recedes while its sketch is edited.
            color = Rgb(214, 218, 222);
        }
        drawables.push((shape.clone(), color, fc, ec, vc, vp));
    }
    let mut lines: Vec<Polyline> = Vec::new();
    // Sketches shown in 3D (not being edited).
    for o in &cad.doc.objects {
        let Feature::Sketch { sketch, .. } = &o.feature else {
            continue;
        };
        if !o.visible || editing.as_deref() == Some(o.name.as_str()) {
            continue;
        }
        let Some(frame) = model.frames.get(&o.name) else {
            continue;
        };
        let selected = cad.selection.iter().any(|s| s.object == o.name);
        for g in &sketch.geos {
            if g.construction {
                continue;
            }
            let pts = g
                .geom
                .polyline(math::radians(5.0))
                .iter()
                .map(|q| frame.to_world(*q))
                .collect();
            lines.push(Polyline {
                pts,
                color: if selected { SELECT } else { Rgb(33, 37, 41) },
                width: 2.0,
                on_top: false,
                dashed: false,
            });
        }
    }
    // Bounding box of the selection.
    if cad.bbox {
        for s in &cad.selection {
            let shape = match cad.doc.get(&s.object).map(|o| &o.feature) {
                Some(Feature::Body { .. }) => model.body_shape.get(&s.object),
                _ => model.shapes.get(&s.object),
            };
            if let Some(b) = shape.and_then(|sh| sh.mesh.bounds()) {
                let (a, c) = (b.min, b.max);
                let corner = |i: u8| V3 {
                    x: if i & 1 == 1 { c.x } else { a.x },
                    y: if i & 2 == 2 { c.y } else { a.y },
                    z: if i & 4 == 4 { c.z } else { a.z },
                };
                for (i, j) in [
                    (0, 1),
                    (1, 3),
                    (3, 2),
                    (2, 0),
                    (4, 5),
                    (5, 7),
                    (7, 6),
                    (6, 4),
                    (0, 4),
                    (1, 5),
                    (2, 6),
                    (3, 7),
                ] {
                    lines.push(Polyline {
                        pts: vec![corner(i), corner(j)],
                        color: Rgb(73, 80, 87),
                        width: 1.0,
                        on_top: true,
                        dashed: true,
                    });
                }
            }
        }
    }
    // Base planes while one is being chosen for a sketch.
    if let Some(Task::PickPlane { plane, .. }) = &cad.task {
        for (bp, _) in super::tasks::base_planes() {
            let f = bp.frame();
            let s = 40.0;
            let pts = [v2(-s, -s), v2(s, -s), v2(s, s), v2(-s, s), v2(-s, -s)]
                .iter()
                .map(|q| f.to_world(*q))
                .collect();
            lines.push(Polyline {
                pts,
                color: if bp.name() == plane {
                    SELECT
                } else {
                    Rgb(120, 130, 140)
                },
                width: if bp.name() == plane { 3.0 } else { 1.5 },
                on_top: true,
                dashed: false,
            });
        }
    }
    // Raster, cached on everything it depends on.
    let key = hash_of(&format!(
        "{}|{w}x{h}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}",
        cad.rev,
        cad.camera,
        cad.selection,
        pre.as_ref().map(|(o, p)| (o, p.element)),
        cad.bbox,
        cad.view_props,
        cad.task.as_ref().map(|t| match t {
            Task::PickPlane { plane, .. } => format!("plane:{plane}"),
            Task::Sketch(e) => format!("sketch:{}", e.name),
            _ => String::new(),
        }),
        cad.doc
            .objects
            .iter()
            .map(|o| o.visible)
            .collect::<Vec<_>>(),
        lines.len()
    ));
    let cached = cad.raster_cache().lock().ok().and_then(|g| {
        g.as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, r)| r.clone())
    });
    let rgba = match cached {
        Some(r) => r,
        None => {
            let ds: Vec<Drawable<'_>> = drawables
                .iter()
                .map(|(sh, color, fc, ec, vc, vp)| {
                    let wire = vp.display == DisplayMode::Wireframe;
                    Drawable {
                        mesh: &sh.mesh,
                        topo: &sh.topo,
                        color: *color,
                        line: LINE,
                        face_colors: fc.clone(),
                        edge_colors: ec.clone(),
                        vertex_colors: vc.clone(),
                        // Wireframe: edges only, the faces fully transparent.
                        transparency: if wire { 100 } else { vp.transparency },
                        show_edges: vp.display != DisplayMode::Shaded || wire,
                        line_width: vp.line_width,
                    }
                })
                .collect();
            let style = Style {
                background_top: Rgb(255, 255, 255),
                background_bottom: Rgb(255, 255, 255),
            };
            let r = Arc::new(view::render(&cad.camera, w, h, &ds, &lines, &style));
            if let Ok(mut g) = cad.raster_cache().lock() {
                *g = Some((key, r.clone()));
            }
            r
        }
    };
    p.node(
        v,
        Primitive::Image {
            width: w,
            height: h,
            rgba: (*rgba).clone(),
        },
        None,
    );
    // The whole view is a drag surface; its target carries the size it was painted at.
    p.region(v, &format!("freecad:view:{w}:{h}"), "3D view");
    if editing.is_some() {
        draw_sketch_overlay(cad, p, l, pointer);
    }
    draw_navcube(cad, p, l, pointer);
    if cad.axis_cross {
        draw_axis_cross(cad, p, l);
    }
}

fn draw_axis_cross(cad: &Cad, p: &mut Painter, l: &Layout) {
    let v = l.view;
    let (cx, cy) = (v.x + v.width as i32 - 50, v.y + v.height as i32 - 50);
    let len = 34.0;
    let cam = &cad.camera;
    let mut axes = [
        ("X", V3::X, Color::rgb(224, 49, 49)),
        ("Y", V3::Y, Color::rgb(47, 158, 68)),
        ("Z", V3::Z, Color::rgb(28, 126, 214)),
    ];
    // Farthest first, so the nearest axis draws on top.
    axes.sort_by(|a, b| a.1.dot(cam.back()).total_cmp(&b.1.dot(cam.back())));
    for (name, dir, color) in axes {
        let (sx, sy) = (dir.dot(cam.right) * len, -dir.dot(cam.up) * len);
        let end = (cx + sx.round() as i32, cy + sy.round() as i32);
        p.line(vec![(cx, cy), end], color, 2);
        p.label(
            end.0 + if sx >= 0.0 { 2 } else { -12 },
            end.1 - 8,
            14,
            name,
            11,
            color,
            true,
            Align::Left,
        );
    }
}

fn draw_navcube(cad: &Cad, p: &mut Painter, l: &Layout, pointer: Option<(i32, i32)>) {
    let v = l.view;
    let c = l.cube();
    let (ox, oy) = (v.x + c.x, v.y + c.y);
    let projected = view::nav_cube_projected(&cad.camera, f64::from(CUBE));
    let facets = view::nav_cube();
    let hover = pointer.map(|(x, y)| v2(f64::from(x - ox), f64::from(y - oy)));
    let hovered = hover.and_then(|q| {
        projected
            .iter()
            .rev()
            .find(|(_, pts, _)| cw_cad::sketch::profile::point_in_polygon(q, pts))
            .map(|(i, _, _)| *i)
    });
    for (i, pts, facing) in &projected {
        let shade = 0.78 + 0.22 * facing;
        let base = if hovered == Some(*i) {
            CUBE_HILITE
        } else {
            CUBE_BASE
        };
        let fill = Color(
            (f64::from(base.0) * shade) as u8,
            (f64::from(base.1) * shade) as u8,
            (f64::from(base.2) * shade) as u8,
            235,
        );
        let poly: Vec<(i32, i32)> = pts
            .iter()
            .map(|q| (ox + q.x.round() as i32, oy + q.y.round() as i32))
            .collect();
        p.path(poly.clone(), fill);
        let mut closed = poly;
        closed.push(closed[0]);
        p.line(closed, Color(60, 70, 80, 150), 1);
        if let (Some(label), true) = (facets[*i].label, *facing > 0.35) {
            let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, q| (a.0 + q.x, a.1 + q.y));
            let n = pts.len() as f64;
            let (mx, my) = (ox + (sx / n).round() as i32, oy + (sy / n).round() as i32);
            let size = if *facing > 0.8 { 11 } else { 9 };
            p.label(mx - 40, my - 8, 80, label, size, INK, true, Align::Center);
        }
    }
    // The cube responds where it is drawn; the arrows and buttons sit above it.
    p.region_above(
        Rect::new(ox + 14, oy + 14, CUBE - 28, CUBE - 28),
        "freecad:navcube",
        "Navigation cube",
    );
    let arrows = [
        (
            "up",
            Rect::new(ox + CUBE as i32 / 2 - 9, oy, 18, 12),
            vec![(0, 12), (9, 1), (18, 12)],
        ),
        (
            "down",
            Rect::new(ox + CUBE as i32 / 2 - 9, oy + CUBE as i32 - 12, 18, 12),
            vec![(0, 0), (9, 11), (18, 0)],
        ),
        (
            "left",
            Rect::new(ox, oy + CUBE as i32 / 2 - 9, 12, 18),
            vec![(12, 0), (1, 9), (12, 18)],
        ),
        (
            "right",
            Rect::new(ox + CUBE as i32 - 12, oy + CUBE as i32 / 2 - 9, 12, 18),
            vec![(0, 0), (11, 9), (0, 18)],
        ),
    ];
    for (dir, r, tri) in arrows {
        let over = pointer.is_some_and(|(x, y)| r.contains(x, y));
        let pts = tri.into_iter().map(|(x, y)| (r.x + x, r.y + y)).collect();
        p.path(
            pts,
            if over {
                Color::rgb(90, 150, 210)
            } else {
                Color(110, 120, 130, 200)
            },
        );
        p.region_above(
            r,
            &format!("freecad:navcube-arrow:{dir}"),
            &format!("Rotate view {dir}"),
        );
    }
    for (dir, x, label) in [
        ("ccw", ox + 2, "Rotate counterclockwise"),
        ("cw", ox + CUBE as i32 - 22, "Rotate clockwise"),
    ] {
        let r = Rect::new(x, oy + 2, 20, 20);
        let over = pointer.is_some_and(|(px, py)| r.contains(px, py));
        let pts = crate::desktop_scene::shared::arc_points(
            r.x + 10,
            r.y + 12,
            7,
            if dir == "cw" { -60 } else { -120 },
            if dir == "cw" { 60 } else { 0 },
            15,
        );
        p.line(
            pts,
            if over {
                Color::rgb(90, 150, 210)
            } else {
                Color(110, 120, 130, 200)
            },
            2,
        );
        p.region_above(r, &format!("freecad:navcube-arrow:{dir}"), label);
    }
    // The mini cube in the corner opens the view menu.
    let m = Rect::new(ox + CUBE as i32 - 22, oy + CUBE as i32 - 22, 18, 18);
    p.border(m, Color::rgb(230, 240, 250), 2, Color(60, 70, 80, 160));
    p.hline(m.x + 4, m.y + 7, 10, Color(60, 70, 80, 160));
    p.vline(m.x + 9, m.y + 7, 8, Color(60, 70, 80, 160));
    p.region_above(m, "freecad:navcube-menu", "Navigation cube menu");
}

/// Sketch geometry, constraints and dimensions, drawn crisply over the raster.
fn draw_sketch_overlay(cad: &Cad, p: &mut Painter, l: &Layout, pointer: Option<(i32, i32)>) {
    let Some(e) = cad.sketch_edit() else { return };
    let Some(s) = cad.sketch() else { return };
    let Some(frame) = cad.sketch_frame() else {
        return;
    };
    let v = l.view;
    let (w, h) = (v.width, v.height);
    let cam = &cad.camera;
    let scr = |q: V2| -> Option<(i32, i32)> {
        to_screen(cam, &frame, q, w, h)
            .map(|(x, y)| (v.x + x.round() as i32, v.y + y.round() as i32))
    };
    let clip = Rect::new(v.x, v.y, w, h);
    let mark = p.scene.nodes.len();
    // The sketch's axes: horizontal red, vertical green, through the origin.
    let span = cam.half_height * 4.0 + 1000.0;
    if let (Some(a), Some(b)) = (scr(v2(-span, 0.0)), scr(v2(span, 0.0))) {
        p.line(vec![a, b], Color(224, 49, 49, 150), 1);
    }
    if let (Some(a), Some(b)) = (scr(v2(0.0, -span)), scr(v2(0.0, span))) {
        p.line(vec![a, b], Color(47, 158, 68, 150), 1);
    }
    // Preselection under the pointer.
    let hover = pointer
        .filter(|(x, y)| v.contains(*x, *y))
        .and_then(|(x, y)| cad.sketch_point(f64::from(x - v.x), f64::from(y - v.y)));
    let pre = hover.and_then(|(_, snap)| snap);
    let full: BTreeSet<i32> = e.report.fully_constrained_geos.iter().copied().collect();
    let fixed: BTreeSet<(i32, Pos)> = e.report.fixed_points.iter().copied().collect();
    for (gi, g) in s.geos.iter().enumerate() {
        let gi = gi as i32;
        let picked = e.picked.contains(&(gi, Pos::None));
        let color = if picked {
            SK_SELECT
        } else if pre == Some((gi, Pos::None)) {
            SK_PRESELECT
        } else if g.construction {
            SK_CONSTRUCTION
        } else if full.contains(&gi) {
            SK_FULL
        } else {
            SK_EDGE
        };
        if !matches!(g.geom, Geom::Point { .. }) {
            let pts: Vec<(i32, i32)> = g
                .geom
                .polyline(math::radians(4.0))
                .into_iter()
                .filter_map(scr)
                .collect();
            if pts.len() >= 2 {
                p.line(pts, color, if g.construction { 1 } else { 2 });
            }
        }
        for pos in g.geom.points() {
            let Some(q) = g.geom.point(pos).and_then(scr) else {
                continue;
            };
            let picked = e.picked.contains(&(gi, pos));
            let c = if picked {
                SK_SELECT
            } else if pre == Some((gi, pos)) {
                SK_PRESELECT
            } else if fixed.contains(&(gi, pos)) {
                SK_FULL
            } else {
                SK_VERTEX
            };
            let size = if pos == Pos::Center { 5 } else { 6 };
            p.box_(
                Rect::new(q.0 - size / 2, q.1 - size / 2, size as u32, size as u32),
                c,
                0,
            );
        }
    }
    // The root point.
    if let Some(o) = scr(V2::ZERO) {
        let c = if e.picked.contains(&(H_AXIS, Pos::Start)) {
            SK_SELECT
        } else {
            SK_VERTEX
        };
        p.box_(Rect::new(o.0 - 3, o.1 - 3, 6, 6), c, 3);
    }
    // Constraints: icons for geometric ones, dimension lines for the rest.
    let mut icon_slots: std::collections::BTreeMap<(i32, i32), u32> = Default::default();
    let conflict: BTreeSet<usize> = e
        .report
        .conflict_groups
        .iter()
        .chain(&e.report.redundant)
        .map(|i| i - 1)
        .collect();
    for (ci, c) in s.constraints.iter().enumerate() {
        let selected = e.constraints.contains(&ci);
        let color = if selected {
            SK_SELECT
        } else if !c.driving {
            SK_REFERENCE
        } else if conflict.contains(&ci) {
            Color::rgb(253, 126, 20)
        } else {
            SK_CONSTRAINT
        };
        if c.kind.is_dimensional() {
            draw_dimension(p, s, c, ci, color, &scr);
            continue;
        }
        let glyph = match c.kind {
            T::Coincident => continue,
            T::PointOnObject => "•",
            T::Horizontal => "H",
            T::Vertical => "V",
            T::Parallel => "∥",
            T::Perpendicular => "⊥",
            T::Tangent => "T",
            T::Equal => "=",
            T::Symmetric => "⋈",
            T::Block => "B",
            _ => "?",
        };
        let anchor = match c.kind {
            T::PointOnObject | T::Symmetric => s.point(c.first, c.first_pos),
            T::Tangent if c.first_pos != Pos::None => s.point(c.first, c.first_pos),
            _ if c.first_pos != Pos::None => s.point(c.first, c.first_pos),
            _ => s.geom(c.first).map(|g| g.midpoint()),
        };
        let Some(at) = anchor.and_then(scr) else {
            continue;
        };
        let slot = icon_slots.entry((at.0 / 12, at.1 / 12)).or_insert(0);
        let r = Rect::new(at.0 + 6 + *slot as i32 * 15, at.1 + 6, 14, 14);
        *slot += 1;
        p.box_(r, if selected { SK_SELECT } else { color }, 2);
        p.label(r.x, r.y, 14, glyph, 10, Color::WHITE, true, Align::Center);
        p.region_above(
            r,
            &format!("freecad:sk:constraint:{ci}"),
            &format!("Constraint{} ({})", ci + 1, c.kind.label()),
        );
    }
    // What the active tool would make, following the pointer.
    if let (Some(tool), Some((at, snap))) = (e.tool, hover) {
        let clicks = &e.clicks;
        let seg = |p: &mut Painter, a: V2, b: V2| {
            if let (Some(x), Some(y)) = (scr(a), scr(b)) {
                p.line(vec![x, y], SK_CREATE, 1);
            }
        };
        match (tool, clicks.as_slice()) {
            (Tool::Line | Tool::Polyline, [.., last]) => seg(p, *last, at),
            (Tool::Rectangle, [a]) => {
                for (x, y) in [
                    (v2(a.x, a.y), v2(at.x, a.y)),
                    (v2(at.x, a.y), at),
                    (at, v2(a.x, at.y)),
                    (v2(a.x, at.y), *a),
                ] {
                    seg(p, x, y);
                }
            }
            (Tool::Circle, [c]) | (Tool::ArcCenter, [c]) => {
                let r = c.dist(at);
                let pts: Vec<(i32, i32)> = Geom::Circle {
                    c: *c,
                    r: r.max(1e-9),
                }
                .polyline(math::radians(5.0))
                .into_iter()
                .filter_map(scr)
                .collect();
                p.line(pts, SK_CREATE, 1);
            }
            (Tool::Slot, [a]) => seg(p, *a, at),
            _ => {}
        }
        if let Some(q) = scr(at) {
            let text = format!("({}, {})", fmt_num(at.x, 2), fmt_num(at.y, 2));
            p.label(
                q.0 + 14,
                q.1 + 10,
                160,
                &text,
                11,
                SK_CREATE,
                false,
                Align::Left,
            );
            if snap.is_some() {
                p.ring(q.0, q.1, 6, 1, SK_PRESELECT);
            }
        }
    }
    for n in &mut p.scene.nodes[mark..] {
        n.clip = Some(clip);
    }
}

/// A dimension: extension and dimension lines with the value, clickable.
fn draw_dimension(
    p: &mut Painter,
    s: &cw_cad::sketch::Sketch,
    c: &cw_cad::sketch::Constraint,
    ci: usize,
    color: Color,
    scr: &dyn Fn(V2) -> Option<(i32, i32)>,
) {
    let text = if c.driving {
        c.display_value()
    } else {
        format!("({})", c.display_value())
    };
    let (a, b) = match c.kind {
        T::Radius | T::Diameter => {
            let Some(g) = s.geom(c.first) else { return };
            let (ctr, r) = match g {
                Geom::Circle { c, r } => (c, r),
                Geom::Arc { c, r, .. } => (c, r),
                _ => return,
            };
            let dir = match g {
                Geom::Arc { start, end, .. } => V2::polar((start + end) / 2.0, 1.0),
                _ => V2::polar(math::PI / 4.0, 1.0),
            };
            if c.kind == T::Diameter {
                (ctr - dir * r, ctr + dir * r)
            } else {
                (ctr, ctr + dir * r)
            }
        }
        T::Angle => {
            let Some(Geom::Line { a, b }) = s.geom(c.first) else {
                if let Some(Geom::Arc {
                    c: ctr,
                    r,
                    start,
                    end,
                }) = s.geom(c.first)
                {
                    let m = ctr + V2::polar((start + end) / 2.0, r * 0.6);
                    label_at(p, scr(m), &text, color, ci);
                }
                return;
            };
            label_at(
                p,
                scr(a.lerp(b, 0.3) + (b - a).norm().perp() * 0.0),
                &text,
                color,
                ci,
            );
            return;
        }
        T::DistanceX | T::DistanceY | T::Distance => {
            let pts = if c.second != cw_cad::sketch::GEO_UNDEF && c.second_pos != Pos::None {
                (
                    s.point(c.first, c.first_pos),
                    s.point(c.second, c.second_pos),
                )
            } else if c.first_pos != Pos::None && c.second != cw_cad::sketch::GEO_UNDEF {
                // Point to line: to the foot of the perpendicular.
                let q = s.point(c.first, c.first_pos);
                let foot = match (q, s.geom(c.second)) {
                    (Some(q), Some(Geom::Line { a, b })) => {
                        let d = (b - a).norm();
                        Some(a + d * (q - a).dot(d))
                    }
                    _ => None,
                };
                (q, foot)
            } else if c.first_pos != Pos::None {
                (Some(V2::ZERO), s.point(c.first, c.first_pos))
            } else {
                (s.point(c.first, Pos::Start), s.point(c.first, Pos::End))
            };
            let (Some(a), Some(b)) = pts else { return };
            match c.kind {
                T::DistanceX => (a, v2(b.x, a.y)),
                T::DistanceY => (a, v2(a.x, b.y)),
                _ => (a, b),
            }
        }
        _ => return,
    };
    let (Some(sa), Some(sb)) = (scr(a), scr(b)) else {
        return;
    };
    // Offset the dimension line off the geometry, as FreeCAD does.
    let (dx, dy) = (f64::from(sb.0 - sa.0), f64::from(sb.1 - sa.1));
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let (nx, ny) = (-dy / len, dx / len);
    let off = if matches!(c.kind, T::Radius | T::Diameter) {
        0.0
    } else {
        18.0
    };
    let (oa, ob) = (
        (
            sa.0 + (nx * off).round() as i32,
            sa.1 + (ny * off).round() as i32,
        ),
        (
            sb.0 + (nx * off).round() as i32,
            sb.1 + (ny * off).round() as i32,
        ),
    );
    if off > 0.0 {
        p.line(vec![sa, oa], color, 1);
        p.line(vec![sb, ob], color, 1);
    }
    p.line(vec![oa, ob], color, 1);
    // Arrowheads.
    for (tip, dir) in [(ob, 1.0), (oa, -1.0)] {
        let (ux, uy) = (dx / len * dir, dy / len * dir);
        let (bx, by) = (f64::from(tip.0) - ux * 8.0, f64::from(tip.1) - uy * 8.0);
        p.path(
            vec![
                tip,
                (
                    (bx + uy * 3.0).round() as i32,
                    (by - ux * 3.0).round() as i32,
                ),
                (
                    (bx - uy * 3.0).round() as i32,
                    (by + ux * 3.0).round() as i32,
                ),
            ],
            color,
        );
    }
    label_at(
        p,
        Some(((oa.0 + ob.0) / 2, (oa.1 + ob.1) / 2)),
        &text,
        color,
        ci,
    );
}

fn label_at(p: &mut Painter, at: Option<(i32, i32)>, text: &str, color: Color, ci: usize) {
    let Some((x, y)) = at else { return };
    let w = p.measure(text, 12, false) + 8;
    let r = Rect::new(x - w as i32 / 2, y - 10, w, 18);
    p.box_(r, Color(255, 255, 255, 200), 2);
    p.label(r.x, r.y + 1, w, text, 12, color, false, Align::Center);
    p.region_above(
        r,
        &format!("freecad:sk:dim:{ci}"),
        &format!("Constraint{} = {text}", ci + 1),
    );
}

/// FreeCAD's status bar text for what is under the pointer.
pub fn preselection_text(cad: &Cad, pre: &Option<(String, Pick)>) -> Option<String> {
    let (object, pick) = pre.as_ref()?;
    let body = cad
        .doc
        .body_of(object)
        .map(|b| format!("{}.", cad.label_of(b)))
        .unwrap_or_default();
    Some(format!(
        "Preselected: {}.{}{}.{} ({} mm, {} mm, {} mm)",
        cad.doc.label,
        body,
        cad.label_of(object),
        pick.element.name(),
        fmt_num(pick.point.x, 2),
        fmt_num(pick.point.y, 2),
        fmt_num(pick.point.z, 2)
    ))
}

pub fn sketch_hover_text(cad: &Cad, pointer: Option<(i32, i32)>, l: &Layout) -> Option<String> {
    let (x, y) = pointer.filter(|(x, y)| l.view.contains(*x, *y))?;
    let (at, snap) = cad.sketch_point(f64::from(x - l.view.x), f64::from(y - l.view.y))?;
    let what = match snap {
        Some((H_AXIS, Pos::Start)) => " on RootPoint".to_owned(),
        Some((H_AXIS, _)) => " on H_Axis".to_owned(),
        Some((V_AXIS, _)) => " on V_Axis".to_owned(),
        Some((g, Pos::None)) => format!(" on Edge{}", g + 1),
        Some((g, pos)) => format!(" on Vertex{}", cad.vertex_number(g, pos).unwrap_or(0)),
        None => String::new(),
    };
    Some(format!(
        "({} mm, {} mm){what}",
        fmt_num(at.x, 2),
        fmt_num(at.y, 2)
    ))
}
