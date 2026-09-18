//! Booleans on B-reps: union, difference and intersection.
//!
//! 1. Every vertex of both solids goes into one pool (points within tolerance are one).
//! 2. Every edge is intersected with every face of the other solid; the crossings join
//!    the pool.
//! 3. Every pair of faces on different surfaces is intersected; the section curve is cut
//!    at the pool's points on it and the pieces inside both faces become new edges.
//! 4. All edges are split at every pool point lying on them, and pieces that coincide
//!    (the same points joined along the same curve) become one edge.
//! 5. Each face is rebuilt from its boundary pieces plus every piece lying inside it —
//!    section edges and, where faces share a surface, the other face's boundary — by
//!    walking the planar arrangement in the face's parameter plane.
//! 6. Each resulting face is classified against the other solid (inside, outside, or on
//!    a face of the same surface, facing the same way or not) and kept or dropped by
//!    the operation; kept faces share their edges, so the result is closed.
//! 7. Faces on one surface are merged back (FreeCAD's Refine).
use super::geom::{Curve, Surface};
use super::intersect::{curve_surface, section_crossings, surface_sections, CurveHit, Prepared};
use super::topo::{edge_params, Box3, Coedge, Edge, Face, Solid, TOL};
use super::uv::{classify, edge_distance, face_uv, loop_polygon, point_in_polys, Where};
use crate::math::{v2, V2, V3};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Union,
    Difference,
    Intersection,
}

/// Vertex pool with tolerant merging on a fixed grid.
struct Pool {
    pts: Vec<V3>,
    tol: f64,
    grid: BTreeMap<(i64, i64, i64), Vec<usize>>,
}
impl Pool {
    fn cell(&self, p: V3) -> (i64, i64, i64) {
        let s = 1.0 / (self.tol * 64.0);
        (
            (p.x * s).floor() as i64,
            (p.y * s).floor() as i64,
            (p.z * s).floor() as i64,
        )
    }
    fn find(&self, p: V3) -> Option<usize> {
        let (cx, cy, cz) = self.cell(p);
        let mut best: Option<(f64, usize)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(l) = self.grid.get(&(cx + dx, cy + dy, cz + dz)) {
                        for &i in l {
                            let d = self.pts[i].dist(p);
                            if d <= self.tol && best.is_none_or(|b| d < b.0) {
                                best = Some((d, i));
                            }
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }
    fn add(&mut self, p: V3) -> usize {
        if let Some(i) = self.find(p) {
            return i;
        }
        self.pts.push(p);
        let c = self.cell(p);
        self.grid.entry(c).or_default().push(self.pts.len() - 1);
        self.pts.len() - 1
    }
    /// Pool points inside a box.
    fn in_box(&self, b: &Box3) -> Vec<usize> {
        (0..self.pts.len())
            .filter(|i| b.contains(self.pts[*i]))
            .collect()
    }
}

#[derive(Clone, Debug)]
struct Src {
    curve: Curve,
    t0: f64,
    t1: f64,
    v0: usize,
    v1: usize,
}

/// A final edge: a piece of a source edge.
#[derive(Clone, Debug)]
struct Piece {
    curve: Curve,
    t0: f64,
    t1: f64,
    v0: usize,
    v1: usize,
    mid: V3,
    quarter: V3,
}

fn same_piece(a: &Piece, b: &Piece, tol: f64) -> Option<bool> {
    let ends = (a.v0 == b.v0 && a.v1 == b.v1) || (a.v0 == b.v1 && a.v1 == b.v0);
    if !ends || a.mid.dist(b.mid) > tol {
        return None;
    }
    if a.v0 == a.v1 {
        // Closed: the same way round if the quarter points agree.
        if a.quarter.dist(b.quarter) <= tol {
            return Some(false);
        }
        // The other quarter point.
        let (b0, b1) = (b.t0, b.t1);
        let q3 = b.curve.eval(b0 + (b1 - b0) * 0.75);
        return (a.quarter.dist(q3) <= tol).then_some(true);
    }
    Some(a.v0 != b.v0)
}

/// Which side a face came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    A,
    B,
}

/// A face rebuilt from pieces: loops of (piece or degenerate index, reversed).
#[derive(Clone, Debug)]
struct SubFace {
    side: Side,
    face: usize,
    loops: Vec<Vec<(Ref, bool)>>,
    point: Option<(V3, V2)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ref {
    Piece(usize),
    /// A point edge at pool vertex `v` spanning `span` of u.
    Degenerate { v: usize, span: f64 },
}

/// `a op b`.
pub fn boolean(a: &Solid, b: &Solid, op: Op) -> Result<Solid, String> {
    if a.is_empty() {
        return Ok(match op {
            Op::Union => b.clone(),
            _ => Solid::default(),
        });
    }
    if b.is_empty() {
        return Ok(match op {
            Op::Intersection => Solid::default(),
            _ => a.clone(),
        });
    }
    let pa = Prepared::new(a);
    let pb = Prepared::new(b);
    let box_a = pa.boxes.iter().fold(Box3::empty(), |x, y| x.union(y));
    let box_b = pb.boxes.iter().fold(Box3::empty(), |x, y| x.union(y));
    if !box_a.overlaps(&box_b) {
        return Ok(match op {
            Op::Union => {
                let mut s = a.clone();
                s.append(b);
                s
            }
            Op::Difference => a.clone(),
            Op::Intersection => Solid::default(),
        });
    }
    let scale = box_a.union(&box_b).diagonal().max(1.0);
    let tol = TOL * (1.0 + scale / 100.0);
    let mut pool = Pool {
        pts: Vec::new(),
        tol,
        grid: BTreeMap::new(),
    };
    let va: Vec<usize> = a.vertices.iter().map(|v| pool.add(v.p)).collect();
    let vb: Vec<usize> = b.vertices.iter().map(|v| pool.add(v.p)).collect();
    // Source edges: A's, then B's (degenerate ones stay with their faces).
    let mut srcs: Vec<Src> = Vec::new();
    let mut src_of_a = vec![usize::MAX; a.edges.len()];
    let mut src_of_b = vec![usize::MAX; b.edges.len()];
    for (i, e) in a.edges.iter().enumerate() {
        if !e.degenerate {
            src_of_a[i] = srcs.len();
            srcs.push(Src {
                curve: e.curve.clone(),
                t0: e.t0,
                t1: e.t1,
                v0: va[e.v0],
                v1: va[e.v1],
            });
        }
    }
    for (i, e) in b.edges.iter().enumerate() {
        if !e.degenerate {
            src_of_b[i] = srcs.len();
            srcs.push(Src {
                curve: e.curve.clone(),
                t0: e.t0,
                t1: e.t1,
                v0: vb[e.v0],
                v1: vb[e.v1],
            });
        }
    }
    // 2. Edges against the other solid's faces.
    for (solid, other, prep_other) in [(a, b, &pb), (b, a, &pa)] {
        for (ei, e) in solid.edges.iter().enumerate() {
            if e.degenerate {
                continue;
            }
            let eb = solid.edge_box(ei);
            for (fi, f) in other.faces.iter().enumerate() {
                if !eb.overlaps(&prep_other.boxes[fi]) {
                    continue;
                }
                if let CurveHit::Points(ts) = curve_surface(&e.curve, e.t0, e.t1, &f.surface, tol * 0.1) {
                    for t in ts {
                        let p = e.curve.eval(t);
                        if classify(other, &prep_other.uvs[fi], fi, p, tol) != Where::Outside {
                            pool.add(p);
                        }
                    }
                }
            }
        }
    }
    // 3. Face against face.
    for fa in 0..a.faces.len() {
        for fb in 0..b.faces.len() {
            let (ba, bb) = (&pa.boxes[fa], &pb.boxes[fb]);
            if !ba.overlaps(bb) {
                continue;
            }
            let (sa, sb) = (&a.faces[fa].surface, &b.faces[fb].surface);
            if sa.same(sb, tol) {
                continue;
            }
            let region = Box3 {
                min: ba.min.max(bb.min),
                max: ba.max.min(bb.max),
            }
            .grow(tol * 10.0 + scale * 1e-6);
            let near: Vec<usize> = pool.in_box(&region);
            let seeds: Vec<V3> = near
                .iter()
                .map(|i| pool.pts[*i])
                .filter(|p| sa.sd(*p).abs() < tol * 10.0 && sb.sd(*p).abs() < tol * 10.0)
                .collect();
            let sections = surface_sections(a, fa, &pa.uvs[fa], b, fb, &pb.uvs[fb], &seeds, &region);
            // Where branches of the section cross, both need a vertex.
            let mut near = near;
            if sections.len() > 1 {
                for i in 0..sections.len() {
                    for j in i + 1..sections.len() {
                        for p in section_crossings(&sections[i], &sections[j], tol) {
                            if region.contains(p) {
                                pool.add(p);
                            }
                        }
                    }
                }
                near = pool.in_box(&region);
            }
            for sec in sections {
                let period = sec.curve.period();
                let mut on: Vec<(f64, usize)> = Vec::new();
                for &vi in &near {
                    let p = pool.pts[vi];
                    if sa.sd(p).abs() > tol * 10.0 || sb.sd(p).abs() > tol * 10.0 {
                        continue;
                    }
                    let t = sec.curve.param_in(p, sec.t0, sec.t1);
                    if sec.curve.eval(t).dist(p) > tol * 10.0 {
                        continue;
                    }
                    if !sec.closed && (t < sec.t0 - 1e-9 || t > sec.t1 + 1e-9) {
                        continue;
                    }
                    on.push((t, vi));
                }
                on.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
                on.dedup_by(|x, y| x.1 == y.1);
                let mut intervals: Vec<(f64, f64, usize, usize)> = Vec::new();
                if sec.closed {
                    let per = period.unwrap_or(sec.t1 - sec.t0);
                    match on.len() {
                        0 => {
                            let t = sec.t0;
                            let mid = sec.curve.eval(t + per / 2.0);
                            let wa = classify(a, &pa.uvs[fa], fa, mid, tol);
                            let wb = classify(b, &pb.uvs[fb], fb, mid, tol);
                            if wa == Where::Inside && wb == Where::Inside {
                                let v = pool.add(sec.curve.eval(t));
                                intervals.push((t, t + per, v, v));
                            }
                        }
                        _ => {
                            for k in 0..on.len() {
                                let (ta, vi) = on[k];
                                let (mut tb, vj) = on[(k + 1) % on.len()];
                                if k + 1 == on.len() {
                                    tb += per;
                                }
                                intervals.push((ta, tb, vi, vj));
                            }
                        }
                    }
                } else {
                    for w in on.windows(2) {
                        intervals.push((w[0].0, w[1].0, w[0].1, w[1].1));
                    }
                }
                for (ta, tb, vi, vj) in intervals {
                    if tb - ta <= 1e-12 {
                        continue;
                    }
                    if vi == vj && !(sec.closed && (tb - ta) > period.unwrap_or(0.0) * 0.5) {
                        continue;
                    }
                    let mid = sec.curve.eval((ta + tb) / 2.0);
                    let wa = classify(a, &pa.uvs[fa], fa, mid, tol);
                    let wb = classify(b, &pb.uvs[fb], fb, mid, tol);
                    if wa == Where::Outside || wb == Where::Outside {
                        continue;
                    }
                    if wa == Where::Boundary && wb == Where::Boundary {
                        continue;
                    }
                    srcs.push(Src {
                        curve: sec.curve.clone(),
                        t0: ta,
                        t1: tb,
                        v0: vi,
                        v1: vj,
                    });
                }
            }
        }
    }
    // 4. Split every source edge at the pool points on it; merge coincident pieces.
    let mut pieces: Vec<Piece> = Vec::new();
    // Per source edge: its pieces in order, as (canonical piece, flipped).
    let mut src_pieces: Vec<Vec<(usize, bool)>> = Vec::with_capacity(srcs.len());
    let mut by_ends: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    for s in &srcs {
        let e = Edge {
            curve: s.curve.clone(),
            t0: s.t0,
            t1: s.t1,
            v0: s.v0,
            v1: s.v1,
            degenerate: false,
        };
        let mut bx = Box3::empty();
        for t in edge_params(&e) {
            bx.add(e.point(t));
        }
        let bx = bx.grow(tol * 10.0 + bx.diagonal() * 0.02);
        let mut cuts: Vec<(f64, usize)> = vec![(s.t0, s.v0), (s.t1, s.v1)];
        let (p0, p1) = (pool.pts[s.v0], pool.pts[s.v1]);
        for vi in pool.in_box(&bx) {
            if vi == s.v0 || vi == s.v1 {
                continue;
            }
            let p = pool.pts[vi];
            let t = s.curve.param_in(p, s.t0, s.t1);
            if t <= s.t0 || t >= s.t1 {
                continue;
            }
            if s.curve.eval(t).dist(p) > tol * 2.0 {
                continue;
            }
            if p.dist(p0) <= tol || p.dist(p1) <= tol {
                continue;
            }
            cuts.push((t, vi));
        }
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        let mut list = Vec::new();
        for w in cuts.windows(2) {
            let ((ta, vi), (tb, vj)) = (w[0], w[1]);
            if tb - ta <= 1e-14 * (1.0 + ta.abs()) {
                continue;
            }
            let mid = s.curve.eval((ta + tb) / 2.0);
            if vi == vj && s.v0 != s.v1 {
                continue;
            }
            if vi != vj && pool.pts[vi].dist(mid) < tol && pool.pts[vj].dist(mid) < tol {
                continue;
            }
            let p = Piece {
                curve: s.curve.clone(),
                t0: ta,
                t1: tb,
                v0: vi,
                v1: vj,
                mid,
                quarter: s.curve.eval(ta + (tb - ta) * 0.25),
            };
            let key = (vi.min(vj), vi.max(vj));
            let found = by_ends.get(&key).and_then(|cands| {
                cands
                    .iter()
                    .find_map(|&c| same_piece(&pieces[c], &p, tol * 100.0).map(|f| (c, f)))
            });
            match found {
                Some((c, flip)) => list.push((c, flip)),
                None => {
                    pieces.push(p);
                    let id = pieces.len() - 1;
                    by_ends.entry(key).or_default().push(id);
                    list.push((id, false));
                }
            }
        }
        src_pieces.push(list);
    }
    // 5. Rebuild each face from its pieces.
    let mut subs: Vec<SubFace> = Vec::new();
    for (side, solid, prep, src_of) in [
        (Side::A, a, &pa, &src_of_a),
        (Side::B, b, &pb, &src_of_b),
    ] {
        for fi in 0..solid.faces.len() {
            let face = &solid.faces[fi];
            // Boundary loops in pieces.
            let mut loops: Vec<Vec<(Ref, bool)>> = Vec::new();
            let mut on_boundary: std::collections::BTreeSet<usize> = Default::default();
            for l in &face.loops {
                let mut out = Vec::new();
                for c in l {
                    let e = &solid.edges[c.edge];
                    if e.degenerate {
                        let vmap = if side == Side::A { &va } else { &vb };
                        let span = (e.t1 - e.t0) * if c.rev { -1.0 } else { 1.0 };
                        out.push((
                            Ref::Degenerate {
                                v: vmap[e.v0],
                                span,
                            },
                            false,
                        ));
                        continue;
                    }
                    let list = &src_pieces[src_of[c.edge]];
                    let seq: Vec<(usize, bool)> = if c.rev {
                        list.iter().rev().map(|(p, f)| (*p, !*f)).collect()
                    } else {
                        list.clone()
                    };
                    for (p, f) in seq {
                        on_boundary.insert(p);
                        out.push((Ref::Piece(p), f));
                    }
                }
                loops.push(out);
            }
            // Pieces lying inside the face.
            let fb = prep.boxes[fi].grow(tol * 10.0);
            let surf = &face.surface;
            let mut inner: Vec<usize> = Vec::new();
            for (pi, p) in pieces.iter().enumerate() {
                if on_boundary.contains(&pi) || !fb.contains(p.mid) {
                    continue;
                }
                let on = [0.2, 0.5, 0.8].iter().all(|s| {
                    let q = p.curve.eval(p.t0 + (p.t1 - p.t0) * s);
                    surf.sd(q).abs() < tol * 10.0
                });
                if !on {
                    continue;
                }
                if classify(solid, &prep.uvs[fi], fi, p.mid, tol) == Where::Inside {
                    inner.push(pi);
                }
            }
            if inner.is_empty() && loops_unsplit(face, solid, &loops, src_of, &src_pieces) {
                subs.push(SubFace {
                    side,
                    face: fi,
                    loops,
                    point: None,
                });
                continue;
            }
            for sf in arrange(surf, &pool.pts, &pieces, &loops, &inner, tol)? {
                subs.push(SubFace {
                    side,
                    face: fi,
                    loops: sf.0,
                    point: sf.1,
                });
            }
        }
    }
    // 6. Classify and select.
    let mut keep: Vec<(usize, bool)> = Vec::new(); // (sub index, reversed)
    for (si, sf) in subs.iter().enumerate() {
        let (solid, other, prep_other) = match sf.side {
            Side::A => (a, b, &pb),
            Side::B => (b, a, &pa),
        };
        let face = &solid.faces[sf.face];
        let (p, uv) = match sf.point {
            Some(x) => x,
            None => {
                let fu = if sf.side == Side::A {
                    &pa.uvs[sf.face]
                } else {
                    &pb.uvs[sf.face]
                };
                super::uv::interior_point(solid, fu, sf.face)
                    .ok_or("a face has no interior")?
            }
        };
        let own_sense = if sf.side == Side::A {
            pa.uvs[sf.face].sense
        } else {
            pb.uvs[sf.face].sense
        };
        let n_own = face.surface.normal(uv.x, uv.y) * own_sense;
        let mut state: Option<&str> = None;
        for (gi, g) in other.faces.iter().enumerate() {
            if !g.surface.same(&face.surface, tol) || !prep_other.boxes[gi].grow(tol).contains(p) {
                continue;
            }
            if classify(other, &prep_other.uvs[gi], gi, p, tol) == Where::Inside {
                let (u, v) = g.surface.project(p);
                let n_other = g.surface.normal(u, v) * prep_other.uvs[gi].sense;
                state = Some(if n_own.dot(n_other) > 0.0 {
                    "same"
                } else {
                    "opposite"
                });
                break;
            }
        }
        let state = match state {
            Some(s) => s,
            None => match prep_other.contains(p) {
                Some(true) => "in",
                Some(false) => "out",
                None => return Err("a face could not be classified".into()),
            },
        };
        let take = match (op, sf.side, state) {
            (Op::Union, _, "out") => Some(false),
            (Op::Union, Side::A, "same") => Some(false),
            (Op::Intersection, _, "in") => Some(false),
            (Op::Intersection, Side::A, "same") => Some(false),
            (Op::Difference, Side::A, "out") => Some(false),
            (Op::Difference, Side::A, "opposite") => Some(false),
            (Op::Difference, Side::B, "in") => Some(true),
            _ => None,
        };
        if let Some(rev) = take {
            keep.push((si, rev));
        }
    }
    // 7. Assemble.
    let mut out = Solid::default();
    for p in &pool.pts {
        out.add_vertex(*p);
    }
    let mut piece_edge: Vec<Option<usize>> = vec![None; pieces.len()];
    for (si, rev) in keep {
        let sf = &subs[si];
        let solid = if sf.side == Side::A { a } else { b };
        let mut loops = Vec::new();
        for l in &sf.loops {
            let mut lo = Vec::new();
            for (r, flip) in l {
                match *r {
                    Ref::Piece(p) => {
                        let e = *piece_edge[p].get_or_insert_with(|| {
                            let pc = &pieces[p];
                            out.add_edge(pc.curve.clone(), pc.t0, pc.t1, pc.v0, pc.v1)
                        });
                        lo.push(Coedge { edge: e, rev: *flip });
                    }
                    Ref::Degenerate { v, span } => {
                        let e = out.add_degenerate(v);
                        out.edges[e].t1 = span.abs();
                        lo.push(Coedge {
                            edge: e,
                            rev: span < 0.0,
                        });
                    }
                }
            }
            loops.push(lo);
        }
        let mut f = Face {
            surface: solid.faces[sf.face].surface.clone(),
            loops,
        };
        if rev {
            for l in &mut f.loops {
                l.reverse();
                for c in l.iter_mut() {
                    c.rev = !c.rev;
                }
            }
        }
        out.faces.push(f);
    }
    out.renumber();
    if out.faces.is_empty() {
        return Ok(out);
    }
    out.check().map_err(|e| {
        let detail = e
            .split_whitespace()
            .nth(1)
            .and_then(|x| x.parse::<usize>().ok())
            .filter(|i| *i < out.edges.len())
            .map(|i| {
                let ed = &out.edges[i];
                format!(
                    " at {:?} from {:?} to {:?}",
                    ed.mid(),
                    out.vertices[ed.v0].p,
                    out.vertices[ed.v1].p
                )
            })
            .unwrap_or_default();
        format!("the boolean left the shape open ({e}{detail})")
    })?;
    Ok(super::refine::refine(&out))
}

/// Whether the face's boundary is unchanged (no edge was split), so it can be kept as is.
fn loops_unsplit(
    face: &Face,
    solid: &Solid,
    loops: &[Vec<(Ref, bool)>],
    src_of: &[usize],
    src_pieces: &[Vec<(usize, bool)>],
) -> bool {
    let _ = loops;
    face.loops.iter().flatten().all(|c| {
        solid.edges[c.edge].degenerate || src_pieces[src_of[c.edge]].len() == 1
    })
}

/// One outgoing half-edge of the arrangement.
#[derive(Clone, Debug)]
struct Half {
    r: Ref,
    flip: bool,
    uv: Vec<V2>,
    from: usize,
    to: usize,
    /// Some(true): a boundary coedge in the face's direction; Some(false): against it.
    boundary: Option<bool>,
}

fn wrap(x: f64, near: f64, per: Option<f64>) -> f64 {
    match per {
        Some(p) => x + ((near - x) / p).round() * p,
        None => x,
    }
}

/// Split a face (its boundary loops in pieces, plus inner pieces) into the regions of the
/// arrangement. Returns each region's loops and a point inside it.
#[allow(clippy::type_complexity)]
fn arrange(
    surf: &Surface,
    pts: &[V3],
    pieces: &[Piece],
    loops: &[Vec<(Ref, bool)>],
    inner: &[usize],
    tol: f64,
) -> Result<Vec<(Vec<Vec<(Ref, bool)>>, Option<(V3, V2)>)>, String> {
    // A temporary face over local edges, to get the boundary's parameter picture.
    let mut tmp = Solid::default();
    let mut vmap: BTreeMap<usize, usize> = BTreeMap::new();
    let mut vid = |tmp: &mut Solid, v: usize| -> usize {
        *vmap.entry(v).or_insert_with(|| tmp.add_vertex(pts[v]))
    };
    let mut refs: Vec<(Ref, bool)> = Vec::new();
    let mut tl = Vec::new();
    for l in loops {
        let mut lo = Vec::new();
        for (r, flip) in l {
            match *r {
                Ref::Piece(p) => {
                    let pc = &pieces[p];
                    let (a, b) = (vid(&mut tmp, pc.v0), vid(&mut tmp, pc.v1));
                    let e = tmp.add_edge(pc.curve.clone(), pc.t0, pc.t1, a, b);
                    lo.push(Coedge { edge: e, rev: *flip });
                }
                Ref::Degenerate { v, span } => {
                    let a = vid(&mut tmp, v);
                    let e = tmp.add_degenerate(a);
                    tmp.edges[e].t1 = span.abs();
                    lo.push(Coedge {
                        edge: e,
                        rev: span < 0.0,
                    });
                }
            }
            refs.push((*r, *flip));
        }
        tl.push(lo);
    }
    tmp.faces.push(Face {
        surface: surf.clone(),
        loops: tl,
    });
    let fu = face_uv(&tmp, 0);
    if !fu.valid {
        return Err("a face's boundary does not close in its parameter plane".into());
    }
    // Work with the face's loops counter-clockwise.
    let flip_u = fu.sense < 0.0;
    let fl = |q: V2| if flip_u { v2(-q.x, q.y) } else { q };
    let (pu, pv) = (surf.period_u(), surf.period_v());
    let (lo, hi) = (fu.lo, fu.hi);
    // Nodes: a pool vertex at a parameter position.
    let mut nodes: Vec<(usize, V2)> = Vec::new();
    let ptol = 1e-7;
    let mut node = |v: usize, q: V2| -> usize {
        for (i, (w, p)) in nodes.iter().enumerate() {
            if *w == v && (p.x - q.x).abs() <= ptol * (1.0 + q.x.abs()) && (p.y - q.y).abs() <= ptol * (1.0 + q.y.abs()) {
                return i;
            }
        }
        nodes.push((v, q));
        nodes.len() - 1
    };
    let mut halves: Vec<Half> = Vec::new();
    // Boundary coedges (from the temporary face; same order as `refs`).
    let mut k = 0;
    for l in &fu.loops {
        for c in l {
            let _ = c;
            k += 1;
        }
    }
    let _ = k;
    // Map temporary coedges back to refs: face_uv keeps loop and coedge order except
    // for moving the outer loop first; match by edge id instead.
    let mut edge_ref: BTreeMap<usize, (Ref, bool)> = BTreeMap::new();
    {
        let mut i = 0;
        for l in &tmp.faces[0].loops {
            for c in l {
                edge_ref.insert(c.edge, refs[i]);
                i += 1;
            }
        }
    }
    for l in &fu.loops {
        for c in l {
            let (r, flip) = edge_ref[&c.co.edge];
            let e = &tmp.edges[c.co.edge];
            let uv: Vec<V2> = c.uv.iter().map(|q| fl(*q)).collect();
            let (va, vb) = match r {
                Ref::Piece(p) => {
                    let pc = &pieces[p];
                    if flip {
                        (pc.v1, pc.v0)
                    } else {
                        (pc.v0, pc.v1)
                    }
                }
                Ref::Degenerate { v, .. } => (v, v),
            };
            let _ = e;
            let from = node(va, uv[0]);
            let to = node(vb, *uv.last().unwrap());
            let mut back = uv.clone();
            back.reverse();
            let rr = match r {
                Ref::Degenerate { v, span } => Ref::Degenerate { v, span: -span },
                x => x,
            };
            halves.push(Half {
                r,
                flip,
                uv,
                from,
                to,
                boundary: Some(true),
            });
            halves.push(Half {
                r: rr,
                flip: !flip,
                uv: back,
                from: to,
                to: from,
                boundary: Some(false),
            });
        }
    }
    // Inner pieces, unwrapped and moved into the face's window.
    for &pi in inner {
        let pc = &pieces[pi];
        let e = Edge {
            curve: pc.curve.clone(),
            t0: pc.t0,
            t1: pc.t1,
            v0: pc.v0,
            v1: pc.v1,
            degenerate: false,
        };
        let ts = edge_params(&e);
        let mut uv: Vec<V2> = Vec::with_capacity(ts.len());
        let mut last: Option<V2> = None;
        for t in &ts {
            let p = e.point(*t);
            let (u, v) = match last {
                Some(r) => surf.project_near(p, (r.x, r.y)),
                None => surf.project(p),
            };
            let mut q = v2(u, v);
            if let Some(r) = last {
                q.x = wrap(q.x, r.x, pu);
                q.y = wrap(q.y, r.y, pv);
            }
            last = Some(q);
            uv.push(q);
        }
        // Poles: an end at a singular point takes the u of its neighbour.
        let n = uv.len();
        if n >= 2 {
            for (end, nb) in [(0usize, 1usize), (n - 1, n - 2)] {
                let p = e.point(ts[end]);
                if super::uv::singular_point(surf, p) {
                    uv[end].x = uv[nb].x;
                }
            }
        }
        // Into the window by the middle.
        let mid = uv[n / 2];
        let du = pu.map_or(0.0, |p| {
            let c = (lo.x + hi.x) / 2.0;
            let mut s = ((c - mid.x) / p).round() * p;
            if mid.x + s < lo.x - 1e-9 {
                s += p;
            }
            if mid.x + s > hi.x + 1e-9 {
                s -= p;
            }
            s
        });
        let dv = pv.map_or(0.0, |p| {
            let c = (lo.y + hi.y) / 2.0;
            let mut s = ((c - mid.y) / p).round() * p;
            if mid.y + s < lo.y - 1e-9 {
                s += p;
            }
            if mid.y + s > hi.y + 1e-9 {
                s -= p;
            }
            s
        });
        let uv: Vec<V2> = uv.into_iter().map(|q| fl(q + v2(du, dv))).collect();
        let from = node(pc.v0, uv[0]);
        let to = node(pc.v1, *uv.last().unwrap());
        let mut back = uv.clone();
        back.reverse();
        halves.push(Half {
            r: Ref::Piece(pi),
            flip: false,
            uv,
            from,
            to,
            boundary: None,
        });
        halves.push(Half {
            r: Ref::Piece(pi),
            flip: true,
            uv: back,
            from: to,
            to: from,
            boundary: None,
        });
    }
    // Split degenerate boundary halves at nodes of the same vertex lying along them.
    let mut split: Vec<Half> = Vec::new();
    for h in halves {
        if let Ref::Degenerate { v, .. } = h.r {
            let (a, b) = (h.uv[0], h.uv[1]);
            let mut cuts: Vec<(f64, usize)> = nodes
                .iter()
                .enumerate()
                .filter(|(i, (w, q))| {
                    *w == v && *i != h.from && *i != h.to && (q.y - a.y).abs() < 1e-9 && {
                        let s = (q.x - a.x) / (b.x - a.x);
                        s > 1e-9 && s < 1.0 - 1e-9
                    }
                })
                .map(|(i, (_, q))| ((q.x - a.x) / (b.x - a.x), i))
                .collect();
            if cuts.is_empty() {
                split.push(h);
                continue;
            }
            cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut prev = (0.0, h.from);
            cuts.push((1.0, h.to));
            for (s, n) in cuts {
                let (pa, pb) = (a + (b - a) * prev.0, a + (b - a) * s);
                // Spans are in the unflipped parameter.
                let span = if flip_u { -(pb.x - pa.x) } else { pb.x - pa.x };
                split.push(Half {
                    r: Ref::Degenerate { v, span },
                    flip: false,
                    uv: vec![pa, pb],
                    from: prev.1,
                    to: n,
                    boundary: h.boundary,
                });
                prev = (s, n);
            }
        } else {
            split.push(h);
        }
    }
    let mut halves = split;
    // Degenerate spans follow the parameter they were drawn in.
    for h in &mut halves {
        if let Ref::Degenerate { v, .. } = h.r {
            let dx = h.uv[1].x - h.uv[0].x;
            h.r = Ref::Degenerate {
                v,
                span: if flip_u { -dx } else { dx },
            };
        }
    }
    // Prune dangling inner pieces.
    loop {
        let mut degree = vec![0usize; nodes.len()];
        for h in &halves {
            degree[h.from] += 1;
        }
        let before = halves.len();
        halves.retain(|h| h.boundary.is_some() || (degree[h.from] > 1 && degree[h.to] > 1));
        if halves.len() == before {
            break;
        }
    }
    // Outgoing halves at each node, by angle.
    let dir_at = |h: &Half, frac: f64| -> f64 {
        let n = h.uv.len();
        let target = if frac <= 0.0 {
            // First sample that moves.
            (1..n)
                .map(|i| h.uv[i])
                .find(|q| (*q - h.uv[0]).len() > 1e-12)
                .unwrap_or(h.uv[n - 1])
        } else {
            let i = ((n - 1) as f64 * frac).ceil() as usize;
            h.uv[i.clamp(1, n - 1)]
        };
        let d = target - h.uv[0];
        d.y.atan2(d.x)
    };
    let mut out_at: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, h) in halves.iter().enumerate() {
        out_at[h.from].push(i);
    }
    for list in &mut out_at {
        list.sort_by(|&x, &y| {
            let (a1, b1) = (dir_at(&halves[x], 0.0), dir_at(&halves[y], 0.0));
            if (a1 - b1).abs() > 1e-9 {
                return a1.total_cmp(&b1);
            }
            let (a2, b2) = (dir_at(&halves[x], 0.3), dir_at(&halves[y], 0.3));
            a2.total_cmp(&b2).then(x.cmp(&y))
        });
    }
    // Twin of each half: same piece, opposite direction, reversed ends.
    let twin = |i: usize, halves: &Vec<Half>| -> Option<usize> {
        let h = &halves[i];
        halves.iter().position(|g| {
            g.from == h.to && g.to == h.from && g.boundary.map(|b| !b) == h.boundary
                && match (g.r, h.r) {
                    (Ref::Piece(a), Ref::Piece(b)) => a == b && g.flip != h.flip,
                    (Ref::Degenerate { v: a, span: s }, Ref::Degenerate { v: b, span: t }) => {
                        a == b && (s + t).abs() < 1e-9 && g.uv[0] == h.uv[1]
                    }
                    _ => false,
                }
        })
    };
    let twins: Vec<Option<usize>> = (0..halves.len()).map(|i| twin(i, &halves)).collect();
    // Trace cycles, face on the left: after arriving at a node along h, leave by the
    // half just clockwise of h's twin.
    let mut used = vec![false; halves.len()];
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    for start in 0..halves.len() {
        if used[start] {
            continue;
        }
        let mut cyc = Vec::new();
        let mut h = start;
        let mut ok = true;
        for _ in 0..halves.len() + 1 {
            if used[h] {
                ok = h == start;
                break;
            }
            used[h] = true;
            cyc.push(h);
            let Some(t) = twins[h] else {
                ok = false;
                break;
            };
            let list = &out_at[halves[h].to];
            let pos = list.iter().position(|x| *x == t).ok_or("broken arrangement")?;
            h = list[(pos + list.len() - 1) % list.len()];
        }
        if ok && !cyc.is_empty() {
            cycles.push(cyc);
        }
    }
    let poly = |c: &Vec<usize>| -> Vec<V2> {
        let mut p = Vec::new();
        for &h in c {
            let uv = &halves[h].uv;
            p.extend_from_slice(&uv[..uv.len() - 1]);
        }
        p
    };
    let area = |p: &[V2]| {
        let n = p.len();
        (0..n).map(|i| p[i].cross(p[(i + 1) % n])).sum::<f64>() / 2.0
    };
    let mut outers: Vec<(usize, Vec<V2>, f64)> = Vec::new();
    let mut holes: Vec<(usize, Vec<V2>)> = Vec::new();
    for (ci, c) in cycles.iter().enumerate() {
        let against = c.iter().any(|h| halves[*h].boundary == Some(false));
        let p = poly(c);
        let a = area(&p);
        if against {
            continue;
        }
        if a > 0.0 {
            outers.push((ci, p, a));
        } else {
            holes.push((ci, p));
        }
    }
    let mut regions: Vec<(usize, Vec<usize>)> = outers.iter().map(|o| (o.0, vec![])).collect();
    for (ci, hp) in &holes {
        // A point just beside the hole, on the region's side (the cycle's left).
        let Some(probe) = (0..hp.len()).find_map(|i| {
            let (a, b) = (hp[i], hp[(i + 1) % hp.len()]);
            let d = b - a;
            let l = d.len();
            (l > 0.0).then(|| (a + b) * 0.5 + d.perp() / l * (l * 1e-3).min(1e-4))
        }) else {
            continue;
        };
        let best = outers
            .iter()
            .enumerate()
            .filter(|(_, o)| point_in_polys(probe, std::slice::from_ref(&o.1)))
            .min_by(|x, y| x.1 .2.total_cmp(&y.1 .2))
            .map(|(i, _)| i);
        if let Some(i) = best {
            regions[i].1.push(*ci);
        }
    }
    let mut result = Vec::new();
    for (oi, hs) in regions {
        let mut loops_out: Vec<Vec<(Ref, bool)>> = Vec::new();
        let mut polys: Vec<Vec<V2>> = Vec::new();
        for ci in std::iter::once(oi).chain(hs.iter().copied()) {
            let c = &cycles[ci];
            loops_out.push(c.iter().map(|h| (halves[*h].r, halves[*h].flip)).collect());
            polys.push(poly(c));
        }
        let point = region_point(&polys).map(|q| {
            let uq = if flip_u { v2(-q.x, q.y) } else { q };
            (surf.eval(uq.x, uq.y), uq)
        });
        let _ = tol;
        result.push((loops_out, point));
    }
    Ok(result)
}

/// A point well inside a region (polygons in the parameter plane, holes included).
fn region_point(polys: &[Vec<V2>]) -> Option<V2> {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in polys.iter().flatten() {
        lo = lo.min(p.y);
        hi = hi.max(p.y);
    }
    if !(hi > lo) {
        return None;
    }
    let mut best: Option<(f64, V2)> = None;
    let scans = 17;
    for k in 1..scans {
        let frac = (k as f64 + 0.137 * ((k * 7) % 5) as f64) / (scans as f64 + 0.7);
        let y = lo + (hi - lo) * frac;
        let mut xs: Vec<f64> = Vec::new();
        for pts in polys {
            let n = pts.len();
            for i in 0..n {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks(2) {
            if pair.len() == 2 {
                let w = pair[1] - pair[0];
                if best.is_none_or(|b| w > b.0) {
                    best = Some((w, v2((pair[0] + pair[1]) / 2.0, y)));
                }
            }
        }
    }
    best.map(|b| b.1)
}

#[allow(dead_code)]
fn unused(_: &[V3], _: usize) -> f64 {
    let _ = edge_distance;
    let _ = loop_polygon;
    0.0
}
