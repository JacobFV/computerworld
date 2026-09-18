//! Building solids from sketch profiles, and recovering faces, edges and vertices from
//! the meshes the booleans produce.
use crate::csg::ear_clip;
use crate::math::{self, v2, Frame, TAU, V2, V3};
use crate::mesh::{FixedMap, Mesh, Surface};
use crate::sketch::profile::{Region, Wire, SEGMENTS};
use crate::sketch::{Geom, Sketch};
use serde::{Deserialize, Serialize};

/// Surface of the side wall a sketch edge sweeps when extruded along `n`.
fn wall_surface(s: &Sketch, geo: i32, frame: &Frame, a: V3, b: V3, n: V3) -> Surface {
    match s.geo(geo).map(|g| &g.geom) {
        Some(Geom::Circle { c, r }) | Some(Geom::Arc { c, r, .. }) => Surface::Cylinder {
            origin: frame.to_world(*c),
            axis: n,
            radius: *r,
        },
        _ => Surface::Plane {
            origin: a,
            normal: (b - a).cross(n).norm(),
        },
    }
}

/// Extrude sketch regions along the frame normal from `z0` to `z1` (`z0 < z1`), or
/// along a custom direction whose normal component spans the same range.
pub fn extrude(s: &Sketch, regions: &[Region], frame: &Frame, z0: f64, z1: f64) -> Mesh {
    let n = frame.z;
    let mut m = Mesh::default();
    if z1 - z0 <= 1e-9 {
        return m;
    }
    let mut surface_of: FixedMap<i32, u32> = FixedMap::default();
    let bottom_s = m.surfaces.len() as u32;
    m.surfaces.push(Surface::Plane {
        origin: frame.origin + n * z0,
        normal: -n,
    });
    let top_s = m.surfaces.len() as u32;
    m.surfaces.push(Surface::Plane {
        origin: frame.origin + n * z1,
        normal: n,
    });
    for region in regions {
        let wires: Vec<&Wire> = std::iter::once(&region.outer)
            .chain(region.holes.iter())
            .collect();
        let mut bottoms: Vec<Vec<u32>> = Vec::new();
        let mut tops: Vec<Vec<u32>> = Vec::new();
        for w in &wires {
            let mut bi = Vec::new();
            let mut ti = Vec::new();
            for p in &w.pts {
                let base = frame.to_world(*p);
                bi.push(m.verts.len() as u32);
                m.verts.push(base + n * z0);
                ti.push(m.verts.len() as u32);
                m.verts.push(base + n * z1);
            }
            let k = w.pts.len();
            for i in 0..k {
                let j = (i + 1) % k;
                let geo = w.edge_geo[i];
                let (a, b) = (m.verts[bi[i] as usize], m.verts[bi[j] as usize]);
                let surf = match s.geo(geo).map(|g| g.geom.is_curve()) {
                    Some(true) => *surface_of.entry(geo).or_insert_with(|| {
                        m.surfaces.push(wall_surface(s, geo, frame, a, b, n));
                        m.surfaces.len() as u32 - 1
                    }),
                    _ => {
                        m.surfaces.push(wall_surface(s, geo, frame, a, b, n));
                        m.surfaces.len() as u32 - 1
                    }
                };
                m.tris.push([bi[i], bi[j], ti[j]]);
                m.tris.push([bi[i], ti[j], ti[i]]);
                m.tri_surface.push(surf);
                m.tri_surface.push(surf);
            }
            bottoms.push(bi);
            tops.push(ti);
        }
        // Caps: the top as drawn, the bottom reversed.
        for t in ear_clip(&m.verts, &tops[0], &tops[1..], n) {
            m.tris.push(t);
            m.tri_surface.push(top_s);
        }
        let rev = |l: &Vec<u32>| l.iter().rev().copied().collect::<Vec<u32>>();
        let b_outer = rev(&bottoms[0]);
        let b_holes: Vec<Vec<u32>> = bottoms[1..].iter().map(rev).collect();
        for t in ear_clip(&m.verts, &b_outer, &b_holes, -n) {
            m.tris.push(t);
            m.tri_surface.push(bottom_s);
        }
    }
    m.compact();
    m
}

/// Surface swept by the profile segment `a → b` (in the rz half-plane) revolving about
/// the axis.
fn revolved_surface(
    s: &Sketch,
    geo: i32,
    frame: &Frame,
    axis_o: V3,
    axis_d: V3,
    a: V3,
    b: V3,
) -> Surface {
    let radial = |p: V3| {
        let d = p - axis_o;
        (d - axis_d * d.dot(axis_d)).len()
    };
    match s.geo(geo).map(|g| &g.geom) {
        Some(Geom::Circle { c, r }) | Some(Geom::Arc { c, r, .. }) => Surface::Revolved {
            origin: axis_o,
            axis: axis_d,
            center: frame.to_world(*c),
            radius: *r,
        },
        _ => {
            let (ra, rb) = (radial(a), radial(b));
            let (za, zb) = ((a - axis_o).dot(axis_d), (b - axis_o).dot(axis_d));
            if (ra - rb).abs() < 1e-9 {
                Surface::Cylinder {
                    origin: axis_o,
                    axis: axis_d,
                    radius: ra,
                }
            } else if (za - zb).abs() < 1e-9 {
                Surface::Plane {
                    origin: axis_o + axis_d * za,
                    normal: axis_d,
                }
            } else {
                Surface::Cone {
                    origin: axis_o,
                    axis: axis_d,
                }
            }
        }
    }
}

/// Revolve sketch regions about an axis (a point and direction in world space) by
/// `angle` radians, starting at `start` radians from the sketch plane.
pub fn revolve(
    s: &Sketch,
    regions: &[Region],
    frame: &Frame,
    axis_o: V3,
    axis_d: V3,
    start: f64,
    angle: f64,
) -> Result<Mesh, String> {
    let axis_d = axis_d.norm();
    if angle.abs() < 1e-9 {
        return Err("The revolution angle must not be zero".into());
    }
    let full = angle.abs() >= TAU - 1e-9;
    let steps = (((angle.abs() / TAU) * SEGMENTS as f64).ceil() as usize).max(3);
    let mut m = Mesh::default();
    let dist = |p: V3| {
        let d = p - axis_o;
        (d - axis_d * d.dot(axis_d)).len()
    };
    // Which side of the axis the profile lies on, within the sketch plane.
    let side_dir = axis_d.cross(frame.z);
    let mut side = 0.0f64;
    for region in regions {
        for w in std::iter::once(&region.outer).chain(region.holes.iter()) {
            for p in &w.pts {
                let q = frame.to_world(*p);
                let sd = (q - axis_o).dot(side_dir);
                if sd.abs() > 1e-7 {
                    if side != 0.0 && sd.signum() != side {
                        return Err("The profile crosses the revolution axis".into());
                    }
                    side = sd.signum();
                }
            }
        }
    }
    if side == 0.0 {
        return Err("The profile lies on the revolution axis".into());
    }
    let rot = |p: V3, k: usize| {
        let t = start + angle * k as f64 / steps as f64;
        crate::math::Xform::rotate(axis_o, axis_d, t).point(p)
    };
    let mut surface_of: FixedMap<i32, u32> = FixedMap::default();
    for region in regions {
        let wires: Vec<&Wire> = std::iter::once(&region.outer)
            .chain(region.holes.iter())
            .collect();
        // ring[w][i][k]: vertex of profile point i at step k (shared on the axis).
        let mut rings: Vec<Vec<Vec<u32>>> = Vec::new();
        for w in &wires {
            let mut ring = Vec::new();
            for p in &w.pts {
                let q = frame.to_world(*p);
                let columns = if full { steps } else { steps + 1 };
                if dist(q) < 1e-9 {
                    let i = m.verts.len() as u32;
                    m.verts.push(q);
                    ring.push(vec![i; columns]);
                } else {
                    let mut col = Vec::with_capacity(columns);
                    for k in 0..columns {
                        col.push(m.verts.len() as u32);
                        m.verts.push(rot(q, k));
                    }
                    ring.push(col);
                }
            }
            rings.push(ring);
        }
        for (wi, w) in wires.iter().enumerate() {
            let kpts = w.pts.len();
            for i in 0..kpts {
                let j = (i + 1) % kpts;
                let geo = w.edge_geo[i];
                let (a, b) = (
                    m.verts[rings[wi][i][0] as usize],
                    m.verts[rings[wi][j][0] as usize],
                );
                let surf = match s.geo(geo).map(|g| g.geom.is_curve()) {
                    Some(true) => *surface_of.entry(geo).or_insert_with(|| {
                        m.surfaces
                            .push(revolved_surface(s, geo, frame, axis_o, axis_d, a, b));
                        m.surfaces.len() as u32 - 1
                    }),
                    _ => {
                        m.surfaces
                            .push(revolved_surface(s, geo, frame, axis_o, axis_d, a, b));
                        m.surfaces.len() as u32 - 1
                    }
                };
                for k in 0..steps {
                    let k2 = if full { (k + 1) % steps } else { k + 1 };
                    let (p0, p1) = (rings[wi][i][k], rings[wi][j][k]);
                    let (q0, q1) = (rings[wi][i][k2], rings[wi][j][k2]);
                    for t in [[p0, p1, q1], [p0, q1, q0]] {
                        if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                            m.tris.push(t);
                            m.tri_surface.push(surf);
                        }
                    }
                }
            }
        }
        if !full {
            let n0 = frame.z;
            let s0 = m.surfaces.len() as u32;
            m.surfaces.push(Surface::Plane {
                origin: rot(frame.origin, 0),
                normal: V3::ZERO,
            });
            let s1 = m.surfaces.len() as u32;
            m.surfaces.push(Surface::Plane {
                origin: rot(frame.origin, steps),
                normal: V3::ZERO,
            });
            let at = |k: usize| -> (Vec<u32>, Vec<Vec<u32>>) {
                let o: Vec<u32> = rings[0].iter().map(|c| c[k]).collect();
                let h: Vec<Vec<u32>> = rings[1..]
                    .iter()
                    .map(|r| r.iter().map(|c| c[k]).collect())
                    .collect();
                (o, h)
            };
            let (o, h) = at(0);
            let rev = |l: &Vec<u32>| l.iter().rev().copied().collect::<Vec<u32>>();
            let n_end = crate::math::Xform::rotate(axis_o, axis_d, start + angle).dir(n0);
            let n_start = crate::math::Xform::rotate(axis_o, axis_d, start).dir(n0);
            for t in ear_clip(
                &m.verts,
                &rev(&o),
                &h.iter().map(rev).collect::<Vec<_>>(),
                -n_start,
            ) {
                m.tris.push(t);
                m.tri_surface.push(s0);
            }
            let (o, h) = at(steps);
            for t in ear_clip(&m.verts, &o, &h, n_end) {
                m.tris.push(t);
                m.tri_surface.push(s1);
            }
        }
    }
    // The winding above is outward for one sense of rotation; flip for the other.
    if m.volume() < 0.0 {
        for t in &mut m.tris {
            t.swap(1, 2);
        }
    }
    fix_plane_normals(&mut m);
    m.compact();
    Ok(m)
}

/// Make every plane surface's normal agree with the triangles lying on it.
pub fn fix_plane_normals(m: &mut Mesh) {
    let mut best: Vec<(f64, V3)> = vec![(0.0, V3::ZERO); m.surfaces.len()];
    for i in 0..m.tris.len() {
        let [a, b, c] = m.tri(i);
        let cr = (b - a).cross(c - a);
        let area = cr.len();
        let s = m.tri_surface[i] as usize;
        if area > best[s].0 {
            best[s] = (area, cr.norm());
        }
    }
    for (s, surf) in m.surfaces.iter_mut().enumerate() {
        if let Surface::Plane { normal, .. } = surf {
            let tri_n = best[s].1;
            if *normal == V3::ZERO || normal.dot(tri_n) < 0.0 {
                *normal = if normal.len() > 0.5 { -*normal } else { tri_n };
            }
        }
    }
}

/// A cylinder (for holes and tests): radius `r` around `axis` from `base`, height `h`.
pub fn cylinder(base: V3, axis: V3, r: f64, h: f64) -> Mesh {
    let frame = Frame::from_normal(base, axis, axis.any_perp());
    let mut s = Sketch::default();
    s.add_geo(Geom::Circle { c: V2::ZERO, r }, false);
    let regions = crate::sketch::profile::regions(&s).expect("a circle is a closed profile");
    extrude(&s, &regions, &frame, 0.0, h)
}

/// A closed polygon as a one-region profile. Consecutive points on the circle `arc`
/// become pieces of one arc geometry, so the solid built from it knows that face is
/// round; every other segment is its own line.
pub fn polygon_region(poly: &[V2], arc: Option<(V2, f64)>) -> (Sketch, Region) {
    let mut s = Sketch::default();
    let mut w = Wire {
        pts: poly.to_vec(),
        edge_geo: Vec::new(),
    };
    let arc_id = arc.map(|(c, r)| {
        s.add_geo(
            Geom::Arc {
                c,
                r,
                start: 0.0,
                end: 1.0,
            },
            true,
        )
    });
    let n = poly.len();
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let curved = arc.is_some_and(|(c, r)| {
            ((p - c).len() - r).abs() < 1e-9 * r.max(1.0)
                && ((q - c).len() - r).abs() < 1e-9 * r.max(1.0)
        });
        let id = match (curved, arc_id) {
            (true, Some(id)) => id,
            _ => s.add_geo(Geom::Line { a: p, b: q }, true),
        };
        w.edge_geo.push(id);
    }
    if crate::sketch::profile::signed_area(&w.pts) < 0.0 {
        w.reverse();
    }
    (
        s,
        Region {
            outer: w,
            holes: vec![],
        },
    )
}

/// A polygon in the (radius, height) half-plane revolved a full turn about the axis
/// through `base` along `axis` — used for holes and circular-edge fillets.
pub fn revolve_rz(base: V3, axis: V3, rz: &[V2], arc: Option<(V2, f64)>) -> Result<Mesh, String> {
    let y = axis.norm();
    let x = y.any_perp();
    let frame = Frame {
        origin: base,
        x,
        y,
        z: x.cross(y).norm(),
    };
    let (s, region) = polygon_region(rz, arc);
    revolve(&s, &[region], &frame, base, y, 0.0, TAU)
}

// ---------------------------------------------------------------------------------
// Topology: faces, edges and vertices recovered from surface tags.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    Line,
    Circle,
    Arc,
    Curve,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub surface: u32,
    pub tris: Vec<usize>,
    pub area: f64,
    pub centroid: V3,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    /// Mesh vertex indices along the edge, first to last (first == last when closed).
    pub verts: Vec<u32>,
    pub faces: (usize, usize),
    pub kind: EdgeKind,
    pub length: f64,
    /// Centre and radius, for circles and arcs.
    pub circle: Option<(V3, f64, V3)>,
}
impl Edge {
    pub fn closed(&self) -> bool {
        self.verts.first() == self.verts.last()
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Topology {
    pub faces: Vec<Face>,
    pub edges: Vec<Edge>,
    /// Mesh vertex index of each topological vertex.
    pub vertices: Vec<u32>,
    /// Face of every triangle.
    pub tri_face: Vec<usize>,
}

pub fn topology(m: &Mesh) -> Topology {
    let n = m.tris.len();
    let mut owner: FixedMap<(u32, u32), usize> = FixedMap::default();
    for (i, t) in m.tris.iter().enumerate() {
        for k in 0..3 {
            owner.insert((t[k], t[(k + 1) % 3]), i);
        }
    }
    let mut tri_face = vec![usize::MAX; n];
    let mut faces: Vec<Face> = Vec::new();
    for seed in 0..n {
        if tri_face[seed] != usize::MAX {
            continue;
        }
        let f = faces.len();
        let s = m.tri_surface[seed];
        let mut members = vec![seed];
        tri_face[seed] = f;
        let mut at = 0;
        while at < members.len() {
            let t = m.tris[members[at]];
            at += 1;
            for k in 0..3 {
                if let Some(&nb) = owner.get(&(t[(k + 1) % 3], t[k])) {
                    if tri_face[nb] == usize::MAX && m.tri_surface[nb] == s {
                        tri_face[nb] = f;
                        members.push(nb);
                    }
                }
            }
        }
        let mut area = 0.0;
        let mut acc = V3::ZERO;
        for &t in &members {
            let a = m.tri_area(t);
            let [p, q, r] = m.tri(t);
            area += a;
            acc += (p + q + r) * (a / 3.0);
        }
        faces.push(Face {
            surface: s,
            tris: members,
            area,
            centroid: if area > 0.0 { acc / area } else { V3::ZERO },
        });
    }
    // Feature mesh edges: between two different faces (or with no partner).
    let mut feature: std::collections::BTreeMap<(u32, u32), (usize, usize)> =
        std::collections::BTreeMap::new();
    for (i, t) in m.tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let other = owner.get(&(b, a)).map(|o| tri_face[*o]);
            let f = tri_face[i];
            if other != Some(f) {
                let key = (a.min(b), a.max(b));
                let pair = match other {
                    Some(g) => (f.min(g), f.max(g)),
                    None => (f, f),
                };
                feature.insert(key, pair);
            }
        }
    }
    // Adjacency of feature edges at each vertex.
    let mut at_vertex: std::collections::BTreeMap<u32, Vec<(u32, u32)>> =
        std::collections::BTreeMap::new();
    for key in feature.keys() {
        at_vertex.entry(key.0).or_default().push(*key);
        at_vertex.entry(key.1).or_default().push(*key);
    }
    let is_corner = |v: u32| -> bool {
        let list = &at_vertex[&v];
        list.len() != 2 || feature[&list[0]] != feature[&list[1]]
    };
    let mut used: std::collections::BTreeSet<(u32, u32)> = std::collections::BTreeSet::new();
    let mut edges: Vec<Edge> = Vec::new();
    let walk = |start: u32,
                first: (u32, u32),
                used: &mut std::collections::BTreeSet<(u32, u32)>|
     -> Vec<u32> {
        let mut chain = vec![start];
        let mut cur = start;
        let mut e = first;
        loop {
            used.insert(e);
            let next = if e.0 == cur { e.1 } else { e.0 };
            chain.push(next);
            cur = next;
            if cur == start || is_corner(cur) {
                break;
            }
            let Some(ne) = at_vertex[&cur].iter().find(|k| !used.contains(k)).copied() else {
                break;
            };
            e = ne;
        }
        chain
    };
    // Open chains start at corners; what remains are closed loops.
    let corners: Vec<u32> = at_vertex
        .keys()
        .copied()
        .filter(|v| is_corner(*v))
        .collect();
    for &c in &corners {
        for e in at_vertex[&c].clone() {
            if used.contains(&e) {
                continue;
            }
            let chain = walk(c, e, &mut used);
            edges.push(make_edge(m, chain, feature[&e]));
        }
    }
    for e in feature.keys().copied().collect::<Vec<_>>() {
        if used.contains(&e) {
            continue;
        }
        let chain = walk(e.0, e, &mut used);
        edges.push(make_edge(m, chain, feature[&e]));
    }
    let mut vertices: Vec<u32> = Vec::new();
    for e in &edges {
        if !e.closed() {
            for v in [e.verts[0], *e.verts.last().unwrap()] {
                if !vertices.contains(&v) {
                    vertices.push(v);
                }
            }
        }
    }
    Topology {
        faces,
        edges,
        vertices,
        tri_face,
    }
}

fn make_edge(m: &Mesh, verts: Vec<u32>, faces: (usize, usize)) -> Edge {
    let pts: Vec<V3> = verts.iter().map(|v| m.verts[*v as usize]).collect();
    let length: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let closed = verts.first() == verts.last();
    let (a, b) = (pts[0], *pts.last().unwrap());
    let straight = !closed && {
        let d = b - a;
        let l = d.len();
        l > 0.0 && pts.iter().all(|p| (*p - a).cross(d).len() / l < 1e-6)
    };
    if straight {
        return Edge {
            verts,
            faces,
            kind: EdgeKind::Line,
            length,
            circle: None,
        };
    }
    // A circle through three well-spread points that every point lies on.
    let circle = if pts.len() >= 3 {
        let (p0, p1, p2) = (pts[0], pts[pts.len() / 3], pts[2 * pts.len() / 3]);
        circle_through(p0, p1, p2).filter(|(c, r, n)| {
            pts.iter().all(|p| {
                ((*p - *c).len() - r).abs() < 1e-6 * r.max(1.0)
                    && (*p - *c).dot(*n).abs() < 1e-6 * r.max(1.0)
            })
        })
    } else {
        None
    };
    Edge {
        verts,
        faces,
        kind: match (circle.is_some(), closed) {
            (true, true) => EdgeKind::Circle,
            (true, false) => EdgeKind::Arc,
            _ => EdgeKind::Curve,
        },
        length,
        circle,
    }
}

/// Centre, radius and plane normal of the circle through three points.
pub fn circle_through(a: V3, b: V3, c: V3) -> Option<(V3, f64, V3)> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let n2 = n.len2();
    if n2 < 1e-18 {
        return None;
    }
    let center = a + (n.cross(ab) * ac.len2() + ac.cross(n) * ab.len2()) / (2.0 * n2);
    Some((center, center.dist(a), n.norm()))
}

/// Where a sketch on a planar face sits: the face's plane, origin at the world origin's
/// projection onto it, x along the world axis most in-plane. `None` for a curved face.
pub fn face_frame(m: &Mesh, t: &Topology, face: usize) -> Option<Frame> {
    let f = t.faces.get(face)?;
    let Surface::Plane { origin, normal } = &m.surfaces[f.surface as usize] else {
        return None;
    };
    let n = normal.norm();
    let hint = if n.x.abs() > 0.9 { V3::Y } else { V3::X };
    Some(Frame::from_normal(n * origin.dot(n), n, hint))
}

// ---------------------------------------------------------------------------------
// Dress-up tools: the volume a fillet or chamfer removes from (or adds to) an edge.

/// How an edge is rounded or bevelled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dress {
    Fillet(f64),
    Chamfer(f64),
}

/// The third corner of the triangle of face `f` on mesh edge `a`–`b`.
fn third_corner(m: &Mesh, t: &Topology, f: usize, a: u32, b: u32) -> Option<(usize, V3)> {
    t.faces[f].tris.iter().find_map(|&tri| {
        let v = m.tris[tri];
        (v.contains(&a) && v.contains(&b)).then(|| {
            let k = v
                .iter()
                .find(|x| **x != a && **x != b)
                .copied()
                .unwrap_or(a);
            (tri, m.verts[k as usize])
        })
    })
}

/// A tool solid for one edge, and whether it is cut away (a convex edge) rather than
/// added (a concave one). Straight edges between two planes are rounded by an
/// extruded section; circular edges between a plane and a coaxial cylinder by a
/// revolved one.
pub fn dress_tool(
    m: &Mesh,
    t: &Topology,
    edge: usize,
    dress: Dress,
) -> Result<(Mesh, bool), String> {
    let e = t.edges.get(edge).ok_or("no such edge")?;
    let (fa, fb) = e.faces;
    if fa == fb || e.verts.len() < 2 {
        return Err("the edge is on the boundary of an open mesh".into());
    }
    let size = match dress {
        Dress::Fillet(r) | Dress::Chamfer(r) => r,
    };
    if size.is_nan() || size <= 0.0 {
        return Err("the size must be positive".into());
    }
    let (v0, v1) = (e.verts[0], e.verts[1]);
    let p = m.verts[v0 as usize];
    // Unit direction from the edge into face `f`, perpendicular to the edge.
    let into = |f: usize, perp: V3| -> Result<V3, String> {
        let (_, q) = third_corner(m, t, f, v0, v1).ok_or("the edge's faces are not adjacent")?;
        Ok(if (q - p).dot(perp) < 0.0 { -perp } else { perp })
    };
    let sa = &m.surfaces[t.faces[fa].surface as usize];
    let sb = &m.surfaces[t.faces[fb].surface as usize];
    let margin = size * 0.25 + 0.05;
    match (e.kind, sa, sb) {
        (EdgeKind::Line, Surface::Plane { normal: n1, .. }, Surface::Plane { normal: n2, .. }) => {
            let p1 = m.verts[*e.verts.last().unwrap() as usize];
            let d = (p1 - p).norm();
            let u1 = into(fa, n1.cross(d).norm())?;
            let u2 = into(fb, n2.cross(d).norm())?;
            let convex = u1.dot(*n2) < 0.0;
            let ex = u1;
            let mut ey = d.cross(u1).norm();
            if ey.dot(u2) < 0.0 {
                ey = -ey;
            }
            let to2 = |v: V3| v2(v.dot(ex), v.dot(ey));
            let sgn = if convex { 1.0 } else { -1.0 };
            let (poly, arc) = section(to2(u1), to2(u2), to2(*n1) * sgn, to2(*n2) * sgn, dress, margin)?;
            let frame = Frame {
                origin: p,
                x: ex,
                y: ey,
                z: ex.cross(ey).norm(),
            };
            let len = p.dist(p1);
            let (s, region) = polygon_region(&poly, arc);
            let (z0, z1) = if frame.z.dot(d) > 0.0 {
                (-margin, len + margin)
            } else {
                (-len - margin, margin)
            };
            Ok((extrude(&s, &[region], &frame, z0, z1), convex))
        }
        (EdgeKind::Circle | EdgeKind::Arc, Surface::Plane { normal, .. }, Surface::Cylinder { origin, axis, radius })
        | (EdgeKind::Circle | EdgeKind::Arc, Surface::Cylinder { origin, axis, radius }, Surface::Plane { normal, .. }) => {
            if normal.cross(*axis).len() > 1e-6 {
                return Err("the cylinder is not perpendicular to the face".into());
            }
            let (plane_face, cyl_face) = if matches!(sa, Surface::Plane { .. }) { (fa, fb) } else { (fb, fa) };
            let ax = axis.norm();
            let foot = *origin + ax * (p - *origin).dot(ax);
            let radial = (p - foot).norm();
            let z0 = (p - *origin).dot(ax);
            let u_plane = into(plane_face, radial)?;
            let u_cyl = into(cyl_face, ax)?;
            let plane_out = if normal.dot(ax) > 0.0 { ax } else { -ax };
            // The cylinder's outward side at the edge, from the triangle there.
            let (tri, _) = third_corner(m, t, cyl_face, v0, v1).ok_or("the edge's faces are not adjacent")?;
            let cyl_out = if m.tri_normal(tri).dot(radial) < 0.0 { -radial } else { radial };
            let convex = u_plane.dot(cyl_out) < 0.0;
            let r2 = |v: V3| v2(v.dot(radial), v.dot(ax));
            let sgn = if convex { 1.0 } else { -1.0 };
            let (poly, arc) = section(r2(u_plane), r2(u_cyl), r2(plane_out) * sgn, r2(cyl_out) * sgn, dress, margin)?;
            let at = v2(*radius, z0);
            let rz: Vec<V2> = poly.iter().map(|q| *q + at).collect();
            if rz.iter().any(|q| q.x <= 1e-6) {
                return Err("the size is too large for this edge".into());
            }
            let tool = revolve_rz(*origin, ax, &rz, arc.map(|(c, r)| (c + at, r)))?;
            Ok((tool, convex))
        }
        _ => Err("fillets and chamfers apply to straight edges between planar faces and to circular edges between a face and a perpendicular cylinder".into()),
    }
}

/// Cross-section of the material a dress-up changes, around an edge at the origin: the
/// corner between the unit rays `a` and `b` (along the two faces), closed by the fillet
/// arc or chamfer line, with a margin out along `ma` and `mb` (the faces' normals, away
/// from the material that changes) so no tool face is coplanar with the solid's.
/// Returns the polygon and, for a fillet, the arc's centre and radius.
#[allow(clippy::type_complexity)]
fn section(
    a: V2,
    b: V2,
    ma: V2,
    mb: V2,
    dress: Dress,
    margin: f64,
) -> Result<(Vec<V2>, Option<(V2, f64)>), String> {
    let (a, b) = (a.norm(), b.norm());
    let phi = math::acos(a.dot(b).clamp(-1.0, 1.0));
    if !(1e-3..=std::f64::consts::PI - 1e-3).contains(&phi) {
        return Err("the faces at this edge are tangent".into());
    }
    let (mut poly, arc) = match dress {
        Dress::Fillet(r) => {
            let t = r / math::tan(phi / 2.0);
            let c = (a + b).norm() * (r / math::sin(phi / 2.0));
            let (s, e) = ((a * t - c).angle(), (b * t - c).angle());
            let sweep = math::wrap_angle(e - s);
            let n = ((sweep.abs() / TAU * SEGMENTS as f64).ceil() as usize).max(4);
            let mut pts: Vec<V2> = (0..=n)
                .map(|i| c + V2::polar(s + sweep * i as f64 / n as f64, r))
                .collect();
            // Exact tangent points at the ends, so they sit on the faces.
            pts[0] = a * t;
            pts[n] = b * t;
            (pts, Some((c, r)))
        }
        Dress::Chamfer(d) => (vec![a * d, b * d], None),
    };
    let (t1, t2) = (poly[0], *poly.last().unwrap());
    poly.push(t2 + mb.norm() * margin);
    poly.push((ma.norm() + mb.norm()) * margin);
    poly.push(t1 + ma.norm() * margin);
    Ok((poly, arc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{profile, tools};
    fn rect_sketch(w: f64, h: f64) -> Sketch {
        let mut s = Sketch::default();
        tools::rectangle(&mut s, v2(0.0, 0.0), v2(w, h), false).unwrap();
        s
    }
    #[test]
    fn extruding_a_rectangle_makes_a_box() {
        let s = rect_sketch(10.0, 20.0);
        let r = profile::regions(&s).unwrap();
        let m = extrude(&s, &r, &Frame::XY, 0.0, 5.0);
        assert!(m.is_watertight());
        assert!((m.volume() - 1000.0).abs() < 1e-9);
        assert!((m.area() - 2.0 * (200.0 + 50.0 + 100.0)).abs() < 1e-9);
        let t = topology(&m);
        assert_eq!((t.faces.len(), t.edges.len(), t.vertices.len()), (6, 12, 8));
        assert!(t.edges.iter().all(|e| e.kind == EdgeKind::Line));
    }
    #[test]
    fn a_cylinder_has_three_faces_and_two_circular_edges() {
        let m = cylinder(V3::ZERO, V3::Z, 5.0, 10.0);
        assert!(m.is_watertight());
        let n = SEGMENTS as f64;
        let polygon = 0.5 * n * 25.0 * (TAU / n).sin();
        assert!((m.volume() - polygon * 10.0).abs() < 1e-9);
        let t = topology(&m);
        assert_eq!(t.faces.len(), 3);
        assert_eq!(t.edges.len(), 2);
        assert!(t.edges.iter().all(|e| e.kind == EdgeKind::Circle));
    }
    #[test]
    fn revolving_a_rectangle_makes_a_tube() {
        // Rectangle x in [0,10] shifted to radius [5,15] about the sketch's y axis.
        let mut s2 = Sketch::default();
        tools::rectangle(&mut s2, v2(5.0, 0.0), v2(15.0, 20.0), false).unwrap();
        let r2 = profile::regions(&s2).unwrap();
        let m = revolve(&s2, &r2, &Frame::XY, V3::ZERO, V3::Y, 0.0, TAU).unwrap();
        assert!(m.is_watertight(), "{}", m.open_edges());
        let n = SEGMENTS as f64;
        let k = 0.5 * n * (TAU / n).sin();
        let want = k * (225.0 - 25.0) * 20.0;
        assert!((m.volume() - want).abs() < 1e-6, "{} vs {want}", m.volume());
        // Half a turn is half the volume, with two flat caps.
        let half = revolve(
            &s2,
            &r2,
            &Frame::XY,
            V3::ZERO,
            V3::Y,
            0.0,
            std::f64::consts::PI,
        )
        .unwrap();
        assert!(half.is_watertight());
        assert!((half.volume() - want / 2.0).abs() < 1e-6);
    }
}
