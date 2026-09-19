//! The 3D Viewer: the board as KiCad's viewer builds it — the substrate extruded from
//! the Edge.Cuts outline to its thickness with every drilled hole cut through it, solder
//! mask on both faces, copper pads, tracks, vias and zone fills, silkscreen, and each
//! footprint's body extruded from its fabrication outline to its height — rendered by
//! the z-buffer rasteriser of `cw_cad` into an image. The camera orbits, pans and zooms;
//! everything is computed with deterministic arithmetic, so a view renders the same
//! pixels every time.
use super::widgets::{self as w, chrome, MenuItem};
use super::{act, icons, Kicad};
use crate::desktop_scene::{shared::Align, Painter};
use crate::{AppEffect, PointerPhase};
use cw_cad::math::{self, v2, v3, V2, V3};
use cw_cad::mesh::Mesh;
use cw_cad::sketch::profile::{Region, Wire};
use cw_cad::sketch::Sketch;
use cw_cad::solid::{self, Topology};
use cw_cad::view::{self, Camera, Drawable, Rgb, Style};
use cw_cad::Frame;
use cw_eda::footprints::PadKind;
use cw_eda::geom::Pt;
use cw_eda::pcb::{Board, DrawShape, Layer, Shape};
use cw_scene::{Color, Primitive, Rect};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// KiCad's default board thickness, millimetres.
pub const BOARD_THICKNESS: f64 = 1.6;
const MASK: f64 = 0.01;
const COPPER: f64 = 0.035;
const SILK: f64 = 0.01;
/// Segments of a full circle (holes, round pads, track ends).
const CIRCLE: usize = 24;
const PANEL_W: u32 = 200;
/// Layers of the scene the Appearance panel turns on and off.
pub const PARTS: [(&str, &str); 5] = [
    ("board", "Board body"),
    ("copper", "Copper"),
    ("mask", "Solder mask"),
    ("silk", "Silkscreen"),
    ("bodies", "Component bodies"),
];
/// Standard views: name, yaw and pitch (tenths of a degree).
pub const VIEWS: [(&str, &str, i32, i32); 7] = [
    ("top", "Top", 0, 900),
    ("bottom", "Bottom", 1800, -900),
    ("front", "Front", 0, 0),
    ("back", "Back", 1800, 0),
    ("left", "Left", -900, 0),
    ("right", "Right", 900, 0),
    ("iso", "Isometric", 450, 350),
];

/// The last rendered view, keyed by a hash of everything it depends on. Never
/// serialised: the pixels are a pure function of the board and the camera.
#[derive(Default)]
pub struct RasterCache(Mutex<Option<(u64, Arc<Vec<u8>>)>>);
impl Clone for RasterCache {
    fn clone(&self) -> Self {
        RasterCache(Mutex::new(self.0.lock().map(|g| g.clone()).unwrap_or(None)))
    }
}
impl std::fmt::Debug for RasterCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RasterCache")
    }
}
impl PartialEq for RasterCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for RasterCache {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct View3dUi {
    /// Turn about the board's normal, tenths of a degree.
    pub yaw: i32,
    /// Elevation above the board plane, tenths of a degree: 900 looks straight down.
    pub pitch: i32,
    /// Half the view's height at the target, micrometres; 0 fits the board.
    pub zoom_um: i64,
    /// What the camera looks at, micrometres (board X, board Y up, Z up).
    pub target_um: (i64, i64, i64),
    /// Scene parts switched off in the Appearance panel.
    pub hidden: Vec<String>,
    /// A left drag pans instead of orbiting.
    pub pan_mode: bool,
    /// The pointer position of a drag in progress.
    pub drag: Option<(i32, i32)>,
    #[serde(skip)]
    pub cache: RasterCache,
}
impl Default for View3dUi {
    fn default() -> Self {
        // KiCad's viewer opens looking straight down on the top of the board.
        Self {
            yaw: 0,
            pitch: 900,
            zoom_um: 0,
            target_um: (0, 0, 0),
            hidden: vec![],
            pan_mode: false,
            drag: None,
            cache: RasterCache::default(),
        }
    }
}
impl View3dUi {
    fn shows(&self, part: &str) -> bool {
        !self.hidden.iter().any(|h| h == part)
    }
}

fn mm_pt(p: Pt) -> V2 {
    // Board Y grows down; the 3D scene's Y grows up.
    v2(p.x as f64 / 1e6, -(p.y as f64) / 1e6)
}
fn signed_area(pts: &[V2]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}
fn wire(mut pts: Vec<V2>, ccw: bool) -> Wire {
    if (signed_area(&pts) > 0.0) != ccw {
        pts.reverse();
    }
    let edge_geo = (0..pts.len() as i32).collect();
    Wire { pts, edge_geo }
}
fn circle(c: V2, r: f64, n: usize) -> Vec<V2> {
    (0..n)
        .map(|i| {
            let (s, co) = math::sin_cos(math::TAU * i as f64 / n as f64);
            v2(c.x + r * co, c.y + r * s)
        })
        .collect()
}
/// A stadium: the outline of a segment `a`–`b` stroked `r` wide on each side.
fn stadium(a: V2, b: V2, r: f64) -> Vec<V2> {
    let d = b - a;
    let len = math::sqrt(d.x * d.x + d.y * d.y);
    if len < 1e-9 {
        return circle(a, r, CIRCLE);
    }
    let base = math::atan2(d.y, d.x);
    let half = CIRCLE / 2;
    let mut pts = Vec::with_capacity(CIRCLE + 2);
    // Round the far end from one side to the other, then the near end.
    for i in 0..=half {
        let t = base - math::TAU / 4.0 + math::TAU / 2.0 * i as f64 / half as f64;
        let (s, c) = math::sin_cos(t);
        pts.push(v2(b.x + r * c, b.y + r * s));
    }
    for i in 0..=half {
        let t = base + math::TAU / 4.0 + math::TAU / 2.0 * i as f64 / half as f64;
        let (s, c) = math::sin_cos(t);
        pts.push(v2(a.x + r * c, a.y + r * s));
    }
    pts
}
fn point_in(poly: &[V2], p: V2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}
fn dist_to_poly(poly: &[V2], p: V2) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let d = b - a;
            let l2 = d.x * d.x + d.y * d.y;
            let t = if l2 > 0.0 {
                (((p - a).x * d.x + (p - a).y * d.y) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = a + d * t;
            math::hypot(p.x - q.x, p.y - q.y)
        })
        .fold(f64::INFINITY, f64::min)
}
/// Extrude an outline with holes from `z0` to `z1` and add it to `into`.
fn prism(into: &mut Mesh, outer: Vec<V2>, holes: Vec<Vec<V2>>, z0: f64, z1: f64) {
    if outer.len() < 3 || z1 <= z0 {
        return;
    }
    let region = Region {
        outer: wire(outer, true),
        holes: holes.into_iter().map(|h| wire(h, false)).collect(),
    };
    let m = solid::extrude(&Sketch::default(), &[region], &Frame::XY, z0, z1);
    into.append(&m);
}
/// A pad's outline in board millimetres.
fn pad_outline(shape: &Shape) -> Vec<V2> {
    match shape {
        Shape::Seg { a, b, r } => stadium(mm_pt(*a), mm_pt(*b), *r as f64 / 1e6),
        other => other
            .corners()
            .map(|c| c.iter().map(|p| mm_pt(*p)).collect())
            .unwrap_or_default(),
    }
}

/// The meshes of the scene, one per material.
pub struct Scene {
    pub board: Mesh,
    pub copper: Mesh,
    pub tracks: Mesh,
    pub silk: Mesh,
    pub bodies: Mesh,
    /// Holes cut through the substrate.
    pub holes: usize,
    /// The outline came from Edge.Cuts rather than around the footprints.
    pub outlined: bool,
    pub min: V3,
    pub max: V3,
}

/// Build the board's 3D scene.
pub fn scene(b: &Board) -> Scene {
    let t = BOARD_THICKNESS;
    let (outline, outlined) = match b.outline() {
        Some(o) if o.len() >= 3 => (o.iter().map(|p| mm_pt(*p)).collect::<Vec<V2>>(), true),
        _ => {
            // No Edge.Cuts: a board around the footprints, 2 mm clear of them.
            let mut lo = v2(f64::INFINITY, f64::INFINITY);
            let mut hi = v2(f64::NEG_INFINITY, f64::NEG_INFINITY);
            for f in &b.footprints {
                let r = f.bounds();
                for p in [mm_pt(r.min), mm_pt(r.max)] {
                    lo = v2(lo.x.min(p.x), lo.y.min(p.y));
                    hi = v2(hi.x.max(p.x), hi.y.max(p.y));
                }
            }
            if !lo.x.is_finite() {
                lo = v2(0.0, -50.0);
                hi = v2(80.0, 0.0);
            }
            (
                vec![
                    v2(lo.x - 2.0, lo.y - 2.0),
                    v2(hi.x + 2.0, lo.y - 2.0),
                    v2(hi.x + 2.0, hi.y + 2.0),
                    v2(lo.x - 2.0, hi.y + 2.0),
                ],
                false,
            )
        }
    };
    // Drilled holes: through-hole pads and vias. A hole that would leave the board or
    // run into another is left out of the cut (the checker reports those).
    let mut drills: Vec<(V2, f64)> = vec![];
    for f in &b.footprints {
        for pad in f.pads.iter().filter(|p| p.drill > 0) {
            drills.push((mm_pt(f.pad_pos(pad)), pad.drill as f64 / 2e6));
        }
    }
    for v in &b.vias {
        drills.push((mm_pt(v.pos), v.drill as f64 / 2e6));
    }
    let mut holes: Vec<(V2, f64)> = vec![];
    for (c, r) in drills {
        let inside = point_in(&outline, c) && dist_to_poly(&outline, c) > r + 0.05;
        let clear = holes
            .iter()
            .all(|(o, q)| math::hypot(o.x - c.x, o.y - c.y) > r + q + 0.05);
        if inside && clear && r > 0.0 {
            holes.push((c, r));
        }
    }
    let hole_polys = || holes.iter().map(|(c, r)| circle(*c, *r, CIRCLE)).collect();
    let mut board = Mesh::default();
    prism(&mut board, outline.clone(), hole_polys(), 0.0, t);
    // Copper exposed through the mask: pads (with their holes) and via rings.
    let mut copper = Mesh::default();
    let mut tracks = Mesh::default();
    let side_z = |front: bool, thick: f64| {
        if front {
            (t + MASK, t + MASK + thick)
        } else {
            (-MASK - thick, -MASK)
        }
    };
    for f in &b.footprints {
        for pad in &f.pads {
            let outer = pad_outline(&f.pad_shape(pad));
            let c = mm_pt(f.pad_pos(pad));
            let hole = holes
                .iter()
                .find(|(h, _)| math::hypot(h.x - c.x, h.y - c.y) < 1e-6)
                .map(|(h, r)| circle(*h, *r, CIRCLE));
            let sides: &[bool] = match pad.kind {
                PadKind::ThroughHole => &[true, false],
                PadKind::Smd => {
                    if f.back {
                        &[false]
                    } else {
                        &[true]
                    }
                }
            };
            for &front in sides {
                let (z0, z1) = side_z(front, COPPER);
                prism(
                    &mut copper,
                    outer.clone(),
                    hole.clone().into_iter().collect(),
                    z0,
                    z1,
                );
            }
        }
    }
    for v in &b.vias {
        let c = mm_pt(v.pos);
        let hole = holes
            .iter()
            .find(|(h, _)| math::hypot(h.x - c.x, h.y - c.y) < 1e-6)
            .map(|(h, r)| circle(*h, *r, CIRCLE));
        for front in [true, false] {
            let (z0, z1) = side_z(front, COPPER);
            prism(
                &mut copper,
                circle(c, v.diameter as f64 / 2e6, CIRCLE),
                hole.clone().into_iter().collect(),
                z0,
                z1,
            );
        }
    }
    // Tracks and zone fills lie under the mask: drawn in the mask's lighter green.
    for tr in &b.tracks {
        let front = tr.layer == Layer::FCu;
        let (z0, z1) = side_z(front, COPPER * 0.5);
        prism(
            &mut tracks,
            stadium(mm_pt(tr.a), mm_pt(tr.b), tr.width as f64 / 2e6),
            vec![],
            z0,
            z1,
        );
    }
    for z in b.zones.iter().filter(|z| !z.keepout) {
        let front = z.layer == Layer::FCu;
        let (z0, z1) = side_z(front, COPPER * 0.5);
        for r in &z.fill {
            let (a, c) = (mm_pt(r.min), mm_pt(r.max));
            prism(
                &mut tracks,
                vec![v2(a.x, a.y), v2(c.x, a.y), v2(c.x, c.y), v2(a.x, c.y)],
                vec![],
                z0,
                z1,
            );
        }
    }
    // Silkscreen strokes, 0.12 mm wide unless the line says otherwise.
    let mut silk = Mesh::default();
    for f in &b.footprints {
        for l in f
            .lines
            .iter()
            .filter(|l| matches!(l.layer, Layer::FSilkS | Layer::BSilkS))
        {
            let front = l.layer == Layer::FSilkS;
            let (z0, z1) = side_z(front, COPPER + SILK);
            let r = (l.width.max(120_000)) as f64 / 2e6;
            prism(
                &mut silk,
                stadium(mm_pt(f.to_board(l.a)), mm_pt(f.to_board(l.b)), r),
                vec![],
                z0,
                z1,
            );
        }
    }
    for d in b
        .drawings
        .iter()
        .filter(|d| matches!(d.layer, Layer::FSilkS | Layer::BSilkS))
    {
        let front = d.layer == Layer::FSilkS;
        let (z0, z1) = side_z(front, COPPER + SILK);
        let r = d.width.max(120_000) as f64 / 2e6;
        let segs = match d.shape {
            DrawShape::Line { a, b } => vec![(a, b)],
            DrawShape::Rect { a, b } => {
                let (c, e) = (Pt::new(b.x, a.y), Pt::new(a.x, b.y));
                vec![(a, c), (c, b), (b, e), (e, a)]
            }
        };
        for (a, c) in segs {
            prism(&mut silk, stadium(mm_pt(a), mm_pt(c), r), vec![], z0, z1);
        }
    }
    // Component bodies: the fabrication outline (or the pads' extent) raised to the
    // footprint's height, on the side the footprint is on.
    let mut bodies = Mesh::default();
    let mut top = t;
    for f in &b.footprints {
        let corners = f.body_poly().or_else(|| {
            let r = f.bounds();
            Some(f.corners(f.to_local(r.min), f.to_local(r.max)))
        });
        let Some(corners) = corners else { continue };
        let h = if f.height > 0 { f.height } else { 1_000_000 } as f64 / 1e6;
        let poly: Vec<V2> = corners.iter().map(|p| mm_pt(*p)).collect();
        let lift = MASK + COPPER + SILK;
        if f.back {
            prism(&mut bodies, poly, vec![], -lift - h, -lift);
        } else {
            prism(&mut bodies, poly, vec![], t + lift, t + lift + h);
            top = top.max(t + lift + h);
        }
    }
    let mut lo = v2(f64::INFINITY, f64::INFINITY);
    let mut hi = v2(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in &outline {
        lo = v2(lo.x.min(p.x), lo.y.min(p.y));
        hi = v2(hi.x.max(p.x), hi.y.max(p.y));
    }
    Scene {
        board,
        copper,
        tracks,
        silk,
        bodies,
        holes: holes.len(),
        outlined,
        min: v3(lo.x, lo.y, -1.0),
        max: v3(hi.x, hi.y, top),
    }
}

/// The camera of a view state in a `w`×`h` canvas.
pub fn camera(v: &View3dUi, s: &Scene, w: u32, h: u32) -> Camera {
    let rad = |tenths: i32| math::radians(tenths as f64 / 10.0);
    let (sy, cy) = math::sin_cos(rad(v.yaw));
    let (sp, cp) = math::sin_cos(rad(v.pitch.clamp(-900, 900)));
    let right = v3(cy, sy, 0.0);
    // Towards the viewer: the horizontal direction in front, tipped up by the pitch.
    let back = v3(sy * cp, -cy * cp, sp);
    let up = back.cross(right).norm();
    let mut cam = Camera {
        target: V3::ZERO,
        right,
        up,
        half_height: 50.0,
        perspective: false,
    };
    if v.zoom_um <= 0 {
        // Fit the board: its footprint on screen, whatever the turn.
        let c = (s.min + s.max) * 0.5;
        let size = s.max - s.min;
        let radius = (math::sqrt(size.x * size.x + size.y * size.y) / 2.0).max(1.0);
        let aspect = f64::from(w.max(1)) / f64::from(h.max(1));
        cam.target = c;
        cam.half_height = radius * 1.08 / aspect.min(1.0);
    } else {
        cam.target = v3(
            v.target_um.0 as f64 / 1000.0,
            v.target_um.1 as f64 / 1000.0,
            v.target_um.2 as f64 / 1000.0,
        );
        cam.half_height = v.zoom_um as f64 / 1000.0;
    }
    cam
}

/// Render the board in a view state: RGBA pixels, `w`×`h`.
pub fn raster(b: &Board, v: &View3dUi, w: u32, h: u32) -> Vec<u8> {
    let s = scene(b);
    let cam = camera(v, &s, w, h);
    let parts: Vec<(&Mesh, Rgb, bool, &str)> = vec![
        (&s.board, Rgb(20, 70, 40), false, "board"),
        (&s.copper, Rgb(214, 176, 72), false, "copper"),
        (&s.tracks, Rgb(52, 128, 70), false, "copper"),
        (&s.silk, Rgb(238, 238, 232), false, "silk"),
        (&s.bodies, Rgb(52, 52, 58), true, "bodies"),
    ];
    let topos: Vec<Topology> = parts.iter().map(|(m, ..)| solid::topology(m)).collect();
    let mut ds = vec![];
    for ((m, color, edges, part), topo) in parts.iter().zip(&topos) {
        if m.tris.is_empty() || !v.shows(part) {
            continue;
        }
        // The substrate: mask green on its faces, the laminate's colour on its sides.
        let face_colors = if *part == "board" {
            let mask = v.shows("mask");
            topo.faces
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let cap = f.surface <= 1;
                    let c = if cap && mask {
                        Rgb(20, 70, 40)
                    } else {
                        Rgb(150, 130, 80)
                    };
                    (i, c)
                })
                .collect()
        } else {
            vec![]
        };
        ds.push(Drawable {
            mesh: m,
            topo,
            color: *color,
            line: Rgb(20, 20, 24),
            face_colors,
            edge_colors: vec![],
            vertex_colors: vec![],
            transparency: 0,
            show_edges: *edges,
            line_width: 1.0,
        });
    }
    let style = Style {
        background_top: Rgb(204, 204, 230),
        background_bottom: Rgb(102, 102, 128),
    };
    view::render(&cam, w, h, &ds, &[], &style)
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl Kicad {
    fn v3d_canvas_size(parts: &[&str]) -> (u32, u32) {
        (
            parts.first().and_then(|v| v.parse().ok()).unwrap_or(800),
            parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(600),
        )
    }
    /// Keep a camera the view has moved to, in whole micrometres.
    fn v3d_store(&mut self, cam: &Camera) {
        let um = |v: f64| (v * 1000.0).round() as i64;
        self.ui.v3d.target_um = (um(cam.target.x), um(cam.target.y), um(cam.target.z));
        self.ui.v3d.zoom_um = um(cam.half_height).max(1);
    }
    fn v3d_cam(&self, w: u32, h: u32) -> Camera {
        camera(&self.ui.v3d, &scene(&self.session.board), w, h)
    }
    pub(super) fn v3d_key(&mut self, window: u64, key: &str) -> Result<Vec<AppEffect>, String> {
        let cmd = match key {
            "z" => "view:top",
            "Z" | "Shift+Z" | "Shift+z" => "view:bottom",
            "y" => "view:front",
            "Y" | "Shift+Y" | "Shift+y" => "view:back",
            "x" => "view:right",
            "X" | "Shift+X" | "Shift+x" => "view:left",
            "Home" => "zoom:fit",
            "F1" => "zoom:in",
            "F2" => "zoom:out",
            "ArrowLeft" => "orbit:left",
            "ArrowRight" => "orbit:right",
            "ArrowUp" => "orbit:up",
            "ArrowDown" => "orbit:down",
            other => return Err(format!("{other} does nothing in the 3D Viewer")),
        };
        self.v3d_command(window, cmd)
    }
    pub(super) fn v3d_pointer(
        &mut self,
        phase: PointerPhase,
        parts: &[&str],
        x: i32,
        y: i32,
    ) -> Result<Vec<AppEffect>, String> {
        let (w, h) = Self::v3d_canvas_size(parts);
        self.ui.canvas = (w, h);
        match phase {
            PointerPhase::Down => self.ui.v3d.drag = Some((x, y)),
            PointerPhase::Move => {
                let Some((px, py)) = self.ui.v3d.drag else {
                    return Ok(vec![]);
                };
                let (dx, dy) = (x - px, y - py);
                if self.ui.v3d.pan_mode {
                    let mut cam = self.v3d_cam(w, h);
                    cam.pan(f64::from(dx), f64::from(dy), h);
                    self.v3d_store(&cam);
                } else {
                    // Half a degree per pixel, as a trackball turns.
                    let v = &mut self.ui.v3d;
                    v.yaw = (v.yaw - dx * 5).rem_euclid(3600);
                    v.pitch = (v.pitch + dy * 5).clamp(-900, 900);
                }
                self.ui.v3d.drag = Some((x, y));
            }
            PointerPhase::Up | PointerPhase::Cancel => self.ui.v3d.drag = None,
        }
        Ok(vec![])
    }
    /// A wheel notch zooms about the pointer.
    pub(super) fn v3d_wheel(&mut self, parts: &[&str], x: i32, y: i32, delta: i32) {
        if delta == 0 {
            return;
        }
        let (w, h) = Self::v3d_canvas_size(parts);
        let mut cam = self.v3d_cam(w, h);
        let factor = if delta < 0 { 0.8 } else { 1.25 };
        cam.zoom_at(factor, f64::from(x), f64::from(y), w, h);
        self.v3d_store(&cam);
    }
    pub(super) fn v3d_command(
        &mut self,
        _window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let parts: Vec<&str> = rest.split(':').collect();
        let arg = parts.get(1).copied().unwrap_or("");
        match parts[0] {
            "view" => {
                let (_, label, yaw, pitch) = VIEWS
                    .iter()
                    .find(|v| v.0 == arg)
                    .ok_or(format!("unknown view {arg}"))?;
                self.ui.v3d.yaw = *yaw;
                self.ui.v3d.pitch = *pitch;
                self.ui.status = format!("{label} view");
            }
            "zoom" => {
                if arg == "fit" {
                    self.ui.v3d.zoom_um = 0;
                } else {
                    let (w, h) = if self.ui.canvas.0 > 0 {
                        self.ui.canvas
                    } else {
                        (800, 600)
                    };
                    let mut cam = self.v3d_cam(w, h);
                    let factor = match arg {
                        "in" => 1.0 / 1.5,
                        "out" => 1.5,
                        other => return Err(format!("unknown zoom {other}")),
                    };
                    cam.zoom_at(factor, f64::from(w) / 2.0, f64::from(h) / 2.0, w, h);
                    self.v3d_store(&cam);
                }
            }
            "orbit" => {
                let v = &mut self.ui.v3d;
                match arg {
                    "left" => v.yaw = (v.yaw + 150).rem_euclid(3600),
                    "right" => v.yaw = (v.yaw - 150).rem_euclid(3600),
                    "up" => v.pitch = (v.pitch + 150).min(900),
                    "down" => v.pitch = (v.pitch - 150).max(-900),
                    other => return Err(format!("unknown direction {other}")),
                }
            }
            "mode" => match arg {
                "orbit" => self.ui.v3d.pan_mode = false,
                "pan" => self.ui.v3d.pan_mode = true,
                other => return Err(format!("unknown mode {other}")),
            },
            "toggle" => {
                if !PARTS.iter().any(|p| p.0 == arg) {
                    return Err(format!("unknown layer {arg}"));
                }
                let hidden = &mut self.ui.v3d.hidden;
                if let Some(i) = hidden.iter().position(|h| h == arg) {
                    hidden.remove(i);
                } else {
                    hidden.push(arg.to_owned());
                }
            }
            other => return Err(format!("unknown 3D Viewer command {other}")),
        }
        Ok(vec![])
    }

    pub(super) fn render_v3d(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (w, h) = (env.width, env.height);
        let c = chrome(env.theme);
        p.scene.background = c.bar;
        let top = (w::MENU_H + w::TOOL_H) as i32;
        let area = Rect::new(
            0,
            top,
            w.saturating_sub(PANEL_W + 1),
            h.saturating_sub(w::MENU_H + w::TOOL_H + w::STATUS_H),
        );
        let (cw, ch) = (area.width.max(1), area.height.max(1));
        let v = &self.ui.v3d;
        let key = fnv(format!(
            "{:?}|{}|{}|{}|{:?}|{:?}|{cw}|{ch}",
            self.session.board, v.yaw, v.pitch, v.zoom_um, v.target_um, v.hidden
        )
        .as_bytes());
        let cached = v.cache.0.lock().ok().and_then(|g| {
            g.as_ref()
                .filter(|(k, _)| *k == key)
                .map(|(_, r)| r.clone())
        });
        let rgba = match cached {
            Some(r) => r,
            None => {
                let r = Arc::new(raster(&self.session.board, v, cw, ch));
                if let Ok(mut g) = v.cache.0.lock() {
                    *g = Some((key, r.clone()));
                }
                r
            }
        };
        p.node(
            area,
            Primitive::Image {
                width: cw,
                height: ch,
                rgba: (*rgba).clone(),
            },
            None,
        );
        p.region(area, &format!("kicad:canvas:3d:{cw}:{ch}"), "3D view");
        // Toolbar.
        let ty = w::MENU_H as i32;
        p.box_(Rect::new(0, ty, w, w::TOOL_H), c.bar, 0);
        p.hline(0, ty + w::TOOL_H as i32 - 1, w, w::EDGE);
        let mut x = 6;
        for (icon, target, tip, on) in [
            (
                icons::zoom_in as w::Icon,
                "kicad:v3d:zoom:in",
                "Zoom in (F1)",
                false,
            ),
            (
                icons::zoom_out,
                "kicad:v3d:zoom:out",
                "Zoom out (F2)",
                false,
            ),
            (
                icons::zoom_fit,
                "kicad:v3d:zoom:fit",
                "Zoom to fit (Home)",
                false,
            ),
            (
                icons::orbit,
                "kicad:v3d:mode:orbit",
                "Drag to orbit",
                !v.pan_mode,
            ),
            (icons::pan, "kicad:v3d:mode:pan", "Drag to pan", v.pan_mode),
        ] {
            w::tool(p, &c, x, ty + 3, icon, Ok(target.into()), tip, on);
            x += 30;
        }
        x += 8;
        w::separator_v(p, x - 5, ty + 3);
        for (name, label, yaw, pitch) in VIEWS {
            let tw = p.measure(label, 12, false) + 14;
            let r = Rect::new(x, ty + 5, tw, 24);
            let on = v.yaw == yaw && v.pitch == pitch;
            p.button(
                r,
                if on { c.selection } else { w::WHITE },
                c.radius,
                &format!("kicad:v3d:view:{name}"),
                &format!("{label} view"),
            );
            p.border(r, Color::TRANSPARENT, c.radius, w::EDGE);
            p.label(r.x, r.y + 4, tw, label, 12, w::INK, on, Align::Center);
            x += tw as i32 + 4;
        }
        // Appearance panel.
        let px = area.x + area.width as i32 + 1;
        p.box_(Rect::new(px, area.y, PANEL_W, area.height), c.panel, 0);
        p.vline(px - 1, area.y, area.height, w::EDGE);
        p.label(
            px + 8,
            area.y + 6,
            PANEL_W - 16,
            "Appearance",
            12,
            w::MUTED,
            true,
            Align::Left,
        );
        for (i, (part, label)) in PARTS.iter().enumerate() {
            w::checkbox(
                p,
                &c,
                px + 10,
                area.y + 30 + i as i32 * 26,
                label,
                v.shows(part),
                &format!("kicad:v3d:toggle:{part}"),
            );
        }
        let b = &self.session.board;
        let outlined = b.outline().is_some_and(|o| o.len() >= 3);
        p.paragraph(
            px + 10,
            area.y + 30 + PARTS.len() as i32 * 26 + 12,
            PANEL_W - 20,
            &format!(
                "Board {} mm thick. {}",
                BOARD_THICKNESS,
                if outlined {
                    "Outline from Edge.Cuts."
                } else {
                    "No Edge.Cuts outline: the board shown surrounds the footprints."
                }
            ),
            12,
            w::MUTED,
        );
        w::status_bar(
            p,
            &c,
            w,
            h,
            &[
                format!(
                    "Turn {:.1}° Tilt {:.1}°",
                    v.yaw as f64 / 10.0,
                    v.pitch as f64 / 10.0
                ),
                format!("{} footprints", b.footprints.len()),
                if v.pan_mode { "pan" } else { "orbit" }.into(),
                self.ui.status.clone(),
            ],
        );
        let menus = v3d_menus(self);
        let titles: Vec<&str> = menus.iter().map(|m| m.0).collect();
        let xs = w::menubar(p, &c, w, &titles, self.ui.menu.as_deref());
        if let Some(open) = &self.ui.menu {
            if let Some(i) = titles.iter().position(|t| t == open) {
                p.z += 40;
                w::menu_panel(p, &c, xs[i], &menus[i].1);
                p.z -= 40;
            }
        }
    }

    pub(super) fn v3d_page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let mut buttons: Vec<(String, String)> = vec![
            ("kicad:v3d:zoom:in".into(), "Zoom In".into()),
            ("kicad:v3d:zoom:out".into(), "Zoom Out".into()),
            ("kicad:v3d:zoom:fit".into(), "Zoom to Fit".into()),
            ("kicad:v3d:mode:orbit".into(), "Orbit".into()),
            ("kicad:v3d:mode:pan".into(), "Pan".into()),
        ];
        for (name, label, ..) in VIEWS {
            buttons.push((format!("kicad:v3d:view:{name}"), format!("{label} View")));
        }
        for (part, label) in PARTS {
            buttons.push((format!("kicad:v3d:toggle:{part}"), format!("Show {label}")));
        }
        for (id, text) in buttons {
            page.elements.push(E::Button {
                id: id.clone(),
                text,
                action: act(&id),
            });
        }
        let s = scene(&self.session.board);
        let v = &self.ui.v3d;
        page.elements.push(E::Text {
            id: "kicad-3d".into(),
            text: format!(
                "3D view: {} footprints, {} drilled holes, board {} mm thick ({}), turn {}°, tilt {}°, hidden: {}",
                self.session.board.footprints.len(),
                s.holes,
                BOARD_THICKNESS,
                if s.outlined { "Edge.Cuts outline" } else { "no outline" },
                v.yaw as f64 / 10.0,
                v.pitch as f64 / 10.0,
                if v.hidden.is_empty() {
                    "nothing".to_owned()
                } else {
                    v.hidden.join(", ")
                }
            ),
        });
    }
}

fn v3d_menus(k: &Kicad) -> Vec<(&'static str, Vec<MenuItem>)> {
    let views = VIEWS
        .iter()
        .map(|(name, label, ..)| {
            let keys = match *name {
                "top" => "Z",
                "bottom" => "Shift+Z",
                "front" => "Y",
                "back" => "Shift+Y",
                "left" => "Shift+X",
                "right" => "X",
                _ => "",
            };
            MenuItem::new(
                &format!("{label} View"),
                keys,
                Ok(format!("kicad:v3d:view:{name}")),
            )
        })
        .collect();
    let mut view_menu = vec![
        MenuItem::new("Zoom In", "F1", Ok("kicad:v3d:zoom:in".into())),
        MenuItem::new("Zoom Out", "F2", Ok("kicad:v3d:zoom:out".into())),
        MenuItem::new("Zoom to Fit", "Home", Ok("kicad:v3d:zoom:fit".into())).sep(),
    ];
    view_menu.extend(PARTS.iter().map(|(part, label)| {
        MenuItem::new(
            &format!(
                "{} {label}",
                if k.ui.v3d.shows(part) { "Hide" } else { "Show" }
            ),
            "",
            Ok(format!("kicad:v3d:toggle:{part}")),
        )
    }));
    vec![
        ("View", view_menu),
        ("Views", views),
        (
            "Help",
            vec![MenuItem::new("About KiCad", "", Ok("kicad:about".into()))],
        ),
    ]
}
