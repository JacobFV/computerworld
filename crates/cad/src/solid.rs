//! Faces, edges and vertices recovered from a triangle mesh's surface tags, for shapes
//! that have no B-rep behind them (an imported STL or OBJ). Everything Part Design
//! builds carries its exact boundary instead: see [`crate::brep`].
use crate::math::{Frame, V3};
use crate::mesh::{FixedMap, Mesh, Surface};
use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v3;
    #[test]
    fn a_cuboid_mesh_gives_six_faces_twelve_edges_and_eight_vertices() {
        let m = Mesh::cuboid(v3(0.0, 0.0, 0.0), v3(2.0, 3.0, 4.0));
        let t = topology(&m);
        assert_eq!((t.faces.len(), t.edges.len(), t.vertices.len()), (6, 12, 8));
        assert!(t.edges.iter().all(|e| e.kind == EdgeKind::Line));
        let top = (0..t.faces.len())
            .find(|i| matches!(m.surfaces[t.faces[*i].surface as usize], Surface::Plane { normal, .. } if normal.z > 0.9))
            .unwrap();
        assert!((t.faces[top].area - 6.0).abs() < 1e-12);
        assert!(face_frame(&m, &t, top).is_some());
    }
}
