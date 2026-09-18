//! Boolean operations on closed meshes.
//!
//! The booleans are Laidlaw/Naylor BSP-tree CSG (the algorithm of csg.js): each operand
//! becomes a BSP tree over its triangles' planes, each clips the other, and the kept
//! fragments are the result. Trees are stored in arenas and walked iteratively, so deep
//! trees cannot overflow the stack (Wasm's is small).
//!
//! Clipping splits polygons, and a split puts new vertices on one polygon's edge that
//! the neighbour across that edge does not have (T-junctions), so raw BSP output is not
//! watertight. [`heal`] makes it so: vertices are welded, every vertex lying on another
//! polygon's edge is inserted into that edge, and each polygon is re-triangulated with
//! ear clipping. Finally coplanar fragments of one face are merged and re-triangulated
//! as one polygon, which keeps repeated booleans from fragmenting faces without end.
use crate::math::{v2, V2, V3};
use crate::mesh::{FixedMap, Mesh, Surface};
use std::collections::BTreeSet;

/// Distance within which a point counts as on a plane.
const EPS: f64 = 1e-6;
/// Distance within which two vertices are one.
const WELD: f64 = 2e-6;

#[derive(Clone, Copy, Debug)]
struct Plane {
    n: V3,
    w: f64,
}
impl Plane {
    fn flip(self) -> Plane {
        Plane {
            n: -self.n,
            w: -self.w,
        }
    }
}

#[derive(Clone, Debug)]
struct Poly {
    v: Vec<V3>,
    plane: Plane,
    surface: u32,
}
impl Poly {
    fn flip(&mut self) {
        self.v.reverse();
        self.plane = self.plane.flip();
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Split `p` by `plane` into the four buckets csg.js uses.
fn split(
    plane: Plane,
    p: Poly,
    co_front: &mut Vec<Poly>,
    co_back: &mut Vec<Poly>,
    front: &mut Vec<Poly>,
    back: &mut Vec<Poly>,
) {
    let mut kind = 0u8;
    let types: Vec<u8> =
        p.v.iter()
            .map(|v| {
                let t = plane.n.dot(*v) - plane.w;
                let k = if t < -EPS {
                    BACK
                } else if t > EPS {
                    FRONT
                } else {
                    COPLANAR
                };
                kind |= k;
                k
            })
            .collect();
    match kind {
        COPLANAR => {
            if plane.n.dot(p.plane.n) > 0.0 {
                co_front.push(p)
            } else {
                co_back.push(p)
            }
        }
        FRONT => front.push(p),
        BACK => back.push(p),
        _ => {
            let mut f = Vec::new();
            let mut b = Vec::new();
            let n = p.v.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (p.v[i], p.v[j]);
                if ti != BACK {
                    f.push(vi);
                }
                if ti != FRONT {
                    b.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let t = (plane.w - plane.n.dot(vi)) / plane.n.dot(vj - vi);
                    let v = vi.lerp(vj, t);
                    f.push(v);
                    b.push(v);
                }
            }
            if f.len() >= 3 {
                front.push(Poly {
                    v: f,
                    plane: p.plane,
                    surface: p.surface,
                });
            }
            if b.len() >= 3 {
                back.push(Poly {
                    v: b,
                    plane: p.plane,
                    surface: p.surface,
                });
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Node {
    plane: Option<Plane>,
    front: Option<usize>,
    back: Option<usize>,
    polys: Vec<Poly>,
}

#[derive(Clone, Debug, Default)]
struct Bsp {
    nodes: Vec<Node>,
}
impl Bsp {
    fn new(polys: Vec<Poly>) -> Bsp {
        let mut t = Bsp {
            nodes: vec![Node::default()],
        };
        t.build(0, polys);
        t
    }
    fn build(&mut self, root: usize, polys: Vec<Poly>) {
        let mut work = vec![(root, polys)];
        while let Some((id, polys)) = work.pop() {
            if polys.is_empty() {
                continue;
            }
            let plane = *self.nodes[id].plane.get_or_insert(polys[0].plane);
            let (mut front, mut back) = (Vec::new(), Vec::new());
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            for p in polys {
                split(plane, p, &mut cf, &mut cb, &mut front, &mut back);
            }
            self.nodes[id].polys.extend(cf);
            self.nodes[id].polys.extend(cb);
            if !front.is_empty() {
                let child = match self.nodes[id].front {
                    Some(c) => c,
                    None => {
                        self.nodes.push(Node::default());
                        let c = self.nodes.len() - 1;
                        self.nodes[id].front = Some(c);
                        c
                    }
                };
                work.push((child, front));
            }
            if !back.is_empty() {
                let child = match self.nodes[id].back {
                    Some(c) => c,
                    None => {
                        self.nodes.push(Node::default());
                        let c = self.nodes.len() - 1;
                        self.nodes[id].back = Some(c);
                        c
                    }
                };
                work.push((child, back));
            }
        }
    }
    fn invert(&mut self) {
        for n in &mut self.nodes {
            for p in &mut n.polys {
                p.flip();
            }
            n.plane = n.plane.map(Plane::flip);
            std::mem::swap(&mut n.front, &mut n.back);
        }
    }
    /// The parts of `polys` outside this tree's solid.
    fn clip_polys(&self, polys: Vec<Poly>) -> Vec<Poly> {
        let mut out = Vec::new();
        let mut work = vec![(0usize, polys)];
        while let Some((id, polys)) = work.pop() {
            let node = &self.nodes[id];
            let Some(plane) = node.plane else {
                out.extend(polys);
                continue;
            };
            let (mut front, mut back) = (Vec::new(), Vec::new());
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            for p in polys {
                split(plane, p, &mut cf, &mut cb, &mut front, &mut back);
            }
            front.extend(cf);
            back.extend(cb);
            match node.front {
                Some(c) => work.push((c, front)),
                None => out.extend(front),
            }
            if let Some(c) = node.back {
                work.push((c, back));
            }
        }
        out
    }
    fn clip_to(&mut self, other: &Bsp) {
        for i in 0..self.nodes.len() {
            let polys = std::mem::take(&mut self.nodes[i].polys);
            self.nodes[i].polys = other.clip_polys(polys);
        }
    }
    fn all(&self) -> Vec<Poly> {
        self.nodes
            .iter()
            .flat_map(|n| n.polys.iter().cloned())
            .collect()
    }
}

/// Merge each planar face's triangles into convex polygons (Hertel–Mehlhorn: drop every
/// diagonal whose removal keeps the union convex). Fewer, larger polygons mean far fewer
/// splits in the BSP trees.
fn convex_pieces(m: &Mesh) -> Vec<(Vec<u32>, u32)> {
    let n = m.tris.len();
    let mut owner: FixedMap<(u32, u32), usize> = FixedMap::default();
    for (i, t) in m.tris.iter().enumerate() {
        for k in 0..3 {
            owner.insert((t[k], t[(k + 1) % 3]), i);
        }
    }
    let mut polys: Vec<Option<Vec<u32>>> = m.tris.iter().map(|t| Some(t.to_vec())).collect();
    let mut id: Vec<usize> = (0..n).collect();
    fn find(id: &mut [usize], mut x: usize) -> usize {
        while id[x] != x {
            id[x] = id[id[x]];
            x = id[x];
        }
        x
    }
    for i in 0..n {
        let s = m.tri_surface[i];
        let normal = match &m.surfaces[s as usize] {
            Surface::Plane { normal, .. } => *normal,
            _ => continue,
        };
        let t = m.tris[i];
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let Some(&j) = owner.get(&(b, a)) else {
                continue;
            };
            if m.tri_surface[j] != s {
                continue;
            }
            let (pi, pj) = (find(&mut id, i), find(&mut id, j));
            if pi == pj {
                continue;
            }
            let (Some(p), Some(q)) = (polys[pi].as_ref(), polys[pj].as_ref()) else {
                continue;
            };
            // The shared edge runs a→b in p and b→a in q.
            let (Some(ia), Some(ib)) = (
                p.iter().position(|v| *v == a),
                q.iter().position(|v| *v == b),
            ) else {
                continue;
            };
            if p[(ia + 1) % p.len()] != b || q[(ib + 1) % q.len()] != a {
                continue;
            }
            let mut merged = Vec::with_capacity(p.len() + q.len() - 2);
            for s in 0..p.len() {
                merged.push(p[(ia + 1 + s) % p.len()]);
                if merged.len() == p.len() {
                    break;
                }
            }
            // merged = b … a (all of p, starting after a). Now q from after a to before b.
            for s in 0..q.len() - 2 {
                merged.push(q[(ib + 2 + s) % q.len()]);
            }
            let pts: Vec<V2> = merged
                .iter()
                .map(|v| project(m.verts[*v as usize], normal))
                .collect();
            let k = pts.len();
            let convex = (0..k).all(|c| {
                let (x, y, z) = (pts[(c + k - 1) % k], pts[c], pts[(c + 1) % k]);
                (y - x).cross(z - y) >= -1e-12 * (y - x).len() * (z - y).len()
            });
            if !convex {
                continue;
            }
            polys[pi] = Some(merged);
            polys[pj] = None;
            id[pj] = pi;
        }
    }
    polys
        .into_iter()
        .enumerate()
        .filter_map(|(i, p)| p.map(|p| (p, m.tri_surface[i])))
        .collect()
}

fn polys_of(m: &Mesh, surface_base: u32) -> Vec<Poly> {
    convex_pieces(m)
        .into_iter()
        .filter_map(|(idx, surface)| {
            let v: Vec<V3> = idx.iter().map(|i| m.verts[*i as usize]).collect();
            // A planar face's triangles all use the face's exact plane, so they
            // classify identically against any splitter.
            let plane = match &m.surfaces[surface as usize] {
                Surface::Plane { origin, normal } => Plane {
                    n: *normal,
                    w: normal.dot(*origin),
                },
                _ => {
                    let n = (v[1] - v[0]).cross(v[2] - v[0]);
                    if n.len() < 1e-18 {
                        return None;
                    }
                    let n = n.norm();
                    Plane { n, w: n.dot(v[0]) }
                }
            };
            Some(Poly {
                v,
                plane,
                surface: surface + surface_base,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Union,
    Difference,
    Intersection,
}

/// `a ∪ b`, `a − b` or `a ∩ b`, returned watertight.
pub fn boolean(a: &Mesh, b: &Mesh, op: Op) -> Mesh {
    if a.is_empty() {
        return match op {
            Op::Union => b.clone(),
            _ => Mesh::default(),
        };
    }
    if b.is_empty() {
        return match op {
            Op::Intersection => Mesh::default(),
            _ => a.clone(),
        };
    }
    // Disjoint bounding boxes need no tree at all.
    if let (Some(ba), Some(bb)) = (a.bounds(), b.bounds()) {
        let apart = ba.max.x < bb.min.x - EPS
            || bb.max.x < ba.min.x - EPS
            || ba.max.y < bb.min.y - EPS
            || bb.max.y < ba.min.y - EPS
            || ba.max.z < bb.min.z - EPS
            || bb.max.z < ba.min.z - EPS;
        if apart {
            return match op {
                Op::Union => {
                    let mut m = a.clone();
                    m.append(b);
                    m.compact();
                    m
                }
                Op::Difference => a.clone(),
                Op::Intersection => Mesh::default(),
            };
        }
    }
    let mut surfaces = a.surfaces.clone();
    surfaces.extend(b.surfaces.iter().cloned());
    let pa = polys_of(a, 0);
    let pb = polys_of(b, a.surfaces.len() as u32);
    let mut ta = Bsp::new(pa);
    let mut tb = Bsp::new(pb);
    match op {
        Op::Union => {
            ta.clip_to(&tb);
            tb.clip_to(&ta);
            tb.invert();
            tb.clip_to(&ta);
            tb.invert();
            let rest = tb.all();
            ta.build(0, rest);
        }
        Op::Difference => {
            ta.invert();
            ta.clip_to(&tb);
            tb.clip_to(&ta);
            tb.invert();
            tb.clip_to(&ta);
            tb.invert();
            let rest = tb.all();
            ta.build(0, rest);
            ta.invert();
        }
        Op::Intersection => {
            ta.invert();
            tb.clip_to(&ta);
            tb.invert();
            ta.clip_to(&tb);
            tb.clip_to(&ta);
            let rest = tb.all();
            ta.build(0, rest);
            ta.invert();
        }
    }
    let polys = ta.all();
    // Cutting b out of a leaves b's faces facing inwards: their planes are flipped.
    heal(polys, surfaces)
}

/// Welded vertex store with a fixed-key spatial hash.
struct Welder {
    verts: Vec<V3>,
    grid: FixedMap<(i64, i64, i64), Vec<u32>>,
}
impl Welder {
    fn cell(p: V3) -> (i64, i64, i64) {
        let s = 1.0 / (WELD * 4.0);
        (
            (p.x * s).floor() as i64,
            (p.y * s).floor() as i64,
            (p.z * s).floor() as i64,
        )
    }
    fn find(&self, p: V3) -> Option<u32> {
        let (cx, cy, cz) = Self::cell(p);
        let mut best: Option<(f64, u32)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = self.grid.get(&(cx + dx, cy + dy, cz + dz)) {
                        for &i in list {
                            let d = self.verts[i as usize].dist(p);
                            if d <= WELD && best.is_none_or(|b| d < b.0 || (d == b.0 && i < b.1)) {
                                best = Some((d, i));
                            }
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }
    fn add(&mut self, p: V3) -> u32 {
        if let Some(i) = self.find(p) {
            return i;
        }
        let i = self.verts.len() as u32;
        self.verts.push(p);
        self.grid.entry(Self::cell(p)).or_default().push(i);
        i
    }
}

/// Turn clipped polygons into a watertight triangle mesh.
fn heal(polys: Vec<Poly>, surfaces: Vec<Surface>) -> Mesh {
    let mut w = Welder {
        verts: Vec::new(),
        grid: FixedMap::default(),
    };
    // 1. Weld, dropping polygons that collapse.
    let mut faces: Vec<(Vec<u32>, Plane, u32)> = Vec::new();
    for p in polys {
        let mut idx: Vec<u32> = Vec::with_capacity(p.v.len());
        for v in &p.v {
            let i = w.add(*v);
            if idx.last() != Some(&i) {
                idx.push(i);
            }
        }
        while idx.len() > 1 && idx.first() == idx.last() {
            idx.pop();
        }
        if idx.len() >= 3 {
            faces.push((idx, p.plane, p.surface));
        }
    }
    // 2. Put every vertex lying on a polygon edge into that edge.
    let grid = EdgeGrid::new(&w.verts);
    for face in &mut faces {
        let n = face.0.len();
        let mut out = Vec::with_capacity(n);
        for k in 0..n {
            let (a, b) = (face.0[k], face.0[(k + 1) % n]);
            out.push(a);
            let (pa, pb) = (w.verts[a as usize], w.verts[b as usize]);
            let mut on: Vec<(f64, u32)> = grid
                .near_segment(pa, pb)
                .into_iter()
                .filter(|&i| i != a && i != b)
                .filter_map(|i| {
                    let p = w.verts[i as usize];
                    let d = pb - pa;
                    let t = (p - pa).dot(d) / d.dot(d);
                    if t <= 0.0 || t >= 1.0 {
                        return None;
                    }
                    ((pa + d * t).dist(p) <= WELD).then_some((t, i))
                })
                .collect();
            on.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            out.extend(on.into_iter().map(|(_, i)| i));
        }
        face.0 = out;
    }
    // A face of the subtracted operand now faces the other way: give it a plane
    // surface whose normal says so.
    let mut surfaces = surfaces;
    let mut flipped: FixedMap<u32, u32> = FixedMap::default();
    for face in &mut faces {
        if let Surface::Plane { origin, normal } = surfaces[face.2 as usize].clone() {
            if normal.dot(face.1.n) < 0.0 {
                let id = *flipped.entry(face.2).or_insert_with(|| {
                    surfaces.push(Surface::Plane {
                        origin,
                        normal: -normal,
                    });
                    surfaces.len() as u32 - 1
                });
                face.2 = id;
            }
        }
    }
    // 3. Triangulate.
    let mut mesh = Mesh {
        verts: w.verts,
        tris: Vec::new(),
        tri_surface: Vec::new(),
        surfaces,
    };
    for (idx, plane, surface) in &faces {
        let mut tris = ear_clip(&mesh.verts, idx, &[], plane.n);
        if !triangulation_ok(&mesh.verts, &tris, idx, plane.n) {
            // A fan from an interior point never leaves a sliver or a vertex on an
            // edge; the point is interior to this polygon alone, so nothing else needs it.
            let c = idx
                .iter()
                .fold(V3::ZERO, |s, i| s + mesh.verts[*i as usize])
                / idx.len() as f64;
            let ci = mesh.verts.len() as u32;
            mesh.verts.push(c);
            tris = (0..idx.len())
                .map(|k| [ci, idx[k], idx[(k + 1) % idx.len()]])
                .filter(|t| t[1] != t[2])
                .collect();
        }
        for t in tris {
            mesh.tris.push(t);
            mesh.tri_surface.push(*surface);
        }
    }
    mesh.compact();
    decimate(&mut mesh);
    mesh.compact();
    refine(&mut mesh);
    mesh.compact();
    mesh
}

/// Uniform grid over vertices for "which vertices lie near this segment" queries.
struct EdgeGrid {
    cell: f64,
    grid: FixedMap<(i64, i64, i64), Vec<u32>>,
}
impl EdgeGrid {
    fn new(verts: &[V3]) -> EdgeGrid {
        let (mut lo, mut hi) = (V3::ZERO, V3::ZERO);
        if let Some(first) = verts.first() {
            lo = *first;
            hi = *first;
            for v in verts {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
        let diag = (hi - lo).len().max(1e-3);
        let cell = diag / 48.0;
        let mut grid: FixedMap<(i64, i64, i64), Vec<u32>> = FixedMap::default();
        for (i, v) in verts.iter().enumerate() {
            grid.entry(Self::key(*v, cell)).or_default().push(i as u32);
        }
        EdgeGrid { cell, grid }
    }
    fn key(p: V3, cell: f64) -> (i64, i64, i64) {
        (
            (p.x / cell).floor() as i64,
            (p.y / cell).floor() as i64,
            (p.z / cell).floor() as i64,
        )
    }
    fn near_segment(&self, a: V3, b: V3) -> Vec<u32> {
        let steps = ((a.dist(b) / (self.cell * 0.5)).ceil() as usize).max(1);
        let mut cells = BTreeSet::new();
        for s in 0..=steps {
            let (cx, cy, cz) = Self::key(a.lerp(b, s as f64 / steps as f64), self.cell);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        cells.insert((cx + dx, cy + dy, cz + dz));
                    }
                }
            }
        }
        let mut out = Vec::new();
        for c in cells {
            if let Some(list) = self.grid.get(&c) {
                out.extend_from_slice(list);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Project onto the plane with normal `n` along its dominant axis, keeping orientation.
fn project(p: V3, n: V3) -> V2 {
    let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
    if az >= ax && az >= ay {
        if n.z > 0.0 {
            v2(p.x, p.y)
        } else {
            v2(p.y, p.x)
        }
    } else if ax >= ay {
        if n.x > 0.0 {
            v2(p.y, p.z)
        } else {
            v2(p.z, p.y)
        }
    } else if n.y > 0.0 {
        v2(p.z, p.x)
    } else {
        v2(p.x, p.z)
    }
}

/// Triangulate a counter-clockwise (seen along `n`) polygon with optional clockwise
/// holes. Holes are bridged into the outer ring first; then ears are clipped, never
/// leaving a zero-area triangle.
pub(crate) fn ear_clip(verts: &[V3], outer: &[u32], holes: &[Vec<u32>], n: V3) -> Vec<[u32; 3]> {
    let pt = |i: u32| project(verts[i as usize], n);
    let mut ring: Vec<u32> = outer.to_vec();
    // Bridge holes, rightmost first (the classic earcut order).
    let mut hs: Vec<&Vec<u32>> = holes.iter().filter(|h| h.len() >= 3).collect();
    hs.sort_by(|a, b| {
        let ma = a.iter().map(|i| pt(*i).x).fold(f64::MIN, f64::max);
        let mb = b.iter().map(|i| pt(*i).x).fold(f64::MIN, f64::max);
        mb.total_cmp(&ma)
    });
    for h in hs {
        let (hi, _) = h
            .iter()
            .enumerate()
            .max_by(|a, b| pt(*a.1).x.total_cmp(&pt(*b.1).x).then(b.0.cmp(&a.0)))
            .unwrap();
        let hp = pt(h[hi]);
        // Nearest ring vertex whose bridge crosses no ring or hole edge.
        let mut cands: Vec<(f64, usize)> = ring
            .iter()
            .enumerate()
            .map(|(k, i)| (pt(*i).dist(hp), k))
            .collect();
        cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let visible = |k: usize| {
            let rp = pt(ring[k]);
            let crosses = |p: V2, q: V2| {
                let (d1, d2) = ((q - p).cross(rp - p), (q - p).cross(hp - p));
                let (d3, d4) = ((hp - rp).cross(p - rp), (hp - rp).cross(q - rp));
                d1 * d2 < 0.0 && d3 * d4 < 0.0
            };
            let rn = ring.len();
            let ring_ok = (0..rn).all(|e| !crosses(pt(ring[e]), pt(ring[(e + 1) % rn])));
            let hn = h.len();
            let hole_ok = (0..hn).all(|e| !crosses(pt(h[e]), pt(h[(e + 1) % hn])));
            ring_ok && hole_ok
        };
        let k = cands
            .iter()
            .map(|c| c.1)
            .find(|k| visible(*k))
            .unwrap_or(cands[0].1);
        // ring[..=k], hole from hi round to hi, back to ring[k], ring[k+1..].
        let mut merged = ring[..=k].to_vec();
        for s in 0..=h.len() {
            merged.push(h[(hi + s) % h.len()]);
        }
        merged.push(ring[k]);
        merged.extend_from_slice(&ring[k + 1..]);
        ring = merged;
    }
    let mut tris = Vec::new();
    let mut guard = 0;
    while ring.len() > 3 && guard < 100_000 {
        guard += 1;
        let m = ring.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (ring[(k + m - 1) % m], ring[k], ring[(k + 1) % m]);
            let (a, b, c) = (pt(ia), pt(ib), pt(ic));
            let area = (b - a).cross(c - a);
            let scale = (b - a).len() * (c - a).len();
            if area <= scale * 1e-12 {
                continue;
            }
            // No other ring vertex may lie inside or on the ear.
            let blocked = ring.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = pt(j);
                let d1 = (b - a).cross(p - a);
                let d2 = (c - b).cross(p - b);
                let d3 = (a - c).cross(p - c);
                d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0
            });
            if blocked {
                continue;
            }
            tris.push([ia, ib, ic]);
            ring.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            // Only collinear or blocked corners remain. A duplicated bridge vertex can
            // block every ear; take the widest convex corner to make progress.
            let mut best: Option<(f64, usize)> = None;
            for k in 0..m {
                let (a, b, c) = (
                    pt(ring[(k + m - 1) % m]),
                    pt(ring[k]),
                    pt(ring[(k + 1) % m]),
                );
                let area = (b - a).cross(c - a);
                if area > 0.0 && best.is_none_or(|x| area > x.0) {
                    best = Some((area, k));
                }
            }
            match best {
                Some((_, k)) => {
                    tris.push([ring[(k + m - 1) % m], ring[k], ring[(k + 1) % m]]);
                    ring.remove(k);
                }
                None => break,
            }
        }
    }
    if ring.len() == 3 {
        let (a, b, c) = (pt(ring[0]), pt(ring[1]), pt(ring[2]));
        if (b - a).cross(c - a) > 0.0 {
            tris.push([ring[0], ring[1], ring[2]]);
            ring.clear();
        }
    }
    // What is left has no area: its points lie along one line, and the triangle that
    // borders that line must be split at each of them or the mesh is left open there.
    for &v in &ring {
        let p = pt(v);
        let hit = tris.iter().enumerate().find_map(|(ti, t)| {
            (0..3).find_map(|k| {
                let (i, j) = (t[k], t[(k + 1) % 3]);
                if i == v || j == v {
                    return None;
                }
                let (a, b) = (pt(i), pt(j));
                let d = b - a;
                let len2 = d.dot(d);
                if len2 == 0.0 {
                    return None;
                }
                let s = (p - a).dot(d) / len2;
                let off = (p - a).cross(d).abs() / len2.sqrt();
                (s > 1e-9 && s < 1.0 - 1e-9 && off <= WELD).then_some((ti, k))
            })
        });
        if let Some((ti, k)) = hit {
            let t = tris[ti];
            let (i, j, o) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
            tris[ti] = [i, v, o];
            tris.push([v, j, o]);
        }
    }
    tris
}

/// Remove vertices that add nothing to the shape, by half-edge collapse: a vertex
/// inside a flat patch, or on a straight crease between two flat patches with its two
/// crease neighbours on either side of it. Collapsing moves no remaining vertex, so the
/// geometry is exactly what it was; the link condition keeps the mesh manifold.
pub fn decimate(m: &mut Mesh) {
    let nv = m.verts.len();
    let mut alive = vec![true; m.tris.len()];
    let mut vt: Vec<Vec<usize>> = vec![Vec::new(); nv];
    for (i, t) in m.tris.iter().enumerate() {
        for &v in t {
            vt[v as usize].push(i);
        }
    }
    let normal = |m: &Mesh, t: [u32; 3]| {
        let (a, b, c) = (
            m.verts[t[0] as usize],
            m.verts[t[1] as usize],
            m.verts[t[2] as usize],
        );
        (b - a).cross(c - a)
    };
    let neighbours = |m: &Mesh, vt: &[Vec<usize>], alive: &[bool], v: u32| -> BTreeSet<u32> {
        vt[v as usize]
            .iter()
            .filter(|t| alive[**t])
            .flat_map(|t| m.tris[*t])
            .filter(|x| *x != v)
            .collect()
    };
    for _pass in 0..4 {
        let mut changed = false;
        for v in 0..nv as u32 {
            let mut tris: Vec<usize> = vt[v as usize]
                .iter()
                .copied()
                .filter(|t| alive[*t])
                .collect();
            tris.sort_unstable();
            tris.dedup();
            if tris.len() < 3 {
                continue;
            }
            // Group the fan by (surface, plane).
            let mut groups: Vec<(u32, V3, Vec<usize>)> = Vec::new();
            for &t in &tris {
                let n = normal(m, m.tris[t]);
                let l = n.len();
                if l == 0.0 {
                    continue;
                }
                let n = n / l;
                let s = m.tri_surface[t];
                match groups
                    .iter_mut()
                    .find(|g| g.0 == s && g.1.dot(n) > 1.0 - 1e-12)
                {
                    Some(g) => g.2.push(t),
                    None => groups.push((s, n, vec![t])),
                }
            }
            let candidates: Vec<u32> = match groups.len() {
                1 => neighbours(m, &vt, &alive, v).into_iter().collect(),
                2 => {
                    // The crease: edges from v whose two triangles lie in different groups.
                    let in_group = |t: usize| groups.iter().position(|g| g.2.contains(&t));
                    let mut crease = BTreeSet::new();
                    for &t in &tris {
                        let tri = m.tris[t];
                        for &u in &tri {
                            if u == v {
                                continue;
                            }
                            let other = tris
                                .iter()
                                .copied()
                                .find(|&o| o != t && m.tris[o].contains(&u));
                            if let Some(o) = other {
                                if in_group(o) != in_group(t) {
                                    crease.insert(u);
                                }
                            }
                        }
                    }
                    let c: Vec<u32> = crease.into_iter().collect();
                    if c.len() != 2 {
                        continue;
                    }
                    let (p, q, x) = (
                        m.verts[c[0] as usize],
                        m.verts[c[1] as usize],
                        m.verts[v as usize],
                    );
                    let d = q - p;
                    let s = (x - p).dot(d) / d.dot(d);
                    if !(s > 0.0 && s < 1.0) || (x - p).cross(d).len() / d.len() > 1e-9 {
                        continue;
                    }
                    c
                }
                _ => continue,
            };
            let ring_v = neighbours(m, &vt, &alive, v);
            for u in candidates {
                // Link condition: v and u share exactly the apexes of their two triangles.
                let shared: Vec<usize> = tris
                    .iter()
                    .copied()
                    .filter(|t| m.tris[*t].contains(&u))
                    .collect();
                if shared.len() != 2 {
                    continue;
                }
                let apexes: BTreeSet<u32> = shared
                    .iter()
                    .flat_map(|t| m.tris[*t])
                    .filter(|x| *x != u && *x != v)
                    .collect();
                let ring_u = neighbours(m, &vt, &alive, u);
                let common: BTreeSet<u32> = ring_v.intersection(&ring_u).copied().collect();
                if common != apexes {
                    continue;
                }
                // No surviving triangle may flip or collapse.
                let ok = tris.iter().filter(|t| !shared.contains(t)).all(|&t| {
                    let before = normal(m, m.tris[t]);
                    let mut moved = m.tris[t];
                    for x in &mut moved {
                        if *x == v {
                            *x = u;
                        }
                    }
                    let after = normal(m, moved);
                    let bl = before.len();
                    let al = after.len();
                    al > 1e-12 * bl.max(1e-300) && after.dot(before) > 0.999_999 * al * bl
                });
                if !ok {
                    continue;
                }
                for &t in &shared {
                    alive[t] = false;
                }
                for &t in &tris {
                    if !alive[t] {
                        continue;
                    }
                    for x in &mut m.tris[t] {
                        if *x == v {
                            *x = u;
                        }
                    }
                    vt[u as usize].push(t);
                }
                vt[v as usize].clear();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    let mut k = 0;
    m.tri_surface.retain(|_| {
        k += 1;
        alive[k - 1]
    });
    let mut k = 0;
    m.tris.retain(|_| {
        k += 1;
        alive[k - 1]
    });
}

/// Merge each planar face's fragments: find the boundary loops of every connected set
/// of triangles on one plane and re-triangulate them as a single polygon with holes.
/// Boundary vertices (including every vertex a neighbour needs) are kept, so the mesh
/// stays watertight.
pub fn refine(m: &mut Mesh) {
    let tri_count = m.tris.len();
    // Directed edge → triangle.
    let mut owner: FixedMap<(u32, u32), usize> = FixedMap::default();
    for (i, t) in m.tris.iter().enumerate() {
        for k in 0..3 {
            owner.insert((t[k], t[(k + 1) % 3]), i);
        }
    }
    let mut group = vec![usize::MAX; tri_count];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for seed in 0..tri_count {
        if group[seed] != usize::MAX {
            continue;
        }
        let s = m.tri_surface[seed];
        let gid = groups.len();
        let mut members = vec![seed];
        group[seed] = gid;
        let mut at = 0;
        while at < members.len() {
            let t = m.tris[members[at]];
            at += 1;
            for k in 0..3 {
                if let Some(&nb) = owner.get(&(t[(k + 1) % 3], t[k])) {
                    if group[nb] == usize::MAX && m.tri_surface[nb] == s {
                        group[nb] = gid;
                        members.push(nb);
                    }
                }
            }
        }
        groups.push(members);
    }
    let mut out_tris: Vec<[u32; 3]> = Vec::with_capacity(tri_count);
    let mut out_surf: Vec<u32> = Vec::with_capacity(tri_count);
    for members in &groups {
        let s = m.tri_surface[members[0]];
        let normal = match &m.surfaces[s as usize] {
            Surface::Plane { normal, .. } if members.len() > 2 => Some(*normal),
            _ => None,
        };
        let redone = normal.and_then(|n| retriangulate(m, members, &group, n));
        match redone {
            Some(tris) => {
                out_surf.extend(std::iter::repeat_n(s, tris.len()));
                out_tris.extend(tris);
            }
            None => {
                for &t in members {
                    out_tris.push(m.tris[t]);
                    out_surf.push(s);
                }
            }
        }
    }
    m.tris = out_tris;
    m.tri_surface = out_surf;
}

fn retriangulate(m: &Mesh, members: &[usize], group: &[usize], n: V3) -> Option<Vec<[u32; 3]>> {
    let _gid = group[members[0]];
    let mut inside: FixedMap<(u32, u32), ()> = FixedMap::default();
    for &t in members {
        let tri = m.tris[t];
        for k in 0..3 {
            inside.insert((tri[k], tri[(k + 1) % 3]), ());
        }
    }
    // Boundary edges: no reverse partner within the group.
    let mut next: std::collections::BTreeMap<u32, Vec<u32>> = std::collections::BTreeMap::new();
    let mut count = 0usize;
    for &t in members {
        let tri = m.tris[t];
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            if !inside.contains_key(&(b, a)) {
                next.entry(a).or_default().push(b);
                count += 1;
            }
        }
    }
    // A vertex where the boundary touches itself makes loops ambiguous: keep as is.
    if next.values().any(|v| v.len() != 1) {
        return None;
    }
    let mut loops: Vec<Vec<u32>> = Vec::new();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for &start in next.keys() {
        if seen.contains(&start) {
            continue;
        }
        let mut lp = vec![start];
        seen.insert(start);
        let mut cur = next[&start][0];
        while cur != start {
            if !seen.insert(cur) {
                return None;
            }
            lp.push(cur);
            cur = *next.get(&cur)?.first()?;
        }
        loops.push(lp);
    }
    if loops.iter().map(Vec::len).sum::<usize>() != count {
        return None;
    }
    let area = |lp: &Vec<u32>| {
        let pts: Vec<V2> = lp
            .iter()
            .map(|i| project(m.verts[*i as usize], n))
            .collect();
        crate::sketch::profile::signed_area(&pts)
    };
    let (outer, holes): (Vec<Vec<u32>>, Vec<Vec<u32>>) =
        loops.into_iter().partition(|l| area(l) > 0.0);
    if outer.len() != 1 {
        return None;
    }
    let before: f64 = members.iter().map(|t| m.tri_area(*t)).sum();
    let tris = ear_clip(&m.verts, &outer[0], &holes, n);
    let after: f64 = tris
        .iter()
        .map(|t| {
            let (a, b, c) = (
                m.verts[t[0] as usize],
                m.verts[t[1] as usize],
                m.verts[t[2] as usize],
            );
            (b - a).cross(c - a).len() / 2.0
        })
        .sum();
    // Every boundary vertex must be used, or a neighbour's edge would be left open.
    let used: BTreeSet<u32> = tris.iter().flatten().copied().collect();
    let all_used = next.keys().all(|v| used.contains(v));
    let boundary: Vec<u32> = next.keys().copied().collect();
    ((after - before).abs() <= 1e-9 * before.max(1.0)
        && all_used
        && triangulation_ok(&m.verts, &tris, &boundary, n))
    .then_some(tris)
}

/// A triangulation is usable when no triangle is a sliver and no boundary vertex sits
/// on a triangle's edge without being one of its corners (which would open the mesh).
fn triangulation_ok(verts: &[V3], tris: &[[u32; 3]], boundary: &[u32], n: V3) -> bool {
    let pt = |i: u32| project(verts[i as usize], n);
    for t in tris {
        let (a, b, c) = (pt(t[0]), pt(t[1]), pt(t[2]));
        let area = (b - a).cross(c - a);
        let longest = (b - a).len().max((c - b).len()).max((a - c).len());
        // Height of the triangle over its longest side.
        if area <= 0.0 || area / longest.max(1e-300) < WELD {
            return false;
        }
        for k in 0..3 {
            let (i, j) = (t[k], t[(k + 1) % 3]);
            let (p, q) = (pt(i), pt(j));
            let d = q - p;
            let len2 = d.dot(d);
            for &v in boundary {
                if v == i || v == j || v == t[(k + 2) % 3] {
                    continue;
                }
                let x = pt(v);
                let s = (x - p).dot(d) / len2;
                if s > 0.0 && s < 1.0 && (x - p).cross(d).abs() / len2.sqrt() <= WELD {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v3;
    fn cube(min: f64, max: f64) -> Mesh {
        Mesh::cuboid(v3(min, min, min), v3(max, max, max))
    }
    #[test]
    fn booleans_of_overlapping_cubes_are_watertight_with_exact_volumes() {
        let a = cube(0.0, 2.0);
        let b = Mesh::cuboid(v3(1.0, 1.0, 1.0), v3(3.0, 3.0, 3.0));
        let u = boolean(&a, &b, Op::Union);
        let d = boolean(&a, &b, Op::Difference);
        let i = boolean(&a, &b, Op::Intersection);
        for (m, want) in [(&u, 15.0), (&d, 7.0), (&i, 1.0)] {
            assert!(m.is_watertight(), "{} open edges", m.open_edges());
            assert!((m.volume() - want).abs() < 1e-9, "{} != {want}", m.volume());
        }
        // Faces were merged: the intersection is a plain cube again.
        assert_eq!(i.surfaces.len(), 6);
        assert_eq!(i.tris.len(), 12);
    }
    #[test]
    fn a_through_hole_leaves_a_watertight_genus_one_solid() {
        let a = Mesh::cuboid(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 2.0));
        let b = Mesh::cuboid(v3(4.0, 4.0, -1.0), v3(6.0, 6.0, 3.0));
        let d = boolean(&a, &b, Op::Difference);
        assert!(d.is_watertight(), "{} open edges", d.open_edges());
        assert!((d.volume() - (200.0 - 8.0)).abs() < 1e-9);
        // Euler characteristic of a torus: V - E + F = 0.
        let e = d.tris.len() * 3 / 2;
        assert_eq!(d.verts.len() as i64 - e as i64 + d.tris.len() as i64, 0);
    }
}
