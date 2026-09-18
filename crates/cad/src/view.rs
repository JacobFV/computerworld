//! The 3D view: an orthographic or perspective camera, a z-buffer rasteriser that shades
//! faces with a headlight and draws edges and vertices depth-tested over them, picking
//! of faces, edges and vertices under the pointer, the navigation cube's facets, and a
//! hidden-line SVG projection. All integer or correctly rounded arithmetic.
use crate::math::{self, v2, v3, V2, V3};
use crate::mesh::Mesh;
use crate::solid::Topology;
use serde::{Deserialize, Serialize};

/// Camera: what it looks at, how it is turned, how much it sees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub target: V3,
    /// Screen right and screen up, in world space; orthonormal.
    pub right: V3,
    pub up: V3,
    /// Half the height of the view, in millimetres at the target.
    pub half_height: f64,
    pub perspective: bool,
}
/// FreeCAD's perspective camera height angle.
const FOV: f64 = std::f64::consts::FRAC_PI_4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StdView {
    Isometric,
    Dimetric,
    Trimetric,
    Front,
    Top,
    Right,
    Rear,
    Bottom,
    Left,
}
impl StdView {
    pub fn label(self) -> &'static str {
        match self {
            StdView::Isometric => "Isometric",
            StdView::Dimetric => "Dimetric",
            StdView::Trimetric => "Trimetric",
            StdView::Front => "Front",
            StdView::Top => "Top",
            StdView::Right => "Right",
            StdView::Rear => "Rear",
            StdView::Bottom => "Bottom",
            StdView::Left => "Left",
        }
    }
    pub const ALL: [StdView; 9] = [
        StdView::Isometric,
        StdView::Dimetric,
        StdView::Trimetric,
        StdView::Front,
        StdView::Top,
        StdView::Right,
        StdView::Rear,
        StdView::Bottom,
        StdView::Left,
    ];
    pub fn parse(s: &str) -> Option<StdView> {
        StdView::ALL
            .into_iter()
            .find(|v| v.label().eq_ignore_ascii_case(s))
    }
}

/// Rotation matrix columns (right, up, back) of a unit quaternion (x, y, z, w).
fn from_quat(x: f64, y: f64, z: f64, w: f64) -> (V3, V3) {
    let n = math::sqrt(x * x + y * y + z * z + w * w);
    let (x, y, z, w) = (x / n, y / n, z / n, w / n);
    let right = v3(
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
    );
    let up = v3(
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
    );
    (right, up)
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Camera {
            target: V3::ZERO,
            right: V3::X,
            up: V3::Y,
            half_height: 50.0,
            perspective: false,
        };
        c.set_view(StdView::Trimetric);
        c
    }
}

impl Camera {
    /// Direction from the target towards the viewer.
    pub fn back(&self) -> V3 {
        self.right.cross(self.up)
    }
    /// Turn to a standard view, keeping target and zoom. FreeCAD's orientations.
    pub fn set_view(&mut self, v: StdView) {
        let (r, u) = match v {
            StdView::Front => (V3::X, V3::Z),
            StdView::Rear => (-V3::X, V3::Z),
            StdView::Top => (V3::X, V3::Y),
            StdView::Bottom => (V3::X, -V3::Y),
            StdView::Right => (V3::Y, V3::Z),
            StdView::Left => (-V3::Y, V3::Z),
            StdView::Isometric => {
                // Looking from (1, -1, 1) with Z up.
                return self.look_from(v3(1.0, -1.0, 1.0), V3::Z);
            }
            StdView::Dimetric => from_quat(0.567_952, 0.103_751, 0.146_726, 0.803_205),
            StdView::Trimetric => from_quat(0.424_708, 0.175_920, 0.335_543, 0.824_957),
        };
        self.right = r.norm();
        self.up = u.norm();
        self.orthonormalise();
    }
    /// Look along `-from` (the viewer at `target + from`), with `up_hint` up.
    pub fn look_from(&mut self, from: V3, up_hint: V3) {
        let back = from.norm();
        let mut up = up_hint - back * up_hint.dot(back);
        if up.len() < 1e-9 {
            up = back.any_perp();
        }
        let up = up.norm();
        self.up = up;
        self.right = up.cross(back).norm();
        self.orthonormalise();
    }
    fn orthonormalise(&mut self) {
        let back = self.right.cross(self.up).norm();
        self.up = back.cross(self.right).norm();
        self.right = self.up.cross(back).norm();
    }
    /// Orbit about the target: `dx` turns about screen up, `dy` about screen right, both
    /// radians. FreeCAD's trackball.
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        let turn_y = math::Xform::rotate(V3::ZERO, self.up, -dx);
        self.right = turn_y.dir(self.right);
        let turn_x = math::Xform::rotate(V3::ZERO, self.right, -dy);
        self.up = turn_x.dir(self.up);
        self.orthonormalise();
    }
    /// Spin about the view direction.
    pub fn roll(&mut self, angle: f64) {
        let t = math::Xform::rotate(V3::ZERO, self.back(), angle);
        self.right = t.dir(self.right);
        self.up = t.dir(self.up);
        self.orthonormalise();
    }
    /// Move the target by a screen-space offset in pixels (view `height` pixels high).
    pub fn pan(&mut self, dx: f64, dy: f64, height: u32) {
        let mm_per_px = 2.0 * self.half_height / f64::from(height.max(1));
        self.target = self.target - self.right * (dx * mm_per_px) + self.up * (dy * mm_per_px);
    }
    /// Zoom by `factor` (<1 in) keeping the point under (`x`, `y`) fixed on screen.
    pub fn zoom_at(&mut self, factor: f64, x: f64, y: f64, w: u32, h: u32) {
        let factor = factor.clamp(0.05, 20.0);
        let before = self.screen_to_plane(x, y, w, h);
        self.half_height = (self.half_height * factor).clamp(1e-3, 1e7);
        let after = self.screen_to_plane(x, y, w, h);
        self.target += before - after;
    }
    /// The point on the plane through the target facing the viewer, under a pixel.
    pub fn screen_to_plane(&self, x: f64, y: f64, w: u32, h: u32) -> V3 {
        let s = 2.0 * self.half_height / f64::from(h.max(1));
        self.target + self.right * ((x - f64::from(w) / 2.0) * s)
            - self.up * ((y - f64::from(h) / 2.0) * s)
    }
    fn distance(&self) -> f64 {
        self.half_height / math::tan(FOV / 2.0)
    }
    pub fn eye(&self) -> V3 {
        self.target + self.back() * self.distance()
    }
    /// Frame `bounds` in a `w`×`h` view.
    pub fn fit(&mut self, min: V3, max: V3, w: u32, h: u32) {
        let c = (min + max) * 0.5;
        let radius = ((max - min).len() / 2.0).max(1.0);
        self.target = c;
        let aspect = f64::from(w.max(1)) / f64::from(h.max(1));
        self.half_height = radius * 1.1 / aspect.min(1.0);
    }
    /// Screen position (pixels) and a depth key that grows away from the viewer, or
    /// `None` behind a perspective camera.
    pub fn project(&self, p: V3, w: u32, h: u32) -> Option<(f64, f64, f64)> {
        let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
        let px_per_mm = f64::from(h.max(1)) / (2.0 * self.half_height);
        if self.perspective {
            let v = p - self.eye();
            let z = -v.dot(self.back());
            let near = self.distance() * 0.01;
            if z < near {
                return None;
            }
            let f = self.distance() * px_per_mm;
            Some((
                cx + v.dot(self.right) * f / z,
                cy - v.dot(self.up) * f / z,
                -1.0 / z,
            ))
        } else {
            let v = p - self.target;
            Some((
                cx + v.dot(self.right) * px_per_mm,
                cy - v.dot(self.up) * px_per_mm,
                -v.dot(self.back()),
            ))
        }
    }
    /// The ray under a pixel: origin and unit direction.
    pub fn ray(&self, x: f64, y: f64, w: u32, h: u32) -> (V3, V3) {
        let on_plane = self.screen_to_plane(x, y, w, h);
        if self.perspective {
            let eye = self.eye();
            (eye, (on_plane - eye).norm())
        } else {
            (
                on_plane + self.back() * (self.half_height * 1e3),
                -self.back(),
            )
        }
    }
}

// ---------------------------------------------------------------------------------
// Rasteriser.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// How to draw one shape.
pub struct Drawable<'a> {
    pub mesh: &'a Mesh,
    pub topo: &'a Topology,
    pub color: Rgb,
    pub line: Rgb,
    /// Face colour overrides (selection, preselection), by face index.
    pub face_colors: Vec<(usize, Rgb)>,
    pub edge_colors: Vec<(usize, Rgb)>,
    pub vertex_colors: Vec<(usize, Rgb)>,
    /// Percent: 0 is opaque. A transparent shape is blended over what is behind it.
    pub transparency: u8,
    pub show_edges: bool,
    /// Edge width in pixels.
    pub line_width: f64,
}
/// A polyline drawn depth-tested into the scene (sketch geometry, axes).
pub struct Polyline {
    pub pts: Vec<V3>,
    pub color: Rgb,
    pub width: f64,
    /// Drawn over everything, as geometry being edited is.
    pub on_top: bool,
    pub dashed: bool,
}

pub struct Style {
    pub background_top: Rgb,
    pub background_bottom: Rgb,
}

/// Supersampling factor per axis.
const SS: u32 = 2;

struct Target {
    w: u32,
    h: u32,
    color: Vec<[u8; 3]>,
    depth: Vec<f64>,
    /// Transparency (percent) of the faces being drawn.
    see_through: u8,
}
impl Target {
    fn put(&mut self, x: i64, y: i64, key: f64, c: Rgb, test: Option<f64>) {
        if x < 0 || y < 0 || x >= i64::from(self.w) || y >= i64::from(self.h) {
            return;
        }
        let i = y as usize * self.w as usize + x as usize;
        match test {
            // A line pixel within `bias` of the surface under it is drawn over it.
            Some(bias) => {
                if key > self.depth[i] + bias {
                    return;
                }
            }
            None => {
                if key >= self.depth[i] {
                    return;
                }
                if self.see_through > 0 {
                    // Blended over what is already there, leaving the depth to it.
                    let t = f64::from(self.see_through.min(100)) / 100.0;
                    let old = self.color[i];
                    let mixed = mix(Rgb(c.0, c.1, c.2), Rgb(old[0], old[1], old[2]), t);
                    self.color[i] = [mixed.0, mixed.1, mixed.2];
                    return;
                }
                self.depth[i] = key;
            }
        }
        self.color[i] = [c.0, c.1, c.2];
    }
    fn tri(&mut self, p: [(f64, f64, f64); 3], c: Rgb) {
        let min_x = p
            .iter()
            .map(|q| q.0)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as i64;
        let max_x = p
            .iter()
            .map(|q| q.0)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(f64::from(self.w)) as i64;
        let min_y = p
            .iter()
            .map(|q| q.1)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as i64;
        let max_y = p
            .iter()
            .map(|q| q.1)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(f64::from(self.h)) as i64;
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[1].1 - p[0].1) * (p[2].0 - p[0].0);
        if area.abs() < 1e-12 {
            return;
        }
        for y in min_y..max_y {
            for x in min_x..max_x {
                let (sx, sy) = (x as f64 + 0.5, y as f64 + 0.5);
                let e = |a: (f64, f64, f64), b: (f64, f64, f64)| {
                    (b.0 - a.0) * (sy - a.1) - (b.1 - a.1) * (sx - a.0)
                };
                let (w0, w1, w2) = (e(p[1], p[2]), e(p[2], p[0]), e(p[0], p[1]));
                let inside = if area > 0.0 {
                    w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
                } else {
                    w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
                };
                if !inside {
                    continue;
                }
                let key = (w0 * p[0].2 + w1 * p[1].2 + w2 * p[2].2) / area;
                self.put(x, y, key, c, None);
            }
        }
    }
    /// A thick depth-tested line from `a` to `b` (screen space with depth keys).
    fn line(
        &mut self,
        a: (f64, f64, f64),
        b: (f64, f64, f64),
        width: f64,
        c: Rgb,
        bias: Option<f64>,
        dashed: bool,
    ) {
        let len = ((b.0 - a.0) * (b.0 - a.0) + (b.1 - a.1) * (b.1 - a.1)).sqrt();
        let steps = (len * 2.0).ceil().max(1.0) as usize;
        let r = (width / 2.0).max(0.5);
        let ri = r.ceil() as i64;
        for s in 0..=steps {
            let t = s as f64 / steps as f64;
            if dashed && ((len * t / (6.0 * SS as f64)) as i64) % 2 == 1 {
                continue;
            }
            let (x, y, k) = (
                a.0 + (b.0 - a.0) * t,
                a.1 + (b.1 - a.1) * t,
                a.2 + (b.2 - a.2) * t,
            );
            for dy in -ri..=ri {
                for dx in -ri..=ri {
                    let (px, py) = (x.floor() as i64 + dx, y.floor() as i64 + dy);
                    let (cx, cy) = (px as f64 + 0.5 - x, py as f64 + 0.5 - y);
                    if cx * cx + cy * cy <= r * r {
                        match bias {
                            Some(b) => self.put(px, py, k, c, Some(b)),
                            None => {
                                if px >= 0
                                    && py >= 0
                                    && px < i64::from(self.w)
                                    && py < i64::from(self.h)
                                {
                                    let i = py as usize * self.w as usize + px as usize;
                                    self.color[i] = [c.0, c.1, c.2];
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn shade(c: Rgb, light: f64) -> Rgb {
    let f = |v: u8| ((f64::from(v) * light).round().clamp(0.0, 255.0)) as u8;
    Rgb(f(c.0), f(c.1), f(c.2))
}
fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let f = |x: u8, y: u8| (f64::from(x) * (1.0 - t) + f64::from(y) * t).round() as u8;
    Rgb(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

/// Screen-space triangle of mesh triangle `i`, clipped against a perspective camera's
/// near plane by dropping triangles that cross it.
fn screen_tri(
    cam: &Camera,
    m: &Mesh,
    i: usize,
    w: u32,
    h: u32,
    ss: f64,
) -> Option<[(f64, f64, f64); 3]> {
    let t = m.tri(i);
    let mut out = [(0.0, 0.0, 0.0); 3];
    for k in 0..3 {
        let (x, y, d) = cam.project(t[k], w, h)?;
        out[k] = (x * ss, y * ss, d);
    }
    Some(out)
}

/// Render drawables and polylines into RGBA pixels, `w`×`h`.
pub fn render(
    cam: &Camera,
    w: u32,
    h: u32,
    shapes: &[Drawable<'_>],
    lines: &[Polyline],
    style: &Style,
) -> Vec<u8> {
    let (sw, sh) = (w * SS, h * SS);
    let mut t = Target {
        w: sw,
        h: sh,
        color: Vec::with_capacity((sw * sh) as usize),
        depth: vec![f64::INFINITY; (sw * sh) as usize],
        see_through: 0,
    };
    for y in 0..sh {
        let c = mix(
            style.background_top,
            style.background_bottom,
            f64::from(y) / f64::from(sh.max(2) - 1),
        );
        for _ in 0..sw {
            t.color.push([c.0, c.1, c.2]);
        }
    }
    let ss = f64::from(SS);
    let back = cam.back();
    // Depth range, for an edge bias that suits the scene's scale.
    let mut range = (f64::INFINITY, f64::NEG_INFINITY);
    for d in shapes {
        for p in &d.mesh.verts {
            if let Some((_, _, k)) = cam.project(*p, w, h) {
                range = (range.0.min(k), range.1.max(k));
            }
        }
    }
    let bias = if range.0.is_finite() {
        (range.1 - range.0).max(1e-9) * 2e-3
    } else {
        1e-6
    };
    // Opaque shapes first, so transparent ones blend over everything behind them.
    let mut order: Vec<&Drawable<'_>> = shapes.iter().collect();
    order.sort_by_key(|d| d.transparency > 0);
    for d in order {
        t.see_through = d.transparency.min(100);
        let mut face_color = vec![d.color; d.topo.faces.len()];
        for (f, c) in &d.face_colors {
            if let Some(slot) = face_color.get_mut(*f) {
                *slot = *c;
            }
        }
        for i in 0..d.mesh.tris.len() {
            let n = d.mesh.tri_normal(i);
            let facing = if cam.perspective {
                let [a, ..] = d.mesh.tri(i);
                n.dot((cam.eye() - a).norm())
            } else {
                n.dot(back)
            };
            if facing <= 0.0 {
                continue;
            }
            let Some(p) = screen_tri(cam, d.mesh, i, w, h, ss) else {
                continue;
            };
            let face = d.topo.tri_face.get(i).copied().unwrap_or(0);
            let base = face_color.get(face).copied().unwrap_or(d.color);
            // A headlight a little above and left of the eye, plus ambient, so faces
            // turned equally from the view (an isometric box) still read apart.
            let light = (back + cam.up * 0.45 - cam.right * 0.25).norm();
            let lit = n.dot(light).max(0.0);
            let c = shade(base, 0.38 + 0.62 * lit);
            t.tri(p, c);
        }
    }
    t.see_through = 0;
    for d in shapes {
        if !d.show_edges {
            continue;
        }
        for (ei, e) in d.topo.edges.iter().enumerate() {
            let c = d.edge_colors.iter().find(|x| x.0 == ei).map(|x| x.1);
            let width = if c.is_some() {
                (d.line_width + 1.5) * ss
            } else {
                d.line_width * 0.75 * ss
            };
            let color = c.unwrap_or(d.line);
            for pair in e.verts.windows(2) {
                let (Some(a), Some(b)) = (
                    cam.project(d.mesh.verts[pair[0] as usize], w, h),
                    cam.project(d.mesh.verts[pair[1] as usize], w, h),
                ) else {
                    continue;
                };
                t.line(
                    (a.0 * ss, a.1 * ss, a.2),
                    (b.0 * ss, b.1 * ss, b.2),
                    width,
                    color,
                    Some(bias),
                    false,
                );
            }
        }
        for (vi, v) in d.topo.vertices.iter().enumerate() {
            let Some(p) = cam.project(d.mesh.verts[*v as usize], w, h) else {
                continue;
            };
            let c = d.vertex_colors.iter().find(|x| x.0 == vi).map(|x| x.1);
            let size = if c.is_some() { 4.0 * ss } else { 2.5 * ss };
            t.line(
                (p.0 * ss, p.1 * ss, p.2),
                (p.0 * ss, p.1 * ss, p.2),
                size,
                c.unwrap_or(d.line),
                Some(bias),
                false,
            );
        }
    }
    for l in lines {
        for pair in l.pts.windows(2) {
            let (Some(a), Some(b)) = (cam.project(pair[0], w, h), cam.project(pair[1], w, h))
            else {
                continue;
            };
            let test = if l.on_top { None } else { Some(bias) };
            t.line(
                (a.0 * ss, a.1 * ss, a.2),
                (b.0 * ss, b.1 * ss, b.2),
                l.width * ss,
                l.color,
                test,
                l.dashed,
            );
        }
    }
    // Box-filter down to the requested size.
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 3];
            for dy in 0..SS {
                for dx in 0..SS {
                    let c = t.color[((y * SS + dy) * sw + x * SS + dx) as usize];
                    for k in 0..3 {
                        acc[k] += u32::from(c[k]);
                    }
                }
            }
            let n = SS * SS;
            out.extend_from_slice(&[
                ((acc[0] + n / 2) / n) as u8,
                ((acc[1] + n / 2) / n) as u8,
                ((acc[2] + n / 2) / n) as u8,
                255,
            ]);
        }
    }
    out
}

// ---------------------------------------------------------------------------------
// Picking.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    Face(usize),
    Edge(usize),
    Vertex(usize),
}
impl Element {
    pub fn name(&self) -> String {
        match self {
            Element::Face(i) => format!("Face{}", i + 1),
            Element::Edge(i) => format!("Edge{}", i + 1),
            Element::Vertex(i) => format!("Vertex{}", i + 1),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pick {
    /// Which of the drawables.
    pub shape: usize,
    pub element: Element,
    /// The picked point in world space.
    pub point: V3,
}

/// What lies under pixel (`x`, `y`): a vertex or edge within `tol` pixels that is not
/// hidden, else the nearest face.
pub fn pick(
    cam: &Camera,
    w: u32,
    h: u32,
    shapes: &[(&Mesh, &Topology)],
    x: f64,
    y: f64,
    tol: f64,
) -> Option<Pick> {
    // Nearest face under the pixel, by the same screen-space rule the rasteriser uses.
    let mut face: Option<(f64, usize, usize, [f64; 3], usize)> = None;
    for (si, (m, topo)) in shapes.iter().enumerate() {
        for i in 0..m.tris.len() {
            let Some(p) = screen_tri(cam, m, i, w, h, 1.0) else {
                continue;
            };
            let area =
                (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[1].1 - p[0].1) * (p[2].0 - p[0].0);
            // Screen y runs down, so a triangle facing the viewer winds clockwise on
            // screen. Back-facing (or edge-on) triangles are culled, as they are drawn.
            if area >= -1e-12 {
                continue;
            }
            let e = |a: (f64, f64, f64), b: (f64, f64, f64)| {
                (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0)
            };
            let (w0, w1, w2) = (e(p[1], p[2]), e(p[2], p[0]), e(p[0], p[1]));
            if w0 > 0.0 || w1 > 0.0 || w2 > 0.0 {
                continue;
            }
            let bary = [w0 / area, w1 / area, w2 / area];
            let key = bary[0] * p[0].2 + bary[1] * p[1].2 + bary[2] * p[2].2;
            if face.is_none_or(|f| key < f.0) {
                face = Some((key, si, i, bary, topo.tri_face.get(i).copied().unwrap_or(0)));
            }
        }
    }
    let surface_key = face.map(|f| f.0).unwrap_or(f64::INFINITY);
    let mut range = (f64::INFINITY, f64::NEG_INFINITY);
    for (m, _) in shapes {
        for p in &m.verts {
            if let Some((_, _, k)) = cam.project(*p, w, h) {
                range = (range.0.min(k), range.1.max(k));
            }
        }
    }
    let bias = if range.0.is_finite() {
        (range.1 - range.0).max(1e-9) * 5e-3
    } else {
        1e-6
    };
    let visible = |k: f64| k <= surface_key + bias;
    // Vertices first: they are the smallest targets.
    let mut best: Option<(f64, Pick)> = None;
    for (si, (m, topo)) in shapes.iter().enumerate() {
        for (vi, v) in topo.vertices.iter().enumerate() {
            let p = m.verts[*v as usize];
            let Some((px, py, k)) = cam.project(p, w, h) else {
                continue;
            };
            let d = ((px - x) * (px - x) + (py - y) * (py - y)).sqrt();
            if d <= tol && visible(k) && best.is_none_or(|b| d < b.0) {
                best = Some((
                    d,
                    Pick {
                        shape: si,
                        element: Element::Vertex(vi),
                        point: p,
                    },
                ));
            }
        }
    }
    if let Some((_, p)) = best {
        return Some(p);
    }
    for (si, (m, topo)) in shapes.iter().enumerate() {
        for (ei, e) in topo.edges.iter().enumerate() {
            for pair in e.verts.windows(2) {
                let (pa, pb) = (m.verts[pair[0] as usize], m.verts[pair[1] as usize]);
                let (Some(a), Some(b)) = (cam.project(pa, w, h), cam.project(pb, w, h)) else {
                    continue;
                };
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len2 = dx * dx + dy * dy;
                let s = if len2 > 0.0 {
                    (((x - a.0) * dx + (y - a.1) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (qx, qy) = (a.0 + dx * s, a.1 + dy * s);
                let d = ((qx - x) * (qx - x) + (qy - y) * (qy - y)).sqrt();
                let k = a.2 + (b.2 - a.2) * s;
                if d <= tol && visible(k) && best.is_none_or(|b| d < b.0) {
                    best = Some((
                        d,
                        Pick {
                            shape: si,
                            element: Element::Edge(ei),
                            point: pa.lerp(pb, s),
                        },
                    ));
                }
            }
        }
    }
    if let Some((_, p)) = best {
        return Some(p);
    }
    face.map(|(_, si, tri, bary, f)| {
        let [a, b, c] = shapes[si].0.tri(tri);
        Pick {
            shape: si,
            element: Element::Face(f),
            point: a * bary[0] + b * bary[1] + c * bary[2],
        }
    })
}

// ---------------------------------------------------------------------------------
// Navigation cube: 6 faces, 12 edge facets and 8 corner facets, as FreeCAD's.

/// One clickable facet of the cube: the view direction it selects (from the target
/// towards the viewer) and its outline on the unit cube.
#[derive(Clone, Debug, PartialEq)]
pub struct Facet {
    pub name: String,
    pub dir: V3,
    pub label: Option<&'static str>,
    pub outline: Vec<V3>,
}

pub fn nav_cube() -> Vec<Facet> {
    let c = 0.72; // Face half-size; the rest of each side is chamfer.
    let mut out = Vec::new();
    let faces: [(&str, V3, V3, V3, &'static str); 6] = [
        ("front", -V3::Y, V3::X, V3::Z, "FRONT"),
        ("rear", V3::Y, -V3::X, V3::Z, "REAR"),
        ("right", V3::X, V3::Y, V3::Z, "RIGHT"),
        ("left", -V3::X, -V3::Y, V3::Z, "LEFT"),
        ("top", V3::Z, V3::X, V3::Y, "TOP"),
        ("bottom", -V3::Z, V3::X, -V3::Y, "BOTTOM"),
    ];
    for (name, n, u, v, label) in faces {
        out.push(Facet {
            name: name.into(),
            dir: n,
            label: Some(label),
            outline: vec![
                n + u * -c + v * -c,
                n + u * c + v * -c,
                n + u * c + v * c,
                n + u * -c + v * c,
            ],
        });
    }
    // Edge facets: between two faces, a strip along their shared edge.
    let axes = [V3::X, V3::Y, V3::Z];
    let signs = [-1.0, 1.0];
    for i in 0..3 {
        for j in i + 1..3 {
            for &si in &signs {
                for &sj in &signs {
                    let a = axes[i] * si;
                    let b = axes[j] * sj;
                    let k = axes[3 - i - j];
                    let dir = (a + b).norm();
                    out.push(Facet {
                        name: format!("edge:{}{}", dir_name(a), dir_name(b)),
                        dir,
                        label: None,
                        outline: vec![
                            a + b * c + k * -c,
                            a * c + b + k * -c,
                            a * c + b + k * c,
                            a + b * c + k * c,
                        ],
                    });
                }
            }
        }
    }
    for &sx in &signs {
        for &sy in &signs {
            for &sz in &signs {
                let (x, y, z) = (V3::X * sx, V3::Y * sy, V3::Z * sz);
                out.push(Facet {
                    name: format!("corner:{}{}{}", dir_name(x), dir_name(y), dir_name(z)),
                    dir: (x + y + z).norm(),
                    label: None,
                    outline: vec![x + y * c + z * c, x * c + y + z * c, x * c + y * c + z],
                });
            }
        }
    }
    out
}
fn dir_name(v: V3) -> &'static str {
    match (
        v.x.signum() as i32,
        v.y.signum() as i32,
        v.z.signum() as i32,
    ) {
        (1, 0, 0) => "right",
        (-1, 0, 0) => "left",
        (0, 1, 0) => "rear",
        (0, -1, 0) => "front",
        (0, 0, 1) => "top",
        _ => "bottom",
    }
}

/// The cube's facets as seen by `cam` (orientation only), projected into a square of
/// `size` pixels: (facet index, outline in pixels, how squarely it faces the viewer),
/// back to front.
pub fn nav_cube_projected(cam: &Camera, size: f64) -> Vec<(usize, Vec<V2>, f64)> {
    let back = cam.back();
    let scale = size / 3.4;
    let mut out: Vec<(usize, Vec<V2>, f64)> = nav_cube()
        .into_iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let facing = f.dir.dot(back);
            (facing > 1e-6).then(|| {
                let pts = f
                    .outline
                    .iter()
                    .map(|p| {
                        v2(
                            size / 2.0 + p.dot(cam.right) * scale,
                            size / 2.0 - p.dot(cam.up) * scale,
                        )
                    })
                    .collect();
                (i, pts, facing)
            })
        })
        .collect();
    out.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
    out
}

// ---------------------------------------------------------------------------------
// SVG: a hidden-line projection of shapes as the camera sees them, in millimetres.

/// Visible edges (and the silhouettes of curved faces) as an SVG drawing at 1:1 in
/// millimetres. Hidden edges are drawn dashed and thin when `hidden` is set.
pub fn svg(cam: &Camera, shapes: &[(&Mesh, &Topology)], hidden: bool) -> String {
    let mut ortho = *cam;
    ortho.perspective = false;
    // Bounds in view coordinates.
    let (mut lo, mut hi) = (
        v2(f64::INFINITY, f64::INFINITY),
        v2(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for (m, _) in shapes {
        for p in &m.verts {
            let q = v2(
                (*p - ortho.target).dot(ortho.right),
                (*p - ortho.target).dot(ortho.up),
            );
            lo = v2(lo.x.min(q.x), lo.y.min(q.y));
            hi = v2(hi.x.max(q.x), hi.y.max(q.y));
        }
    }
    if !lo.x.is_finite() {
        return "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n".into();
    }
    let margin = ((hi - lo).len() * 0.05).max(1.0);
    let (lo, hi) = (lo - v2(margin, margin), hi + v2(margin, margin));
    // Depth buffer at 0.1 mm (capped) for the visibility test.
    let span = hi - lo;
    let res = (1600.0 / span.x.max(span.y)).clamp(0.5, 10.0);
    let (bw, bh) = (
        (span.x * res).ceil() as u32 + 1,
        (span.y * res).ceil() as u32 + 1,
    );
    ortho.target =
        ortho.target + ortho.right * ((lo.x + hi.x) / 2.0) + ortho.up * ((lo.y + hi.y) / 2.0);
    ortho.half_height = span.y / 2.0;
    let mut buf = Target {
        w: bw,
        h: bh,
        color: vec![[0; 3]; (bw * bh) as usize],
        depth: vec![f64::INFINITY; (bw * bh) as usize],
        see_through: 0,
    };
    let back = ortho.back();
    for (m, _) in shapes {
        for i in 0..m.tris.len() {
            if m.tri_normal(i).dot(back) <= 0.0 {
                continue;
            }
            if let Some(p) = screen_tri(&ortho, m, i, bw, bh, 1.0) {
                buf.tri(p, Rgb(0, 0, 0));
            }
        }
    }
    let depth_at = |x: f64, y: f64| -> f64 {
        let (xi, yi) = (x.floor() as i64, y.floor() as i64);
        let mut best = f64::INFINITY;
        // The farthest of the neighbourhood, so an edge on a silhouette is not hidden
        // by the face it bounds.
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (px, py) = (xi + dx, yi + dy);
                if px < 0 || py < 0 || px >= i64::from(bw) || py >= i64::from(bh) {
                    return f64::INFINITY;
                }
                let d = buf.depth[py as usize * bw as usize + px as usize];
                if !d.is_finite() {
                    return f64::INFINITY;
                }
                best = if best.is_finite() { best.max(d) } else { d };
            }
        }
        best
    };
    let bias = span.x.max(span.y) * 1e-3;
    let to_mm = |p: (f64, f64)| (p.0 / res, p.1 / res);
    let mut visible: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut hidden_runs: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut segments: Vec<(V3, V3)> = Vec::new();
    for (m, topo) in shapes {
        for e in &topo.edges {
            for pair in e.verts.windows(2) {
                segments.push((m.verts[pair[0] as usize], m.verts[pair[1] as usize]));
            }
        }
        // Silhouettes: mesh edges inside a curved face between a triangle facing the
        // viewer and one facing away.
        let mut owner: std::collections::BTreeMap<(u32, u32), usize> =
            std::collections::BTreeMap::new();
        for (i, t) in m.tris.iter().enumerate() {
            for k in 0..3 {
                owner.insert((t[k], t[(k + 1) % 3]), i);
            }
        }
        for (&(a, b), &i) in &owner {
            if a > b {
                continue;
            }
            if let Some(&j) = owner.get(&(b, a)) {
                if topo.tri_face.get(i) == topo.tri_face.get(j) {
                    let (fi, fj) = (m.tri_normal(i).dot(back), m.tri_normal(j).dot(back));
                    if (fi > 0.0) != (fj > 0.0) {
                        segments.push((m.verts[a as usize], m.verts[b as usize]));
                    }
                }
            }
        }
    }
    for (pa, pb) in segments {
        let (Some(a), Some(b)) = (ortho.project(pa, bw, bh), ortho.project(pb, bw, bh)) else {
            continue;
        };
        let len = ((b.0 - a.0) * (b.0 - a.0) + (b.1 - a.1) * (b.1 - a.1)).sqrt();
        let steps = (len * 2.0).ceil().max(1.0) as usize;
        let mut run: Vec<(f64, f64)> = Vec::new();
        let mut run_visible = true;
        for s in 0..=steps {
            let t = s as f64 / steps as f64;
            let (x, y, k) = (
                a.0 + (b.0 - a.0) * t,
                a.1 + (b.1 - a.1) * t,
                a.2 + (b.2 - a.2) * t,
            );
            let seen = k <= depth_at(x, y) + bias;
            if s > 0 && seen != run_visible {
                if run.len() > 1 {
                    if run_visible {
                        visible.push(run.clone());
                    } else {
                        hidden_runs.push(run.clone());
                    }
                }
                run = vec![*run.last().unwrap()];
            }
            run_visible = seen;
            run.push(to_mm((x, y)));
        }
        if run.len() > 1 {
            if run_visible {
                visible.push(run);
            } else {
                hidden_runs.push(run);
            }
        }
    }
    let (wmm, hmm) = (span.x, span.y);
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n<title>FreeCAD SVG export</title>\n",
        w = math::fmt_num(wmm, 3),
        h = math::fmt_num(hmm, 3)
    );
    let poly = |pts: &[(f64, f64)]| {
        pts.iter()
            .map(|p| format!("{},{}", math::fmt_num(p.0, 4), math::fmt_num(p.1, 4)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    s.push_str("<g id=\"visible\" fill=\"none\" stroke=\"#000000\" stroke-width=\"0.35\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n");
    for r in &visible {
        s.push_str(&format!("<polyline points=\"{}\"/>\n", poly(r)));
    }
    s.push_str("</g>\n");
    if hidden {
        s.push_str("<g id=\"hidden\" fill=\"none\" stroke=\"#000000\" stroke-width=\"0.18\" stroke-dasharray=\"1.5,1\">\n");
        for r in &hidden_runs {
            s.push_str(&format!("<polyline points=\"{}\"/>\n", poly(r)));
        }
        s.push_str("</g>\n");
    }
    s.push_str("</svg>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solid::topology;
    #[test]
    fn standard_views_look_where_freecad_does() {
        let mut c = Camera::default();
        c.set_view(StdView::Front);
        assert!((c.back() - (-V3::Y)).len() < 1e-12, "front looks along +Y");
        c.set_view(StdView::Top);
        assert!((c.back() - V3::Z).len() < 1e-12);
        c.set_view(StdView::Right);
        assert!((c.back() - V3::X).len() < 1e-12);
        c.set_view(StdView::Isometric);
        assert!((c.back() - v3(1.0, -1.0, 1.0).norm()).len() < 1e-12);
        c.set_view(StdView::Trimetric);
        assert!((c.right.len() - 1.0).abs() < 1e-12 && c.right.dot(c.up).abs() < 1e-12);
    }
    #[test]
    fn picking_finds_the_face_edge_and_vertex_under_the_pointer() {
        let m = Mesh::cuboid(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0));
        let t = topology(&m);
        let mut c = Camera::default();
        c.set_view(StdView::Front);
        c.fit(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0), 200, 200);
        let (x, y, _) = c.project(v3(5.0, 0.0, 5.0), 200, 200).unwrap();
        let p = pick(&c, 200, 200, &[(&m, &t)], x, y, 4.0).unwrap();
        let Element::Face(f) = p.element else {
            panic!("{p:?}")
        };
        assert!(
            crate::document::face_normal(&m, &t, f).y < -0.9,
            "the front face"
        );
        let (x, y, _) = c.project(v3(5.0, 0.0, 10.0), 200, 200).unwrap();
        assert!(matches!(
            pick(&c, 200, 200, &[(&m, &t)], x, y + 1.0, 4.0)
                .unwrap()
                .element,
            Element::Edge(_)
        ));
        let (x, y, _) = c.project(v3(10.0, 0.0, 10.0), 200, 200).unwrap();
        assert!(matches!(
            pick(&c, 200, 200, &[(&m, &t)], x, y, 4.0).unwrap().element,
            Element::Vertex(_)
        ));
        assert!(pick(&c, 200, 200, &[(&m, &t)], 2.0, 2.0, 4.0).is_none());
    }
    #[test]
    fn rendering_is_deterministic_and_draws_the_shape() {
        let m = Mesh::cuboid(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0));
        let t = topology(&m);
        let mut c = Camera::default();
        c.fit(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0), 64, 48);
        let style = Style {
            background_top: Rgb(255, 255, 255),
            background_bottom: Rgb(255, 255, 255),
        };
        let d = || Drawable {
            mesh: &m,
            topo: &t,
            color: Rgb(173, 181, 189),
            line: Rgb(25, 25, 25),
            face_colors: vec![],
            edge_colors: vec![],
            vertex_colors: vec![],
            transparency: 0,
            show_edges: true,
            line_width: 2.0,
        };
        let a = render(&c, 64, 48, &[d()], &[], &style);
        let b = render(&c, 64, 48, &[d()], &[], &style);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64 * 48 * 4);
        let center = (24 * 64 + 32) * 4;
        assert_ne!(
            &a[center..center + 3],
            &[255, 255, 255],
            "the cube covers the centre"
        );
        assert_eq!(&a[0..3], &[255, 255, 255]);
        // Half transparent: the white background shows through, lightening the face.
        let see = render(
            &c,
            64,
            48,
            &[Drawable {
                transparency: 50,
                ..d()
            }],
            &[],
            &style,
        );
        let ink = |px: &[u8]| px.iter().map(|&v| u64::from(255 - v)).sum::<u64>();
        assert!(ink(&see) < ink(&a) && ink(&see) > 0);
        // Fully transparent and without edges: nothing is drawn.
        let none = render(
            &c,
            64,
            48,
            &[Drawable {
                transparency: 100,
                show_edges: false,
                ..d()
            }],
            &[],
            &style,
        );
        assert!(none.chunks(4).all(|p| p[..3] == [255, 255, 255]));
    }
    #[test]
    fn the_cube_has_twenty_six_facets_and_svg_hides_the_back() {
        assert_eq!(nav_cube().len(), 26);
        let m = Mesh::cuboid(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0));
        let t = topology(&m);
        let mut c = Camera::default();
        c.set_view(StdView::Isometric);
        let s = svg(&c, &[(&m, &t)], true);
        assert!(s.starts_with("<?xml"));
        // Nine of the cube's twelve edges face the viewer in an isometric view.
        assert!(s.matches("<polyline").count() >= 9);
        assert!(s.contains("id=\"hidden\""));
    }
}
