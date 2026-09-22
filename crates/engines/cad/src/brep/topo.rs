//! Boundary representation: vertices, edges on exact curves, faces on exact surfaces
//! bounded by loops of oriented edges (coedges).
//!
//! Conventions follow OpenCascade's: a face's loops run counter-clockwise seen from
//! outside the solid (the face's material on the left), a closed periodic face (a full
//! cylinder) is cut open by a *seam* edge that its loop uses twice, once each way, and a
//! face that pinches to a point (a sphere at its poles) has a *degenerate* edge there.
use super::geom::{Curve, Surface};
use crate::math::{Xform, V3};
use serde::{Deserialize, Serialize};

/// Model tolerance: points closer than this are one point.
pub const TOL: f64 = 1e-7;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub p: V3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub curve: Curve,
    pub t0: f64,
    pub t1: f64,
    pub v0: usize,
    pub v1: usize,
    /// A point edge (a sphere's pole): no length, used by one face only.
    #[serde(default)]
    pub degenerate: bool,
}
impl Edge {
    pub fn closed(&self) -> bool {
        self.v0 == self.v1 && !self.degenerate
    }
    pub fn point(&self, t: f64) -> V3 {
        self.curve.eval(t)
    }
    pub fn mid(&self) -> V3 {
        self.curve.eval((self.t0 + self.t1) / 2.0)
    }
    pub fn start(&self) -> V3 {
        self.curve.eval(self.t0)
    }
    pub fn end(&self) -> V3 {
        self.curve.eval(self.t1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coedge {
    pub edge: usize,
    /// Traversed from `v1` to `v0`.
    pub rev: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub surface: Surface,
    pub loops: Vec<Vec<Coedge>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub min: V3,
    pub max: V3,
}
impl Box3 {
    pub fn empty() -> Box3 {
        Box3 {
            min: V3 {
                x: f64::INFINITY,
                y: f64::INFINITY,
                z: f64::INFINITY,
            },
            max: V3 {
                x: f64::NEG_INFINITY,
                y: f64::NEG_INFINITY,
                z: f64::NEG_INFINITY,
            },
        }
    }
    pub fn add(&mut self, p: V3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    pub fn grow(&self, d: f64) -> Box3 {
        let e = V3 { x: d, y: d, z: d };
        Box3 {
            min: self.min - e,
            max: self.max + e,
        }
    }
    pub fn overlaps(&self, o: &Box3) -> bool {
        self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
            && self.min.z <= o.max.z
            && o.min.z <= self.max.z
    }
    pub fn contains(&self, p: V3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }
    pub fn union(&self, o: &Box3) -> Box3 {
        Box3 {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }
    pub fn diagonal(&self) -> f64 {
        if self.min.x > self.max.x {
            0.0
        } else {
            (self.max - self.min).len()
        }
    }
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
}

impl Solid {
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
    pub fn add_vertex(&mut self, p: V3) -> usize {
        self.vertices.push(Vertex { p });
        self.vertices.len() - 1
    }
    pub fn add_edge(&mut self, curve: Curve, t0: f64, t1: f64, v0: usize, v1: usize) -> usize {
        self.edges.push(Edge {
            curve,
            t0,
            t1,
            v0,
            v1,
            degenerate: false,
        });
        self.edges.len() - 1
    }
    pub fn add_degenerate(&mut self, v: usize) -> usize {
        let p = self.vertices[v].p;
        self.edges.push(Edge {
            curve: Curve::Line { o: p, d: V3::X },
            t0: 0.0,
            t1: 0.0,
            v0: v,
            v1: v,
            degenerate: true,
        });
        self.edges.len() - 1
    }
    /// Start and end vertex of a coedge.
    pub fn co_ends(&self, c: Coedge) -> (usize, usize) {
        let e = &self.edges[c.edge];
        if c.rev {
            (e.v1, e.v0)
        } else {
            (e.v0, e.v1)
        }
    }
    /// Samples of an edge for display and parameter-space work: parameters from `t0` to
    /// `t1`, the same for every face that uses it.
    pub fn edge_params(&self, e: usize) -> Vec<f64> {
        edge_params(&self.edges[e])
    }
    pub fn edge_box(&self, e: usize) -> Box3 {
        let mut b = Box3::empty();
        for t in self.edge_params(e) {
            b.add(self.edges[e].point(t));
        }
        let pad = match self.edges[e].curve {
            Curve::Line { .. } => 0.0,
            _ => b.diagonal() * 0.02,
        };
        b.grow(pad + TOL)
    }
    /// Box of a face's boundary (a curved face may bulge beyond it: see
    /// `uv::face_box`).
    pub fn boundary_box(&self, f: usize) -> Box3 {
        let mut b = Box3::empty();
        for l in &self.faces[f].loops {
            for c in l {
                b = b.union(&self.edge_box(c.edge));
            }
        }
        b
    }
    pub fn bounds(&self) -> Box3 {
        let mut b = Box3::empty();
        for e in 0..self.edges.len() {
            b = b.union(&self.edge_box(e));
        }
        b
    }
    /// Reverse every face (turn the solid inside out).
    pub fn reversed(&self) -> Solid {
        let mut s = self.clone();
        for f in &mut s.faces {
            for l in &mut f.loops {
                l.reverse();
                for c in l.iter_mut() {
                    c.rev = !c.rev;
                }
            }
        }
        s
    }
    pub fn transformed(&self, x: &Xform) -> Solid {
        let mut s = Solid {
            vertices: self
                .vertices
                .iter()
                .map(|v| Vertex { p: x.point(v.p) })
                .collect(),
            edges: self
                .edges
                .iter()
                .map(|e| Edge {
                    curve: e.curve.transformed(x),
                    ..e.clone()
                })
                .collect(),
            faces: self
                .faces
                .iter()
                .map(|f| Face {
                    surface: f.surface.transformed(x),
                    loops: f.loops.clone(),
                })
                .collect(),
        };
        if x.det() < 0.0 {
            s = s.reversed();
        }
        s
    }
    /// Every coedge's edge used exactly twice in opposite directions (seams by one face,
    /// degenerate edges once): the shell is closed and consistently oriented.
    pub fn check(&self) -> Result<(), String> {
        let mut fwd = vec![0usize; self.edges.len()];
        let mut back = vec![0usize; self.edges.len()];
        for f in &self.faces {
            for l in &f.loops {
                if l.is_empty() {
                    return Err("empty loop".into());
                }
                for (i, c) in l.iter().enumerate() {
                    let next = l[(i + 1) % l.len()];
                    if self.co_ends(*c).1 != self.co_ends(next).0 {
                        return Err(format!(
                            "loop broken between edges {} and {}",
                            c.edge, next.edge
                        ));
                    }
                    if c.rev {
                        back[c.edge] += 1;
                    } else {
                        fwd[c.edge] += 1;
                    }
                }
            }
        }
        for (i, e) in self.edges.iter().enumerate() {
            if e.degenerate {
                continue;
            }
            if fwd[i] + back[i] == 0 {
                continue;
            }
            if fwd[i] != 1 || back[i] != 1 {
                return Err(format!(
                    "edge {i} used {} forwards and {} backwards",
                    fwd[i], back[i]
                ));
            }
        }
        Ok(())
    }
    /// Separate closed shells (face sets connected through shared edges).
    pub fn lumps(&self) -> Vec<Vec<usize>> {
        let n = self.faces.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut [usize], mut x: usize) -> usize {
            while p[x] != x {
                p[x] = p[p[x]];
                x = p[x];
            }
            x
        }
        let mut owner: Vec<Option<usize>> = vec![None; self.edges.len()];
        for (fi, f) in self.faces.iter().enumerate() {
            for l in &f.loops {
                for c in l {
                    if self.edges[c.edge].degenerate {
                        continue;
                    }
                    match owner[c.edge] {
                        Some(g) => {
                            let (a, b) = (find(&mut parent, fi), find(&mut parent, g));
                            if a != b {
                                parent[b.max(a)] = a.min(b);
                            }
                        }
                        None => owner[c.edge] = Some(fi),
                    }
                }
            }
        }
        let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
        for f in 0..n {
            let r = find(&mut parent, f);
            match groups.iter_mut().find(|g| g.0 == r) {
                Some(g) => g.1.push(f),
                None => groups.push((r, vec![f])),
            }
        }
        groups.into_iter().map(|g| g.1).collect()
    }
    /// Drop unused vertices and edges and number what remains in OpenCascade's order:
    /// edges and vertices as first met walking faces, loops and coedges in order.
    pub fn renumber(&mut self) {
        let mut emap = vec![usize::MAX; self.edges.len()];
        let mut vmap = vec![usize::MAX; self.vertices.len()];
        let mut edges = Vec::new();
        let mut verts = Vec::new();
        for f in &mut self.faces {
            for l in &mut f.loops {
                for c in l.iter_mut() {
                    if emap[c.edge] == usize::MAX {
                        let e = self.edges[c.edge].clone();
                        emap[c.edge] = edges.len();
                        for v in [e.v0, e.v1] {
                            if vmap[v] == usize::MAX {
                                vmap[v] = verts.len();
                                verts.push(self.vertices[v].clone());
                            }
                        }
                        edges.push(e);
                    }
                    c.edge = emap[c.edge];
                }
            }
        }
        for e in &mut edges {
            e.v0 = vmap[e.v0];
            e.v1 = vmap[e.v1];
        }
        self.edges = edges;
        self.vertices = verts;
    }
    /// Faces using each edge: (face, loop, index) for every coedge.
    pub fn edge_uses(&self) -> Vec<Vec<(usize, usize, usize)>> {
        let mut uses = vec![Vec::new(); self.edges.len()];
        for (fi, f) in self.faces.iter().enumerate() {
            for (li, l) in f.loops.iter().enumerate() {
                for (ci, c) in l.iter().enumerate() {
                    uses[c.edge].push((fi, li, ci));
                }
            }
        }
        uses
    }
    /// The two faces an edge separates (the same face twice for a seam).
    pub fn edge_faces(&self, e: usize) -> Option<(usize, usize)> {
        let mut fs = Vec::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for l in &f.loops {
                for c in l {
                    if c.edge == e {
                        fs.push(fi);
                    }
                }
            }
        }
        match fs.as_slice() {
            [a, b] => Some((*a, *b)),
            [a] => Some((*a, *a)),
            _ => None,
        }
    }
    /// Append another solid's entities (no boolean: the caller knows they are disjoint).
    pub fn append(&mut self, o: &Solid) {
        let (vb, eb) = (self.vertices.len(), self.edges.len());
        self.vertices.extend(o.vertices.iter().cloned());
        for e in &o.edges {
            let mut e = e.clone();
            e.v0 += vb;
            e.v1 += vb;
            self.edges.push(e);
        }
        for f in &o.faces {
            let mut f = f.clone();
            for l in &mut f.loops {
                for c in l.iter_mut() {
                    c.edge += eb;
                }
            }
            self.faces.push(f);
        }
    }
}

/// Sample parameters of an edge: lines are their ends, conics every 1/64 of a turn,
/// other curves finely enough that the chord turns at most as much.
pub fn edge_params(e: &Edge) -> Vec<f64> {
    if e.degenerate {
        return vec![e.t0, e.t1];
    }
    let span = e.t1 - e.t0;
    let n = match &e.curve {
        Curve::Line { .. } => 1,
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            ((span.abs() / crate::math::TAU * 64.0).ceil() as usize).max(2)
        }
        _ => {
            // Total turning of the tangent, estimated on a fine sampling.
            let m = 64;
            let mut turn = 0.0;
            let mut prev: Option<V3> = None;
            for i in 0..=m {
                let t = e.t0 + span * i as f64 / m as f64;
                let d = e.curve.d1(t).1.norm();
                if let Some(p) = prev {
                    turn += crate::math::acos(p.dot(d).clamp(-1.0, 1.0));
                }
                prev = Some(d);
            }
            ((turn / crate::math::TAU * 64.0).ceil() as usize).clamp(8, 256)
        }
    };
    (0..=n)
        .map(|i| {
            if i == n {
                e.t1
            } else {
                e.t0 + span * i as f64 / n as f64
            }
        })
        .collect()
}
