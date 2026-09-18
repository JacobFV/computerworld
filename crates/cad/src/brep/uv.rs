//! Faces in their surface's parameter plane: every coedge's samples mapped to (u, v),
//! unwrapped across periods so each loop is a closed polygon, which is what point-in-face
//! tests, tessellation and the mass integrals work on. Nothing here is stored: it is
//! derived from the exact 3D geometry whenever it is needed.
use super::geom::Surface;
use super::topo::{edge_params, Box3, Coedge, Solid, TOL};
use crate::math::{v2, V2, V3};

#[derive(Clone, Debug)]
pub struct CoUV {
    pub co: Coedge,
    /// Edge parameters of the samples, in traversal order.
    pub ts: Vec<f64>,
    pub pts: Vec<V3>,
    pub uv: Vec<V2>,
}

#[derive(Clone, Debug)]
pub struct FaceUV {
    /// Loops with the outer one first.
    pub loops: Vec<Vec<CoUV>>,
    /// +1 when `∂S/∂u × ∂S/∂v` points out of the solid, −1 when into it.
    pub sense: f64,
    pub lo: V2,
    pub hi: V2,
    /// Every loop closed in the parameter plane.
    pub valid: bool,
}

/// Whether the surface's u is undefined at `p` (a pole or apex).
fn singular(s: &Surface, p: V3) -> bool {
    match s {
        Surface::Sphere { f, r } | Surface::Cone { f, r, .. } => {
            let l = f.to_local(p);
            let _ = r;
            let scale = 1.0 + l.z.abs() + r.abs();
            (l.x * l.x + l.y * l.y).sqrt() < 1e-9 * scale
        }
        Surface::Revolution { f, .. } => {
            let l = f.to_local(p);
            (l.x * l.x + l.y * l.y).sqrt() < 1e-9 * (1.0 + l.z.abs())
        }
        Surface::Torus { f, major, minor } if major <= minor => {
            let l = f.to_local(p);
            (l.x * l.x + l.y * l.y).sqrt() < 1e-9 * (1.0 + major)
        }
        _ => false,
    }
}

/// Whether the surface's u is undefined at `p` (a pole or apex).
pub fn singular_point(s: &Surface, p: V3) -> bool {
    singular(s, p)
}

fn wrap_near(x: f64, near: f64, period: Option<f64>) -> f64 {
    match period {
        Some(p) => x + ((near - x) / p).round() * p,
        None => x,
    }
}

/// The parameter-plane picture of face `f`.
pub fn face_uv(s: &Solid, f: usize) -> FaceUV {
    let face = &s.faces[f];
    let surf = &face.surface;
    let (pu, pv) = (surf.period_u(), surf.period_v());
    let mut loops: Vec<Vec<CoUV>> = Vec::new();
    let mut valid = true;
    for l in &face.loops {
        let n = l.len();
        // Start at a real edge: a degenerate one takes its start from what precedes it.
        let k0 = (0..n).find(|k| !s.edges[l[*k].edge].degenerate).unwrap_or(0);
        let mut out: Vec<Option<CoUV>> = vec![None; n];
        let mut prev: Option<V2> = None;
        for step in 0..n {
            let k = (k0 + step) % n;
            let c = l[k];
            let e = &s.edges[c.edge];
            if e.degenerate {
                let p = s.vertices[e.v0].p;
                let start = prev.unwrap_or(V2::ZERO);
                let span = (e.t1 - e.t0) * if c.rev { -1.0 } else { 1.0 };
                let end = start + v2(span, 0.0);
                out[k] = Some(CoUV {
                    co: c,
                    ts: if c.rev { vec![e.t1, e.t0] } else { vec![e.t0, e.t1] },
                    pts: vec![p, p],
                    uv: vec![start, end],
                });
                prev = Some(end);
                continue;
            }
            let mut ts = edge_params(e);
            if c.rev {
                ts.reverse();
            }
            let pts: Vec<V3> = ts.iter().map(|t| e.point(*t)).collect();
            let mut res: Vec<V2> = Vec::with_capacity(pts.len());
            let mut seed: Option<(f64, f64)> = prev.map(|p| (p.x, p.y));
            let mut last = prev;
            for p in &pts {
                if singular(surf, *p) {
                    let (_, v) = surf.project(*p);
                    let v = match last {
                        Some(r) => wrap_near(v, r.y, pv),
                        None => v,
                    };
                    res.push(v2(f64::NAN, v));
                    continue;
                }
                let (u, v) = match seed {
                    Some(sd) => surf.project_near(*p, sd),
                    None => surf.project(*p),
                };
                seed = Some((u, v));
                let mut q = v2(u, v);
                if let Some(r) = last {
                    q.x = wrap_near(q.x, r.x, pu);
                    q.y = wrap_near(q.y, r.y, pv);
                }
                last = Some(q);
                res.push(q);
            }
            // Singular samples take the u of their nearest defined neighbour.
            for i in 0..res.len() {
                if res[i].x.is_nan() {
                    let near = (0..res.len())
                        .filter(|j| !res[*j].x.is_nan())
                        .min_by_key(|j| (*j as i64 - i as i64).abs())
                        .map(|j| res[j].x)
                        .or(prev.map(|p| p.x))
                        .unwrap_or(0.0);
                    res[i].x = near;
                }
            }
            prev = res.last().copied();
            out[k] = Some(CoUV {
                co: c,
                ts,
                pts,
                uv: res,
            });
        }
        let out: Vec<CoUV> = out.into_iter().flatten().collect();
        // The loop must close in the parameter plane.
        if let (Some(first), Some(last)) = (
            out.get(k0).and_then(|c| c.uv.first().copied()),
            prev,
        ) {
            let scale = 1.0 + first.x.abs() + first.y.abs();
            if (last - first).len() > 1e-6 * scale {
                valid = false;
            }
        }
        loops.push(out);
    }
    // Outer loop: the largest by area; inner loops moved into its period window.
    let areas: Vec<f64> = loops.iter().map(|l| loop_area(l)).collect();
    if let Some(outer) = (0..loops.len()).max_by(|a, b| {
        areas[*a]
            .abs()
            .total_cmp(&areas[*b].abs())
            .then(b.cmp(a))
    }) {
        loops.swap(0, outer);
    }
    let (mut lo, mut hi) = (
        v2(f64::INFINITY, f64::INFINITY),
        v2(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    if let Some(outer) = loops.first() {
        for c in outer {
            for q in &c.uv {
                lo = v2(lo.x.min(q.x), lo.y.min(q.y));
                hi = v2(hi.x.max(q.x), hi.y.max(q.y));
            }
        }
    }
    let center = (lo + hi) * 0.5;
    for l in loops.iter_mut().skip(1) {
        let n: usize = l.iter().map(|c| c.uv.len()).sum();
        if n == 0 {
            continue;
        }
        let mean = l
            .iter()
            .flat_map(|c| c.uv.iter())
            .fold(V2::ZERO, |a, b| a + *b)
            / n as f64;
        let du = pu.map_or(0.0, |p| ((center.x - mean.x) / p).round() * p);
        let dv = pv.map_or(0.0, |p| ((center.y - mean.y) / p).round() * p);
        if du != 0.0 || dv != 0.0 {
            for c in l.iter_mut() {
                for q in c.uv.iter_mut() {
                    *q = *q + v2(du, dv);
                }
            }
        }
        for c in l.iter() {
            for q in &c.uv {
                lo = v2(lo.x.min(q.x), lo.y.min(q.y));
                hi = v2(hi.x.max(q.x), hi.y.max(q.y));
            }
        }
    }
    let sense = if areas.iter().map(|a| a.abs()).fold(0.0, f64::max) == 0.0 {
        1.0
    } else if loop_area(&loops[0]) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    FaceUV {
        loops,
        sense,
        lo,
        hi,
        valid,
    }
}

/// Signed area of a loop in the parameter plane.
pub fn loop_area(l: &[CoUV]) -> f64 {
    let pts: Vec<V2> = loop_polygon(l);
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    (0..n).map(|i| pts[i].cross(pts[(i + 1) % n])).sum::<f64>() / 2.0
}

/// The loop as one closed polygon (each coedge's last sample dropped: it is the next
/// one's first).
pub fn loop_polygon(l: &[CoUV]) -> Vec<V2> {
    let mut out = Vec::new();
    for c in l {
        let k = c.uv.len();
        out.extend_from_slice(&c.uv[..k.saturating_sub(1)]);
    }
    out
}

/// Where a point lies relative to a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    Inside,
    Boundary,
    Outside,
}

/// Distance from `p` to edge `e` and the parameter of the nearest point.
pub fn edge_distance(s: &Solid, e: usize, p: V3) -> (f64, f64) {
    let ed = &s.edges[e];
    if ed.degenerate {
        return (s.vertices[ed.v0].p.dist(p), ed.t0);
    }
    let t = ed.curve.param_in(p, ed.t0, ed.t1);
    let tc = t.clamp(ed.t0.min(ed.t1), ed.t0.max(ed.t1));
    let d = ed.point(tc).dist(p);
    // The ends too (projection may land on the far side of a closed curve).
    let (d0, d1) = (ed.start().dist(p), ed.end().dist(p));
    if d0 < d && d0 <= d1 {
        (d0, ed.t0)
    } else if d1 < d {
        (d1, ed.t1)
    } else {
        (d, tc)
    }
}

/// Classify a point lying on the face's surface against the face.
pub fn classify(s: &Solid, fu: &FaceUV, f: usize, p: V3, tol: f64) -> Where {
    let face = &s.faces[f];
    // Nearest boundary point.
    let mut best: Option<(f64, usize, usize, f64)> = None;
    for (li, l) in face.loops.iter().enumerate() {
        for (ci, c) in l.iter().enumerate() {
            let (d, t) = edge_distance(s, c.edge, p);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, li, ci, t));
            }
        }
    }
    let Some((dist, li, ci, t)) = best else {
        return Where::Outside;
    };
    if dist <= tol {
        return Where::Boundary;
    }
    // Close to one edge's interior: decide by the side of that edge, exactly.
    let c = face.loops[li][ci];
    let e = &s.edges[c.edge];
    let interior = !e.degenerate && {
        let lo = e.t0.min(e.t1);
        let hi = e.t0.max(e.t1);
        let span = hi - lo;
        t > lo + span * 1e-6 && t < hi - span * 1e-6
    };
    let chord = s.edge_box(c.edge).diagonal() * 4e-3 + 50.0 * TOL;
    if interior && dist < chord {
        let (q, d) = e.curve.d1(t);
        let tan = if c.rev { -d } else { d };
        let (u, v) = face.surface.project(q);
        let n = face.surface.normal(u, v) * fu.sense;
        let inward = n.cross(tan);
        return if (p - q).dot(inward) > 0.0 {
            Where::Inside
        } else {
            Where::Outside
        };
    }
    if inside_uv(s, fu, f, p) {
        Where::Inside
    } else {
        Where::Outside
    }
}

/// Parameter-plane containment (even-odd over all loops), trying every period copy of
/// the point that falls in the face's window.
pub fn inside_uv(s: &Solid, fu: &FaceUV, f: usize, p: V3) -> bool {
    let surf = &s.faces[f].surface;
    let (u, v) = surf.project(p);
    let cands = |x: f64, lo: f64, hi: f64, per: Option<f64>| -> Vec<f64> {
        match per {
            Some(pp) => {
                let mut out = Vec::new();
                let k0 = ((lo - x) / pp).floor() as i64 - 1;
                for k in k0..k0 + 4 {
                    let y = x + k as f64 * pp;
                    if y >= lo - 1e-9 && y <= hi + 1e-9 {
                        out.push(y);
                    }
                }
                if out.is_empty() {
                    out.push(wrap_near(x, (lo + hi) / 2.0, per));
                }
                out
            }
            None => vec![x],
        }
    };
    let polys: Vec<Vec<V2>> = fu.loops.iter().map(|l| loop_polygon(l)).collect();
    for uu in cands(u, fu.lo.x, fu.hi.x, surf.period_u()) {
        for vv in cands(v, fu.lo.y, fu.hi.y, surf.period_v()) {
            if point_in_polys(v2(uu, vv), &polys) {
                return true;
            }
        }
    }
    false
}

pub fn point_in_polys(q: V2, polys: &[Vec<V2>]) -> bool {
    let mut inside = false;
    for pts in polys {
        let n = pts.len();
        if n < 3 {
            continue;
        }
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (pts[i], pts[j]);
            if (a.y > q.y) != (b.y > q.y) {
                let x = a.x + (q.y - a.y) * (b.x - a.x) / (b.y - a.y);
                if q.x < x {
                    inside = !inside;
                }
            }
            j = i;
        }
    }
    inside
}

/// A face's box, including where a curved face bulges beyond its boundary.
pub fn face_box(s: &Solid, fu: &FaceUV, f: usize) -> Box3 {
    let mut b = s.boundary_box(f);
    let surf = &s.faces[f].surface;
    let bulges = !matches!(
        surf,
        Surface::Plane { .. } | Surface::Cylinder { .. } | Surface::Cone { .. }
    );
    if bulges && fu.lo.x.is_finite() {
        let polys: Vec<Vec<V2>> = fu.loops.iter().map(|l| loop_polygon(l)).collect();
        let n = 24;
        for i in 0..=n {
            for j in 0..=n {
                let q = v2(
                    fu.lo.x + (fu.hi.x - fu.lo.x) * i as f64 / n as f64,
                    fu.lo.y + (fu.hi.y - fu.lo.y) * j as f64 / n as f64,
                );
                if point_in_polys(q, &polys) {
                    b.add(surf.eval(q.x, q.y));
                }
            }
        }
        b = b.grow(b.diagonal() * 0.02);
    }
    b
}

/// A point well inside a face (for classifying it), found on the parameter-plane
/// polygon: the middle of the widest interior run of a horizontal scan line.
pub fn interior_point(s: &Solid, fu: &FaceUV, f: usize) -> Option<(V3, V2)> {
    let polys: Vec<Vec<V2>> = fu.loops.iter().map(|l| loop_polygon(l)).collect();
    let (lo, hi) = (fu.lo, fu.hi);
    if !(hi.y > lo.y) {
        return None;
    }
    let mut best: Option<(f64, V2)> = None;
    let scans = 13;
    for k in 1..scans {
        // Irregular fractions avoid landing on vertices.
        let frac = (k as f64 + 0.137 * ((k * 7) % 5) as f64) / (scans as f64 + 0.7);
        let y = lo.y + (hi.y - lo.y) * frac;
        let mut xs: Vec<f64> = Vec::new();
        for pts in &polys {
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
    let (_, q) = best?;
    let _ = f;
    Some((s.faces[f].surface.eval(q.x, q.y), q))
}
