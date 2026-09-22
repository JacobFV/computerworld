//! Tessellation for display and mesh export: every face triangulated in its parameter
//! plane (ear clipping with holes, then interior points on curved faces restored to a
//! Delaunay triangulation by edge flips), mapped back to 3D through the exact surface.
//! Edge samples are shared by the faces on both sides, so the mesh is watertight; the
//! triangles remember their face, and the faces, edges and vertices of the returned
//! [`Topology`] are the B-rep's own, in its order.
use super::geom::{Curve, Surface};
use super::mass;
use super::topo::{edge_params, Solid};
use super::uv::{face_uv, loop_polygon, point_in_polys, FaceUV};
use crate::math::{v2, TAU, V2, V3};
use crate::mesh::{self, Mesh};
use crate::solid::{Edge as TEdge, EdgeKind, Face as TFace, Topology};

fn mesh_surface(s: &Surface, fu: &FaceUV) -> mesh::Surface {
    match s {
        Surface::Plane { f } => mesh::Surface::Plane {
            origin: f.origin,
            normal: f.z * fu.sense,
        },
        Surface::Cylinder { f, r } => mesh::Surface::Cylinder {
            origin: f.origin,
            axis: f.z,
            radius: *r,
        },
        Surface::Cone { f, .. } => mesh::Surface::Cone {
            origin: f.origin,
            axis: f.z,
        },
        Surface::Sphere { f, r } => mesh::Surface::Revolved {
            origin: f.origin,
            axis: f.z,
            center: f.origin,
            radius: *r,
        },
        Surface::Torus { f, major, minor } => mesh::Surface::Revolved {
            origin: f.origin,
            axis: f.z,
            center: f.origin + f.x * *major,
            radius: *minor,
        },
        Surface::Revolution { f, .. } => mesh::Surface::Revolved {
            origin: f.origin,
            axis: f.z,
            center: f.origin,
            radius: 0.0,
        },
        _ => mesh::Surface::Facets,
    }
}

/// Triangulate a polygon with holes in the plane: `outer` counter-clockwise, holes
/// clockwise, as index lists into `pts`.
pub fn ear_clip2(pts: &[V2], outer: &[usize], holes: &[Vec<usize>]) -> Vec<[usize; 3]> {
    let pt = |i: usize| pts[i];
    let mut ring: Vec<usize> = outer.to_vec();
    let mut hs: Vec<&Vec<usize>> = holes.iter().filter(|h| h.len() >= 3).collect();
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
            let hn = h.len();
            (0..rn).all(|e| !crosses(pt(ring[e]), pt(ring[(e + 1) % rn])))
                && (0..hn).all(|e| !crosses(pt(h[e]), pt(h[(e + 1) % hn])))
        };
        let k = cands
            .iter()
            .map(|c| c.1)
            .find(|k| visible(*k))
            .unwrap_or(cands[0].1);
        let mut merged = ring[..=k].to_vec();
        for s in 0..=h.len() {
            merged.push(h[(hi + s) % h.len()]);
        }
        merged.push(ring[k]);
        merged.extend_from_slice(&ring[k + 1..]);
        ring = merged;
    }
    let mut tris = Vec::new();
    let mut dropped: Vec<usize> = Vec::new();
    let mut guard = 0;
    while ring.len() > 3 && guard < 200_000 {
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
            let blocked = ring.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = pt(j);
                if p == a || p == b || p == c {
                    return false;
                }
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
            let mut best: Option<(f64, usize)> = None;
            for k in 0..m {
                let (a, b, c) = (
                    pt(ring[(k + m - 1) % m]),
                    pt(ring[k]),
                    pt(ring[(k + 1) % m]),
                );
                let area = (b - a).cross(c - a);
                if area > 1e-10 * (b - a).len() * (c - b).len() && best.is_none_or(|x| area > x.0) {
                    best = Some((area, k));
                }
            }
            match best {
                Some((_, k)) => {
                    tris.push([ring[(k + m - 1) % m], ring[k], ring[(k + 1) % m]]);
                    ring.remove(k);
                }
                None => {
                    // Only collinear corners are left (a sliver of a face). Clip one
                    // anyway: the triangle has next to no area, but it keeps every
                    // boundary edge in the mesh, which is what the face across it needs.
                    let k = (0..m)
                        .max_by(|&x, &y| {
                            let area = |k: usize| {
                                let (a, b, c) = (
                                    pt(ring[(k + m - 1) % m]),
                                    pt(ring[k]),
                                    pt(ring[(k + 1) % m]),
                                );
                                (b - a).cross(c - a)
                            };
                            area(x).total_cmp(&area(y))
                        })
                        .unwrap_or(0);
                    tris.push([ring[(k + m - 1) % m], ring[k], ring[(k + 1) % m]]);
                    ring.remove(k);
                }
            }
        }
    }
    if ring.len() == 3 {
        tris.push([ring[0], ring[1], ring[2]]);
    } else {
        dropped.extend(ring.iter().copied());
    }
    // Points left on a straight run must still split the triangle along it, or the
    // neighbour across that edge would not match.
    for v in dropped {
        let p = pt(v);
        let hit = tris.iter().enumerate().find_map(|(ti, t)| {
            if t.contains(&v) {
                return None;
            }
            (0..3).find_map(|k| {
                let (i, j) = (t[k], t[(k + 1) % 3]);
                let (a, b) = (pt(i), pt(j));
                let d = b - a;
                let len2 = d.dot(d);
                if len2 == 0.0 {
                    return None;
                }
                let s = (p - a).dot(d) / len2;
                let off = (p - a).cross(d).abs() / len2.sqrt();
                (s > 1e-9 && s < 1.0 - 1e-9 && off <= 1e-9 * (1.0 + len2.sqrt())).then_some((ti, k))
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

/// Insert points into a triangulation, restoring the Delaunay property (measured with
/// the parameter axes scaled by `sx`, `sy`) by flipping edges that are not constrained.
fn insert_points(
    pts: &mut Vec<V2>,
    tris: &mut Vec<[usize; 3]>,
    constrained: &std::collections::BTreeSet<(usize, usize)>,
    extra: &[V2],
    sx: f64,
    sy: f64,
) {
    let sc = |p: V2| v2(p.x * sx, p.y * sy);
    let in_circle = |a: V2, b: V2, c: V2, d: V2| -> bool {
        let (a, b, c, d) = (sc(a), sc(b), sc(c), sc(d));
        let (adx, ady) = (a.x - d.x, a.y - d.y);
        let (bdx, bdy) = (b.x - d.x, b.y - d.y);
        let (cdx, cdy) = (c.x - d.x, c.y - d.y);
        let det = (adx * adx + ady * ady) * (bdx * cdy - cdx * bdy)
            - (bdx * bdx + bdy * bdy) * (adx * cdy - cdx * ady)
            + (cdx * cdx + cdy * cdy) * (adx * bdy - bdx * ady);
        {
            let m = adx * adx + ady * ady + bdx * bdx + bdy * bdy + cdx * cdx + cdy * cdy;
            det > 1e-12 * m * m
        }
    };
    let key = |a: usize, b: usize| (a.min(b), a.max(b));
    for q in extra {
        // Locate.
        let Some(ti) = tris.iter().position(|t| {
            let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
            let d1 = (b - a).cross(*q - a);
            let d2 = (c - b).cross(*q - b);
            let d3 = (a - c).cross(*q - c);
            let area = (b - a).cross(c - a);
            let m = 1e-9 * area.abs();
            d1 > m && d2 > m && d3 > m
        }) else {
            continue;
        };
        let qi = pts.len();
        pts.push(*q);
        let [a, b, c] = tris[ti];
        tris[ti] = [a, b, qi];
        tris.push([b, c, qi]);
        tris.push([c, a, qi]);
    }
    // Lawson flips over the whole triangulation until it is Delaunay.
    for _pass in 0..200 {
        let mut owner: std::collections::BTreeMap<(usize, usize), usize> =
            std::collections::BTreeMap::new();
        for (i, t) in tris.iter().enumerate() {
            for k in 0..3 {
                owner.insert((t[k], t[(k + 1) % 3]), i);
            }
        }
        let mut flipped = false;
        let mut touched = vec![false; tris.len()];
        let keys: Vec<(usize, usize)> = owner.keys().copied().collect();
        for (u, v) in keys {
            if u > v || constrained.contains(&key(u, v)) {
                continue;
            }
            let (Some(&t1), Some(&t2)) = (owner.get(&(u, v)), owner.get(&(v, u))) else {
                continue;
            };
            if touched[t1] || touched[t2] {
                continue;
            }
            let a = *tris[t1].iter().find(|x| **x != u && **x != v).unwrap();
            let far = *tris[t2].iter().find(|x| **x != u && **x != v).unwrap();
            // t1 = (u, v, a) counter-clockwise; flip when `far` is inside its circle.
            if !in_circle(pts[u], pts[v], pts[a], pts[far]) {
                continue;
            }
            let (pu, pv, pa, pf) = (pts[u], pts[v], pts[a], pts[far]);
            let convex = (pf - pa).cross(pu - pa) * (pf - pa).cross(pv - pa) < 0.0
                && (pv - pu).cross(pa - pu) * (pv - pu).cross(pf - pu) < 0.0;
            if !convex {
                continue;
            }
            tris[t1] = [u, far, a];
            tris[t2] = [far, v, a];
            touched[t1] = true;
            touched[t2] = true;
            flipped = true;
        }
        if !flipped {
            break;
        }
    }
}

/// Curvature steps for the interior points of a curved face, per parameter.
fn interior_steps(s: &Surface, fu: &FaceUV) -> Option<(f64, f64)> {
    let (du, dv) = (fu.hi.x - fu.lo.x, fu.hi.y - fu.lo.y);
    let ang = TAU / 48.0;
    match s {
        Surface::Plane { .. } => None,
        Surface::Cylinder { .. } | Surface::Cone { .. } => {
            // As fine along the axis as round it, so triangles never span a wide angle.
            let mid = v2((fu.lo.x + fu.hi.x) / 2.0, (fu.lo.y + fu.hi.y) / 2.0);
            let (_, a, b) = s.d1(mid.x, mid.y);
            let iso = ang * a.len() / b.len().max(1e-300);
            Some((ang, iso.max(dv / 96.0).max(1e-9)))
        }
        Surface::Sphere { .. } | Surface::Torus { .. } => Some((ang, ang)),
        Surface::Revolution { .. } => Some((ang, (dv / 16.0).max(1e-9))),
        Surface::Pipe { .. } => Some(((du / 24.0).max(1e-9), ang)),
        _ => Some(((du / 16.0).max(1e-9), (dv / 16.0).max(1e-9))),
    }
}

/// The display mesh of a solid, and faces/edges/vertices matching its B-rep.
pub fn tessellate(s: &Solid) -> (Mesh, Topology) {
    let mut m = Mesh::default();
    // B-rep vertices first, then each edge's interior samples.
    let vbase: Vec<u32> = s
        .vertices
        .iter()
        .map(|v| {
            m.verts.push(v.p);
            m.verts.len() as u32 - 1
        })
        .collect();
    let mut edge_samples: Vec<Vec<u32>> = Vec::with_capacity(s.edges.len());
    for e in &s.edges {
        let ts = edge_params(e);
        let n = ts.len();
        let mut idx = Vec::with_capacity(n);
        idx.push(vbase[e.v0]);
        if !e.degenerate {
            for t in &ts[1..n - 1] {
                m.verts.push(e.point(*t));
                idx.push(m.verts.len() as u32 - 1);
            }
            idx.push(vbase[e.v1]);
        }
        edge_samples.push(idx);
    }
    let props = mass::face_props(s);
    let mut faces = Vec::with_capacity(s.faces.len());
    let mut tri_face = Vec::new();
    for (fi, face) in s.faces.iter().enumerate() {
        let fu = face_uv(s, fi);
        m.surfaces.push(mesh_surface(&face.surface, &fu));
        let mut pts: Vec<V2> = Vec::new();
        let mut ids: Vec<u32> = Vec::new();
        let mut rings: Vec<Vec<usize>> = Vec::new();
        for l in &fu.loops {
            let mut ring = Vec::new();
            for c in l {
                let e = &s.edges[c.co.edge];
                if e.degenerate {
                    // A pole is one point: the parameter plane crosses it in a straight
                    // run, and every point of that run is the same vertex, so only its
                    // start belongs to the ring (more would make triangles that collapse
                    // and leave the mesh non-manifold there).
                    pts.push(c.uv[0]);
                    ids.push(vbase[e.v0]);
                    ring.push(pts.len() - 1);
                    continue;
                }
                let samples = &edge_samples[c.co.edge];
                let n = c.uv.len();
                for i in 0..n - 1 {
                    let si = if c.co.rev { n - 1 - i } else { i };
                    pts.push(c.uv[i]);
                    ids.push(samples[si]);
                    ring.push(pts.len() - 1);
                }
            }
            // Drop repeated consecutive points.
            let mut clean: Vec<usize> = Vec::with_capacity(ring.len());
            let dup = |i: usize, j: usize| {
                let (a, b) = (pts[i], pts[j]);
                (ids[i] == ids[j] || a == b) && (a - b).len() <= 1e-9 * (1.0 + a.len())
            };
            for &i in &ring {
                if clean.last().is_some_and(|&j: &usize| dup(i, j)) {
                    continue;
                }
                clean.push(i);
            }
            while clean.len() > 1 && dup(clean[0], *clean.last().unwrap()) {
                clean.pop();
            }
            if clean.len() >= 3 {
                rings.push(clean);
            }
        }
        let mut local_tris: Vec<[usize; 3]> = Vec::new();
        if !rings.is_empty() {
            // Counter-clockwise outer ring in the parameter plane.
            let flip = fu.sense < 0.0;
            let oriented: Vec<Vec<usize>> = rings
                .iter()
                .map(|r| {
                    if flip {
                        r.iter().rev().copied().collect()
                    } else {
                        r.clone()
                    }
                })
                .collect();
            let mut tris = ear_clip2(&pts, &oriented[0], &oriented[1..]);
            if let Some((su, sv)) = interior_steps(&face.surface, &fu) {
                let polys: Vec<Vec<V2>> = fu.loops.iter().map(|l| loop_polygon(l)).collect();
                let mut extra = Vec::new();
                let nu = (((fu.hi.x - fu.lo.x) / su).ceil() as usize).clamp(1, 96);
                let nv = (((fu.hi.y - fu.lo.y) / sv).ceil() as usize).clamp(1, 96);
                let (du, dv) = (
                    (fu.hi.x - fu.lo.x) / nu as f64,
                    (fu.hi.y - fu.lo.y) / nv as f64,
                );
                for i in 1..nu {
                    for j in 1..nv {
                        let q = v2(fu.lo.x + du * i as f64, fu.lo.y + dv * j as f64);
                        if !point_in_polys(q, &polys) {
                            continue;
                        }
                        // Keep clear of the boundary.
                        let near = pts.iter().any(|p| {
                            ((p.x - q.x) / du).abs() < 0.35 && ((p.y - q.y) / dv).abs() < 0.35
                        });
                        if !near {
                            extra.push(q);
                        }
                    }
                }
                if !extra.is_empty() {
                    let mut constrained = std::collections::BTreeSet::new();
                    for r in &oriented {
                        for k in 0..r.len() {
                            let (a, b) = (r[k], r[(k + 1) % r.len()]);
                            constrained.insert((a.min(b), a.max(b)));
                        }
                    }
                    let mid = v2((fu.lo.x + fu.hi.x) / 2.0, (fu.lo.y + fu.hi.y) / 2.0);
                    let (_, a, b) = face.surface.d1(mid.x, mid.y);
                    let before = pts.len();
                    insert_points(
                        &mut pts,
                        &mut tris,
                        &constrained,
                        &extra,
                        a.len().max(1e-9),
                        b.len().max(1e-9),
                    );
                    for q in &pts[before..] {
                        m.verts.push(face.surface.eval(q.x, q.y));
                        ids.push(m.verts.len() as u32 - 1);
                    }
                }
            }
            for t in tris {
                local_tris.push(if flip { [t[0], t[2], t[1]] } else { t });
            }
        }
        let mut members = Vec::new();
        for t in local_tris {
            let tri = [ids[t[0]], ids[t[1]], ids[t[2]]];
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                continue;
            }
            members.push(m.tris.len());
            m.tris.push(tri);
            m.tri_surface.push(fi as u32);
            tri_face.push(fi);
        }
        faces.push(TFace {
            surface: fi as u32,
            tris: members,
            area: props[fi].area,
            centroid: props[fi].centroid,
        });
    }
    split_shared_chords(s, &mut m, &mut tri_face, &mut faces);
    let edges: Vec<TEdge> = (0..s.edges.len())
        .map(|ei| {
            let e = &s.edges[ei];
            let faces = s.edge_faces(ei).unwrap_or((0, 0));
            let (kind, circle) = match &e.curve {
                _ if e.degenerate => (EdgeKind::Curve, None),
                Curve::Line { .. } => (EdgeKind::Line, None),
                Curve::Circle { f, r } => (
                    if e.closed() {
                        EdgeKind::Circle
                    } else {
                        EdgeKind::Arc
                    },
                    Some((f.origin, *r, f.z)),
                ),
                _ => (EdgeKind::Curve, None),
            };
            TEdge {
                verts: edge_samples[ei].clone(),
                faces,
                kind,
                length: mass::edge_length(s, ei),
                circle,
            }
        })
        .collect();
    let topo = Topology {
        faces,
        edges,
        vertices: vbase,
        tri_face,
    };
    (m, topo)
}

/// Two faces can chord between the same pair of vertices: on a sliver face, ear
/// clipping draws a diagonal between two samples of the edge it shares with its
/// neighbour, and the neighbour draws the same diagonal. Each face is a sound
/// triangulation on its own, but the mesh then uses that chord four times instead of
/// twice. Split the triangles of all but one of those faces at the chord's mid point
/// — a real point of the face's own surface — which leaves every edge with two uses.
fn split_shared_chords(s: &Solid, m: &mut Mesh, tri_face: &mut Vec<usize>, faces: &mut [TFace]) {
    for _ in 0..16 {
        let mut users: std::collections::BTreeMap<(u32, u32), Vec<usize>> = Default::default();
        for (i, t) in m.tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                users.entry((a.min(b), a.max(b))).or_default().push(i);
            }
        }
        let shared: Vec<((u32, u32), Vec<usize>)> =
            users.into_iter().filter(|(_, u)| u.len() > 2).collect();
        if shared.is_empty() {
            return;
        }
        let mut split_any = false;
        // A split rewrites its triangles, so a triangle is only ever split once per
        // pass; anything else waits for the next one, when the counts are fresh.
        let mut touched: std::collections::BTreeSet<usize> = Default::default();
        for (pair, us) in shared {
            if us.iter().any(|i| touched.contains(i)) {
                continue;
            }
            let mut per_face: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
            for &i in &us {
                per_face.entry(tri_face[i]).or_default().push(i);
            }
            let keep = match per_face.keys().next() {
                // All the uses are inside one face: splitting would not separate them.
                Some(_) if per_face.len() < 2 => continue,
                Some(&f) => f,
                None => continue,
            };
            let (pa, pb) = (m.verts[pair.0 as usize], m.verts[pair.1 as usize]);
            let chord = (pb - pa).len();
            for (&fi, tris) in per_face.iter() {
                if fi == keep {
                    continue;
                }
                let surface = &s.faces[fi].surface;
                let mid = (pa + pb) * 0.5;
                let (u, v) = surface.project(mid);
                let q = surface.eval(u, v);
                // Keep the split point on the chord if the projection wandered off.
                let p = if (q - mid).len() <= chord * 0.5 {
                    q
                } else {
                    mid
                };
                m.verts.push(p);
                let nv = m.verts.len() as u32 - 1;
                for &i in tris {
                    let t = m.tris[i];
                    let Some(k) = (0..3).find(|&k| {
                        let (a, b) = (t[k], t[(k + 1) % 3]);
                        (a.min(b), a.max(b)) == pair
                    }) else {
                        continue;
                    };
                    let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                    touched.insert(i);
                    m.tris[i] = [a, nv, c];
                    faces[fi].tris.push(m.tris.len());
                    m.tris.push([nv, b, c]);
                    m.tri_surface.push(fi as u32);
                    tri_face.push(fi);
                    split_any = true;
                }
            }
        }
        if !split_any {
            return;
        }
    }
}

/// Mid point of an edge, and its direction (unit tangent for lines, axis for circles).
pub fn edge_hint(s: &Solid, e: usize) -> (V3, V3) {
    let ed = &s.edges[e];
    let mid = ed.mid();
    let dir = match &ed.curve {
        Curve::Line { d, .. } => *d,
        Curve::Circle { f, .. } | Curve::Ellipse { f, .. } => f.z,
        c => c.d1((ed.t0 + ed.t1) / 2.0).1.norm(),
    };
    (mid, dir)
}
