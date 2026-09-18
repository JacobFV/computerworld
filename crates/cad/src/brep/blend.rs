//! Fillets and chamfers on B-rep edges.
//!
//! Each selected edge gets a tool body: the material between the two faces and the
//! rolling ball (or the chamfer's flat), swept along the edge. Convex edges' tools are
//! cut away, concave edges' tools are added, both by the exact booleans, so the rounds
//! meet their neighbours exactly. Three kinds of edge are handled:
//!
//! - *Prismatic*: a straight edge between faces swept along it (planes and cylinders
//!   parallel to the edge) — the section across the edge is the same everywhere, so the
//!   tool is an extrusion of a 2D section bounded by lines and arcs.
//! - *Revolved*: a circular edge between faces of revolution about the circle's axis
//!   (planes across it, coaxial cylinders, cones, spheres, tori) — the tool is the 2D
//!   section in the half-plane, revolved.
//! - *General*: any closed edge between faces with distance fields (the intersection of
//!   two cylinders, a cylinder cut obliquely): the ball's centre runs along the curve
//!   where the two faces' surfaces offset by the radius meet, the round is the tube
//!   round that spine, and the tool is bounded by the tube and the two faces between
//!   the edge and the contact curves.
//!
//! Where three rounded convex edges meet at a corner of three planes, the corner is
//! blended by the ball rolling into it (a spherical patch), or for chamfers cut by the
//! plane through the three chamfer corners, as OpenCascade blends a box corner.
use super::boolean::{boolean, Op};
use super::build::{extrude, revolve, Region2, Seg2, Wire2};
use super::geom::{Curve, Surface, TraceKind, Traced};
use super::intersect::Prepared;
use super::topo::{Box3, Coedge, Face, Solid, TOL};
use super::uv::{classify, Where};
use crate::math::{self, v2, Frame, PI, TAU, V2, V3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dress {
    Fillet(f64),
    /// Distances along the first and second face.
    Chamfer(f64, f64),
}

/// A trace of a face in a 2D section.
#[derive(Clone, Copy, Debug)]
enum Trace {
    /// Through `p` along `d`.
    Line { p: V2, d: V2 },
    Circle { c: V2, r: f64 },
}

fn wrap_pi(a: f64) -> f64 {
    math::wrap_angle(a)
}

/// The face side of an edge: the face, its outward normal and into-face direction at `p`.
struct Side {
    face: usize,
    n: V3,
    into: V3,
}

fn side_at(s: &Solid, prep: &Prepared, e: usize, face: usize, p: V3, t: f64) -> Option<Side> {
    let f = &s.faces[face];
    let co = f.loops.iter().flatten().find(|c| c.edge == e)?;
    let (_, d) = s.edges[e].curve.d1(t);
    let tan = if co.rev { -d } else { d }.norm();
    let (u, v) = f.surface.project(p);
    let n = f.surface.normal(u, v) * prep.uvs[face].sense;
    Some(Side {
        face,
        n,
        into: n.cross(tan).norm(),
    })
}

/// The 2D section of the change a dress makes at an edge: `e` the edge point, traces
/// and into-face directions and outward normals of both faces (all in the section's
/// coordinates). Returns the region and the two contact points.
#[allow(clippy::too_many_arguments)]
fn section(
    e: V2,
    tr: [Trace; 2],
    into: [V2; 2],
    n: [V2; 2],
    convex: bool,
    dress: Dress,
    what: &str,
) -> Result<(Region2, [V2; 2]), String> {
    let (t1, t2) = (into[0].norm(), into[1].norm());
    let phi = math::acos(t1.dot(t2).clamp(-1.0, 1.0));
    if phi < 1e-3 || phi > PI - 1e-3 {
        return Err(format!("{what}: the faces at this edge are tangent"));
    }
    // Along a trace from the edge point into the face: segment to the point `q`.
    let along = |k: usize, from: V2, to: V2| -> Seg2 {
        match tr[k] {
            Trace::Line { .. } => Seg2::Line(from, to),
            Trace::Circle { c, r } => {
                let a0 = (from - c).angle();
                let sweep = wrap_pi((to - c).angle() - a0);
                Seg2::Arc { c, r, a0, sweep }
            }
        }
    };
    let (mut segs, contacts) = match dress {
        Dress::Fillet(r) => {
            let s = if convex { -1.0 } else { 1.0 };
            // Offset traces towards the ball.
            let off = |k: usize| -> Trace {
                match tr[k] {
                    Trace::Line { p, d } => Trace::Line {
                        p: p + n[k] * (s * r),
                        d,
                    },
                    Trace::Circle { c, r: rr } => {
                        let outward = (e - c).dot(n[k]) > 0.0;
                        let nr = if outward { rr + s * r } else { rr - s * r };
                        Trace::Circle { c, r: nr }
                    }
                }
            };
            let cands = trace_hits(off(0), off(1));
            // The centre on the material's side (convex) or the air's (concave): the
            // candidate nearest the edge in the wedge's bisector direction.
            let bis = (t1 + t2).norm();
            let c = cands
                .into_iter()
                .filter(|c| c.x.is_finite() && (*c - e).dot(bis) > 0.0)
                .min_by(|a, b| a.dist(e).total_cmp(&b.dist(e)))
                .ok_or(format!("{what}: no ball of this radius fits the edge"))?;
            let foot = |k: usize| -> V2 {
                match tr[k] {
                    Trace::Line { p, d } => {
                        let d = d.norm();
                        p + d * (c - p).dot(d)
                    }
                    Trace::Circle { c: q, r: rr } => q + (c - q).norm() * rr,
                }
            };
            let (p1, p2) = (foot(0), foot(1));
            let a0 = (p1 - c).angle();
            let sweep = wrap_pi((p2 - c).angle() - a0);
            (
                vec![
                    along(0, e, p1),
                    Seg2::Arc {
                        c,
                        r: r.abs(),
                        a0,
                        sweep,
                    },
                    along(1, p2, e),
                ],
                [p1, p2],
            )
        }
        Dress::Chamfer(d1, d2) => {
            let at = |k: usize, d: f64, t: V2| -> V2 {
                match tr[k] {
                    Trace::Line { .. } => e + t * d,
                    Trace::Circle { c, r } => {
                        // Arc length d from the edge point, turning towards `t`.
                        let a0 = (e - c).angle();
                        let dir = if (e - c).perp().dot(t) > 0.0 { 1.0 } else { -1.0 };
                        c + V2::polar(a0 + dir * d / r, r)
                    }
                }
            };
            let (p1, p2) = (at(0, d1, t1), at(1, d2, t2));
            (
                vec![along(0, e, p1), Seg2::Line(p1, p2), along(1, p2, e)],
                [p1, p2],
            )
        }
    };
    segs.retain(|g| g.full() || g.start().dist(g.end()) > 1e-12);
    let w = Wire2 { segs };
    let w = if w.area() < 0.0 { w.reversed() } else { w };
    Ok((
        Region2 {
            outer: w,
            holes: vec![],
        },
        contacts,
    ))
}

fn trace_hits(a: Trace, b: Trace) -> Vec<V2> {
    match (a, b) {
        (Trace::Line { p, d }, Trace::Line { p: q, d: e }) => {
            let den = d.cross(e);
            if den.abs() < 1e-14 {
                return vec![];
            }
            vec![p + d * ((q - p).cross(e) / den)]
        }
        (Trace::Line { p, d }, Trace::Circle { c, r }) | (Trace::Circle { c, r }, Trace::Line { p, d }) => {
            let d = d.norm();
            let w = p - c;
            super::num::quadratic(1.0, 2.0 * w.dot(d), w.dot(w) - r * r)
                .into_iter()
                .map(|t| p + d * t)
                .collect()
        }
        (Trace::Circle { c: c1, r: r1 }, Trace::Circle { c: c2, r: r2 }) => {
            let dv = c2 - c1;
            let dist = dv.len();
            if dist < 1e-14 || dist > r1 + r2 || dist < (r1 - r2).abs() {
                return vec![];
            }
            let a = (r1 * r1 - r2 * r2 + dist * dist) / (2.0 * dist);
            let h = math::sqrt((r1 * r1 - a * a).max(0.0));
            let m = c1 + dv * (a / dist);
            let perp = dv.perp() / dist;
            vec![m + perp * h, m - perp * h]
        }
    }
}

/// A half-space bounded by a face's surface, as a solid big enough to cover `region`:
/// the side opposite the face's outward normal at `p`.
fn inner_half_space(s: &Solid, prep: &Prepared, face: usize, p: V3, region: &Box3) -> Option<Solid> {
    let f = &s.faces[face];
    let size = region.diagonal() * 4.0 + 10.0;
    match &f.surface {
        Surface::Plane { .. } => {
            let (u, v) = f.surface.project(p);
            let n = f.surface.normal(u, v) * prep.uvs[face].sense;
            let q = f.surface.eval(u, v);
            // A square around the region's centre, projected into the plane.
            let c = region.min.lerp(region.max, 0.5);
            let c = c - n * (c - q).dot(n);
            let fr = Frame::from_normal(c, -n, n.any_perp());
            let r = super::build::polygon_region(&[
                v2(-size, -size),
                v2(size, -size),
                v2(size, size),
                v2(-size, size),
            ]);
            Some(extrude(&[r], &fr, 0.0, size))
        }
        Surface::Cylinder { f: cf, r } => {
            // Material inside the cylinder: the cylinder itself; outside: a box minus it.
            let (u, v) = f.surface.project(p);
            let outward_radial = f.surface.normal(u, v) * prep.uvs[face].sense;
            let radial = (p - cf.origin) - cf.z * (p - cf.origin).dot(cf.z);
            let base = cf.origin + cf.z * ((region.min.lerp(region.max, 0.5) - cf.origin).dot(cf.z) - size);
            let cyl = super::build::cylinder(base, cf.z, *r, 2.0 * size);
            if outward_radial.dot(radial) > 0.0 {
                Some(cyl)
            } else {
                let c = region.min.lerp(region.max, 0.5);
                let e = V3 {
                    x: size,
                    y: size,
                    z: size,
                };
                let bx = super::build::cuboid(c - e, c + e);
                boolean(&bx, &cyl, Op::Difference).ok()
            }
        }
        _ => None,
    }
}

/// The faces at a vertex other than the edge's own two.
fn end_faces(s: &Solid, e: usize, v: usize) -> Vec<usize> {
    let (fa, fb) = s.edge_faces(e).unwrap_or((usize::MAX, usize::MAX));
    let mut out = Vec::new();
    for (fi, f) in s.faces.iter().enumerate() {
        if fi == fa || fi == fb {
            continue;
        }
        let touches = f.loops.iter().flatten().any(|c| {
            let ed = &s.edges[c.edge];
            ed.v0 == v || ed.v1 == v
        });
        if touches {
            out.push(fi);
        }
    }
    out
}

struct Tool {
    solid: Solid,
    convex: bool,
    /// For corner blends: the edge's section kind and its faces.
    prismatic: bool,
    faces: (usize, usize),
    /// Contact points of the section at the edge's start and end.
    ends: [(V3, V3); 2],
}

/// Clip a tool extended past the edge's ends by the end faces at them.
#[allow(clippy::too_many_arguments)]
fn clip_ends(
    s: &Solid,
    prep: &Prepared,
    e: usize,
    mut tool: Solid,
    what: &str,
) -> Result<Solid, String> {
    let ed = &s.edges[e];
    let region = tool.bounds();
    for (v, p) in [(ed.v0, ed.start()), (ed.v1, ed.end())] {
        for g in end_faces(s, e, v) {
            let h = inner_half_space(s, prep, g, p, &region).ok_or(format!(
                "{what}: the edge ends on a {} face this fillet cannot stop at",
                s.faces[g].surface.name().to_lowercase()
            ))?;
            tool = boolean(&tool, &h, Op::Intersection)?;
        }
    }
    Ok(tool)
}

/// Whether a point lies on a face (inside or on its boundary).
fn on_face(s: &Solid, prep: &Prepared, f: usize, p: V3) -> bool {
    classify(s, &prep.uvs[f], f, p, TOL * 100.0) != Where::Outside
}

fn prismatic_tool(
    s: &Solid,
    prep: &Prepared,
    e: usize,
    dress: Dress,
    what: &str,
) -> Result<Option<Tool>, String> {
    let ed = &s.edges[e];
    let Curve::Line { d, .. } = ed.curve else {
        return Ok(None);
    };
    let (fa, fb) = s.edge_faces(e).ok_or(format!("{what}: the edge has no faces"))?;
    let (sa, sb) = (&s.faces[fa].surface, &s.faces[fb].surface);
    if !(sa.extruded_along(d) && sb.extruded_along(d)) || fa == fb {
        return Ok(None);
    }
    let p0 = ed.start();
    let len = ed.end().dist(p0);
    let a = side_at(s, prep, e, fa, p0, ed.t0).ok_or("edge not in face")?;
    let b = side_at(s, prep, e, fb, p0, ed.t0).ok_or("edge not in face")?;
    let convex = a.into.dot(b.n) < 0.0;
    let x = a.into;
    let y = d.cross(x).norm();
    let to2 = |w: V3| v2(w.dot(x), w.dot(y));
    let at2 = |q: V3| to2(q - p0);
    let trace = |surf: &Surface, into: V3| -> Trace {
        match surf {
            Surface::Cylinder { f, r } => Trace::Circle { c: at2(f.origin), r: *r },
            _ => Trace::Line {
                p: V2::ZERO,
                d: to2(into),
            },
        }
    };
    let (region, contacts) = section(
        V2::ZERO,
        [trace(sa, a.into), trace(sb, b.into)],
        [to2(a.into), to2(b.into)],
        [to2(a.n), to2(b.n)],
        convex,
        dress,
        what,
    )?;
    // The contacts must lie on their faces along the whole edge.
    let back = |q: V2, s: f64| p0 + x * q.x + y * q.y + d * s;
    for sfrac in [0.25, 0.5, 0.75] {
        let (c1, c2) = (back(contacts[0], len * sfrac), back(contacts[1], len * sfrac));
        if !on_face(s, prep, fa, c1) || !on_face(s, prep, fb, c2) {
            return Err(format!("{what}: the size is too large for this edge"));
        }
    }
    let frame = Frame {
        origin: p0,
        x,
        y,
        z: d,
    };
    // Ends square to the edge and flush with a planar end face need no clipping.
    let square = |v: usize, p: V3| -> bool {
        let ends = end_faces(s, e, v);
        !ends.is_empty()
            && ends.iter().all(|&g| match &s.faces[g].surface {
                Surface::Plane { f } => f.z.cross(d).len() < 1e-9 && (p - f.origin).dot(f.z).abs() < TOL,
                _ => false,
            })
    };
    let margin = region.outer.segs.iter().map(|g| g.start().len()).fold(0.0, f64::max) * 2.0 + 1.0;
    let (sq0, sq1) = (square(ed.v0, p0), square(ed.v1, ed.end()));
    let z0 = if sq0 { 0.0 } else { -margin };
    let z1 = if sq1 { len } else { len + margin };
    let mut tool = extrude(&[region], &frame, z0, z1);
    if !(sq0 && sq1) {
        tool = clip_ends(s, prep, e, tool, what)?;
    }
    Ok(Some(Tool {
        solid: tool,
        convex,
        prismatic: true,
        faces: (fa, fb),
        ends: [
            (back(contacts[0], 0.0), back(contacts[1], 0.0)),
            (back(contacts[0], len), back(contacts[1], len)),
        ],
    }))
}

fn revolved_tool(
    s: &Solid,
    prep: &Prepared,
    e: usize,
    dress: Dress,
    what: &str,
) -> Result<Option<Tool>, String> {
    let ed = &s.edges[e];
    let Curve::Circle { f: cf, .. } = &ed.curve else {
        return Ok(None);
    };
    let (o, k) = (cf.origin, cf.z);
    let (fa, fb) = s.edge_faces(e).ok_or(format!("{what}: the edge has no faces"))?;
    let (sa, sb) = (&s.faces[fa].surface, &s.faces[fb].surface);
    if fa == fb || !(sa.revolved_about(o, k) && sb.revolved_about(o, k)) {
        return Ok(None);
    }
    let p0 = ed.start();
    let rho = (p0 - o).norm();
    let a = side_at(s, prep, e, fa, p0, ed.t0).ok_or("edge not in face")?;
    let b = side_at(s, prep, e, fb, p0, ed.t0).ok_or("edge not in face")?;
    let convex = a.into.dot(b.n) < 0.0;
    let to2 = |w: V3| v2(w.dot(rho), w.dot(k));
    let at2 = |q: V3| to2(q - o);
    let trace = |surf: &Surface, into: V3| -> Trace {
        match surf {
            Surface::Sphere { f, r } => Trace::Circle { c: at2(f.origin), r: *r },
            Surface::Torus { f, major, minor } => Trace::Circle {
                c: at2(f.origin) + v2(*major, 0.0),
                r: *minor,
            },
            _ => Trace::Line {
                p: at2(p0),
                d: to2(into),
            },
        }
    };
    let e2 = at2(p0);
    let (region, contacts) = section(
        e2,
        [trace(sa, a.into), trace(sb, b.into)],
        [to2(a.into), to2(b.into)],
        [to2(a.n), to2(b.n)],
        convex,
        dress,
        what,
    )?;
    if region
        .outer
        .segs
        .iter()
        .any(|g| (0..=8).any(|i| g.point(i as f64 / 8.0).x <= 1e-9))
    {
        return Err(format!("{what}: the size is too large for this edge"));
    }
    let back = |q: V2, ang: f64| {
        let r = crate::math::Xform::rotate(o, k, ang);
        r.point(o + rho * q.x + k * q.y)
    };
    let span = ed.t1 - ed.t0;
    for frac in [0.25, 0.5, 0.75] {
        let ang = span * frac;
        if !on_face(s, prep, fa, back(contacts[0], ang)) || !on_face(s, prep, fb, back(contacts[1], ang)) {
            return Err(format!("{what}: the size is too large for this edge"));
        }
    }
    let frame = Frame {
        origin: o,
        x: rho,
        y: k,
        z: rho.cross(k).norm(),
    };
    let full = ed.closed();
    let tool = if full {
        revolve(&[region], &frame, o, k, 0.0, TAU)?
    } else {
        // An arc: flush with end planes containing the axis, else extended and clipped.
        let flush = |v: usize, p: V3| {
            let ends = end_faces(s, e, v);
            !ends.is_empty()
                && ends.iter().all(|&g| match &s.faces[g].surface {
                    Surface::Plane { f } => f.z.dot(k).abs() < 1e-9 && (o - f.origin).dot(f.z).abs() < TOL && (p - f.origin).dot(f.z).abs() < TOL,
                    _ => false,
                })
        };
        let (fl0, fl1) = (flush(ed.v0, p0), flush(ed.v1, ed.end()));
        let extra = 0.2f64.min((TAU - span) / 3.0);
        let start = if fl0 { 0.0 } else { -extra };
        let end = if fl1 { span } else { span + extra };
        let t = revolve(&[region], &frame, o, k, start, end - start)?;
        if fl0 && fl1 {
            t
        } else {
            clip_ends(s, prep, e, t, what)?
        }
    };
    let ends = [
        (back(contacts[0], 0.0), back(contacts[1], 0.0)),
        (back(contacts[0], span), back(contacts[1], span)),
    ];
    Ok(Some(Tool {
        solid: tool,
        convex,
        prismatic: false,
        faces: (fa, fb),
        ends,
    }))
}

/// Offset sign so that `sd = sign * r` is the surface moved `r` into the material
/// (`inside`) or out of it.
fn offset_sign(s: &Solid, prep: &Prepared, face: usize, p: V3, inside: bool) -> f64 {
    let f = &s.faces[face];
    let (u, v) = f.surface.project(p);
    let n_out = f.surface.normal(u, v) * prep.uvs[face].sense;
    let g = f.surface.grad(p);
    let out_positive = g.dot(n_out) > 0.0;
    match (out_positive, inside) {
        (true, true) | (false, false) => -1.0,
        _ => 1.0,
    }
}

/// The closed chain of edges between the same two faces through edge `e`, in order,
/// with each edge's direction.
fn closed_chain(s: &Solid, e: usize) -> Option<Vec<(usize, bool)>> {
    let faces = s.edge_faces(e)?;
    let key = |x: (usize, usize)| (x.0.min(x.1), x.0.max(x.1));
    let mut chain = vec![(e, true)];
    let start = s.edges[e].v0;
    let mut at = s.edges[e].v1;
    for _ in 0..s.edges.len() {
        if at == start {
            return Some(chain);
        }
        let next = (0..s.edges.len()).find(|&x| {
            !chain.iter().any(|c| c.0 == x)
                && !s.edges[x].degenerate
                && s.edge_faces(x).map(key) == Some(key(faces))
                && (s.edges[x].v0 == at || s.edges[x].v1 == at)
        })?;
        let fwd = s.edges[next].v0 == at;
        at = if fwd { s.edges[next].v1 } else { s.edges[next].v0 };
        chain.push((next, fwd));
    }
    None
}

/// A rolling-ball tool round a closed chain of edges between two faces of any kind.
fn general_tool(s: &Solid, prep: &Prepared, e: usize, dress: Dress, what: &str) -> Result<Tool, String> {
    let chain = closed_chain(s, e).ok_or(format!(
        "{what}: an edge between these faces can be rounded only where it closes on itself"
    ))?;
    let r = match dress {
        Dress::Fillet(r) => r,
        Dress::Chamfer(d1, d2) => (d1 + d2) / 2.0,
    };
    let (fa, fb) = s.edge_faces(e).ok_or(format!("{what}: the edge has no faces"))?;
    let (sa, sb) = (s.faces[fa].surface.clone(), s.faces[fb].surface.clone());
    let ed = &s.edges[e];
    let p0 = ed.start();
    let a = side_at(s, prep, e, fa, p0, ed.t0).ok_or("edge not in face")?;
    let b = side_at(s, prep, e, fb, p0, ed.t0).ok_or("edge not in face")?;
    let convex = a.into.dot(b.n) < 0.0;
    let oa = offset_sign(s, prep, fa, p0, convex) * r;
    let ob = offset_sign(s, prep, fb, p0, convex) * r;
    // Trace the spine along the chain: for samples of each edge, the point in its normal
    // plane at the two offsets.
    let per_edge = (256 / chain.len()).max(32);
    let mut guide: Vec<V3> = Vec::new();
    let mut prev: Option<V3> = None;
    for &(ce, fwd) in &chain {
        let cd = &s.edges[ce];
        for i in 0..per_edge {
            let f = i as f64 / per_edge as f64;
            let t = if fwd {
                cd.t0 + (cd.t1 - cd.t0) * f
            } else {
                cd.t1 - (cd.t1 - cd.t0) * f
            };
            let (q, dq) = cd.curve.d1(t);
            let dir = dq.norm();
            let mut x = match prev {
                Some(p) => p,
                None => {
                    let sec_a = side_at(s, prep, ce, fa, q, t).ok_or("edge not in face")?;
                    let sec_b = side_at(s, prep, ce, fb, q, t).ok_or("edge not in face")?;
                    let bis = (sec_a.into + sec_b.into).norm();
                    let half = math::acos(sec_a.into.dot(sec_b.into).clamp(-1.0, 1.0)) / 2.0;
                    q + bis * (r / math::sin(half.max(1e-3)))
                }
            };
            for _ in 0..60 {
                let fa_ = sa.sd(x) - oa;
                let fb_ = sb.sd(x) - ob;
                let fc = (x - q).dot(dir);
                let Some(step) =
                    super::num::solve3v([sa.grad(x), sb.grad(x), dir], [-fa_, -fb_, -fc])
                else {
                    break;
                };
                x += step;
                if step.len() < 1e-14 * (1.0 + x.len()) {
                    break;
                }
            }
            if (sa.sd(x) - oa).abs() > 1e-7 || (sb.sd(x) - ob).abs() > 1e-7 {
                return Err(format!("{what}: no ball of this radius rolls along the edge"));
            }
            guide.push(x);
            prev = Some(x);
        }
    }
    guide.push(guide[0]);
    let mut ts = Vec::with_capacity(guide.len());
    let mut acc = 0.0;
    for (i, p) in guide.iter().enumerate() {
        if i > 0 {
            acc += p.dist(guide[i - 1]);
        }
        ts.push(acc);
    }
    let len = acc;
    let spine = Curve::Traced(Box::new(Traced {
        kind: TraceKind::Inter {
            a: sa.clone(),
            oa,
            b: sb.clone(),
            ob,
        },
        pts: guide,
        ts,
        closed: true,
    }));
    let foot = |srf: &Surface| {
        Curve::Traced(Box::new(Traced {
            kind: TraceKind::Foot {
                spine: spine.clone(),
                s: srf.clone(),
            },
            pts: vec![],
            ts: vec![0.0, len],
            closed: true,
        }))
    };
    let (c1, c2) = (foot(&sa), foot(&sb));
    for frac in [0.0, 0.2, 0.4, 0.6, 0.8] {
        let t = len * frac;
        if !on_face(s, prep, fa, c1.eval(t)) || !on_face(s, prep, fb, c2.eval(t)) {
            return Err(format!("{what}: the size is too large for this edge"));
        }
    }
    let mut tool = Solid::default();
    // The chain's own edges, with their vertices.
    let mut vmap: std::collections::BTreeMap<usize, usize> = Default::default();
    let mut eloop = Vec::new();
    for &(ce, fwd) in &chain {
        let cd = s.edges[ce].clone();
        let v0 = *vmap
            .entry(cd.v0)
            .or_insert_with(|| tool.add_vertex(s.vertices[cd.v0].p));
        let v1 = *vmap
            .entry(cd.v1)
            .or_insert_with(|| tool.add_vertex(s.vertices[cd.v1].p));
        let ne = tool.add_edge(cd.curve.clone(), cd.t0, cd.t1, v0, v1);
        eloop.push(Coedge { edge: ne, rev: !fwd });
    }
    let v1 = tool.add_vertex(c1.eval(0.0));
    let v2_ = tool.add_vertex(c2.eval(0.0));
    let e1 = tool.add_edge(c1.clone(), 0.0, len, v1, v1);
    let e2 = tool.add_edge(c2.clone(), 0.0, len, v2_, v2_);
    let (sc, dsc) = spine.d1(0.0);
    let pipe = Surface::Pipe {
        spine: Box::new(spine.clone()),
        r,
        toward: Box::new(sa.clone()),
        t0: 0.0,
        t1: len,
    };
    let mut faces = Vec::new();
    match dress {
        Dress::Fillet(_) => {
            // The seam of the tube: the ball's section at the start, contact to contact.
            let nrm = dsc.norm();
            let (pa, pb) = (tool.vertices[v1].p, tool.vertices[v2_].p);
            let fr = Frame::from_normal(sc, nrm, pa - sc);
            let ang_b = {
                let w = pb - sc;
                math::atan2(w.dot(fr.y), w.dot(fr.x))
            };
            // The short way round, facing the edge.
            let (t0, t1, from, to) = if ang_b >= 0.0 {
                (0.0, ang_b, v1, v2_)
            } else {
                (ang_b, 0.0, v2_, v1)
            };
            let seam = tool.add_edge(Curve::Circle { f: fr, r }, t0, t1, from, to);
            let seam_rev = from != v1;
            faces.push(Face {
                surface: pipe,
                loops: vec![vec![
                    Coedge { edge: e1, rev: false },
                    Coedge { edge: seam, rev: seam_rev },
                    Coedge { edge: e2, rev: true },
                    Coedge { edge: seam, rev: !seam_rev },
                ]],
            });
        }
        Dress::Chamfer(..) => {
            let ruled = Surface::Ruled {
                a: Box::new(c1.clone()),
                b: Box::new(c2.clone()),
                t0: 0.0,
                t1: len,
            };
            let (pa, pb) = (tool.vertices[v1].p, tool.vertices[v2_].p);
            let l = pa.dist(pb);
            let seam = tool.add_edge(Curve::Line { o: pa, d: (pb - pa) / l }, 0.0, l, v1, v2_);
            faces.push(Face {
                surface: ruled,
                loops: vec![vec![
                    Coedge { edge: e1, rev: false },
                    Coedge { edge: seam, rev: false },
                    Coedge { edge: e2, rev: true },
                    Coedge { edge: seam, rev: true },
                ]],
            });
        }
    }
    // The strips of both faces between the edge and the contact curves.
    let mut eloop = eloop;
    faces.extend(strip_faces(&mut tool, &mut eloop, &[(sa.clone(), e1), (sb.clone(), e2)]));
    tool.faces = faces;
    // Orient: the tool must have positive volume; flip whichever faces disagree.
    orient_tool(&mut tool)?;
    Ok(Tool {
        solid: tool,
        convex,
        prismatic: false,
        faces: (fa, fb),
        ends: [(V3::ZERO, V3::ZERO); 2],
    })
}

/// The strip faces between a closed chain `eloop` and closed contact curves on the same
/// surfaces: two loops where the strip does not wrap round its surface, else one loop
/// cut open along a line of constant u (splitting the chain there).
fn strip_faces(tool: &mut Solid, eloop: &mut Vec<Coedge>, strips: &[(Surface, usize)]) -> Vec<Face> {
    let valid = |t: &Solid, srf: &Surface, loops: Vec<Vec<Coedge>>| -> bool {
        let tmp = Solid {
            vertices: t.vertices.clone(),
            edges: t.edges.clone(),
            faces: vec![Face {
                surface: srf.clone(),
                loops,
            }],
        };
        super::uv::face_uv(&tmp, 0).valid
    };
    // Phase 1: where each wrapping strip needs its seam, split the chain.
    let mut seam_at: Vec<Option<usize>> = Vec::new();
    for (srf, ce) in strips {
        let two = vec![eloop.clone(), vec![Coedge { edge: *ce, rev: true }]];
        if valid(tool, srf, two) {
            seam_at.push(None);
            continue;
        }
        let pc = tool.vertices[tool.edges[*ce].v0].p;
        let (u0, _) = srf.project(pc);
        let mut found = None;
        'search: for (k, c) in eloop.iter().enumerate() {
            let e = tool.edges[c.edge].clone();
            let f = |t: f64| {
                let (u, _) = srf.project(e.curve.eval(t));
                math::wrap_angle(u - u0)
            };
            let n = 128;
            for i in 0..n {
                let ta = e.t0 + (e.t1 - e.t0) * i as f64 / n as f64;
                let tb = e.t0 + (e.t1 - e.t0) * (i + 1) as f64 / n as f64;
                let (fa, fb) = (f(ta), f(tb));
                if (fa < 0.0) != (fb < 0.0) && (fa - fb).abs() < PI {
                    found = Some((k, super::num::brent(f, ta, tb, fa, fb, 1e-14)));
                    break 'search;
                }
            }
        }
        let Some((k, t)) = found else {
            seam_at.push(None);
            continue;
        };
        let c = eloop[k];
        let e = tool.edges[c.edge].clone();
        let vs = tool.add_vertex(e.curve.eval(t));
        let ne = tool.add_edge(e.curve.clone(), t, e.t1, vs, e.v1);
        tool.edges[c.edge].t1 = t;
        tool.edges[c.edge].v1 = vs;
        let pair = if c.rev {
            [Coedge { edge: ne, rev: true }, Coedge { edge: c.edge, rev: true }]
        } else {
            [c, Coedge { edge: ne, rev: false }]
        };
        eloop.splice(k..=k, pair);
        seam_at.push(Some(vs));
    }
    // Phase 2: the faces.
    let mut faces = Vec::new();
    for ((srf, ce), at) in strips.iter().zip(seam_at) {
        let Some(vs) = at else {
            faces.push(Face {
                surface: srf.clone(),
                loops: vec![eloop.clone(), vec![Coedge { edge: *ce, rev: true }]],
            });
            continue;
        };
        let k = eloop
            .iter()
            .position(|c| tool.co_ends(*c).0 == vs)
            .unwrap_or(0);
        let mut chain = eloop.clone();
        chain.rotate_left(k);
        let pa = tool.vertices[vs].p;
        let vb = tool.edges[*ce].v0;
        let pb = tool.vertices[vb].p;
        let (u0, va) = srf.project(pa);
        let (_, vb_) = srf.project(pb);
        let forward = vb_ >= va;
        // The line of constant u between the two points: a generator of a cylinder or
        // cone, a meridian circle of a sphere or torus.
        let (curve, s0, s1) = match srf {
            Surface::Sphere { f, .. } | Surface::Torus { f, .. } => {
                let (su, cu) = math::sin_cos(u0);
                let er = f.x * cu + f.y * su;
                let (center, r) = match srf {
                    Surface::Torus { major, minor, .. } => (f.origin + er * *major, *minor),
                    Surface::Sphere { r, .. } => (f.origin, *r),
                    _ => unreachable!(),
                };
                let fr = Frame {
                    origin: center,
                    x: er,
                    y: f.z,
                    z: er.cross(f.z),
                };
                let c = Curve::Circle { f: fr, r };
                let ta = c.project(pa);
                let mut tb = c.project(pb);
                if forward {
                    while tb < ta {
                        tb += TAU;
                    }
                    (c, ta, tb)
                } else {
                    while tb > ta {
                        tb -= TAU;
                    }
                    (c, tb, ta)
                }
            }
            _ => (
                Curve::Line {
                    o: if forward { pa } else { pb },
                    d: if forward { (pb - pa).norm() } else { (pa - pb).norm() },
                },
                0.0,
                pa.dist(pb),
            ),
        };
        let (sv0, sv1) = if forward { (vs, vb) } else { (vb, vs) };
        let seam = tool.add_edge(curve, s0, s1, sv0, sv1);
        let rev = !forward;
        chain.extend([
            Coedge { edge: seam, rev },
            Coedge { edge: *ce, rev: true },
            Coedge { edge: seam, rev: !rev },
        ]);
        faces.push(Face {
            surface: srf.clone(),
            loops: vec![chain],
        });
    }
    faces
}

/// Make every face of a freshly built tool agree with its neighbours and point out.
fn orient_tool(t: &mut Solid) -> Result<(), String> {
    // Flip faces until each edge is used once each way (a small search: tools have
    // three or four faces).
    let n = t.faces.len();
    for mask in 0..(1u32 << n) {
        let mut trial = t.clone();
        for (i, f) in trial.faces.iter_mut().enumerate() {
            if mask & (1 << i) != 0 {
                for l in &mut f.loops {
                    l.reverse();
                    for c in l.iter_mut() {
                        c.rev = !c.rev;
                    }
                }
            }
        }
        if trial.check().is_ok() && super::mass::mass_props(&trial).volume > 0.0 {
            *t = trial;
            return Ok(());
        }
    }
    let why = t.check().err().unwrap_or_else(|| {
        format!("volume {}", super::mass::mass_props(t).volume)
    });
    Err(format!("the blend's tool could not be closed ({why})"))
}

/// Round (or bevel) `edges` of `s`.
pub fn dress(s: &Solid, edges: &[usize], dress: Dress, what: &str) -> Result<Solid, String> {
    let size_ok = match dress {
        Dress::Fillet(r) => r.is_finite() && r > 0.0,
        Dress::Chamfer(a, b) => a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0,
    };
    if !size_ok {
        return Err(match dress {
            Dress::Fillet(_) => "Fillet radius must be greater than zero".into(),
            Dress::Chamfer(..) => "Size must be greater than zero".into(),
        });
    }
    if edges.is_empty() {
        return Err("No edges specified".into());
    }
    let prep = Prepared::new(s);
    let mut tools: Vec<(usize, Tool)> = Vec::new();
    for &e in edges {
        if e >= s.edges.len() || s.edges[e].degenerate {
            return Err(format!("{what}: no such edge"));
        }
        let tool = match prismatic_tool(s, &prep, e, dress, what)? {
            Some(t) => t,
            None => match revolved_tool(s, &prep, e, dress, what)? {
                Some(t) => t,
                None => general_tool(s, &prep, e, dress, what)?,
            },
        };
        tools.push((e, tool));
    }
    // Corners where three rounded convex prismatic edges meet on three planes.
    let mut corners: Vec<Solid> = Vec::new();
    for v in 0..s.vertices.len() {
        let at: Vec<&(usize, Tool)> = tools
            .iter()
            .filter(|(e, _)| {
                let ed = &s.edges[*e];
                ed.v0 == v || ed.v1 == v
            })
            .collect();
        if at.len() != 3 || !at.iter().all(|(_, t)| t.convex && t.prismatic) {
            continue;
        }
        let mut faces: Vec<usize> = at.iter().flat_map(|(_, t)| [t.faces.0, t.faces.1]).collect();
        faces.sort_unstable();
        faces.dedup();
        if faces.len() != 3 || !faces.iter().all(|f| matches!(s.faces[*f].surface, Surface::Plane { .. })) {
            continue;
        }
        let equal = match dress {
            Dress::Fillet(_) => true,
            Dress::Chamfer(a, b) => (a - b).abs() < 1e-12,
        };
        if !equal {
            continue;
        }
        corners.push(corner_tool(s, &prep, v, &faces, &at, dress)?);
    }
    // Cuts first, one at a time, then the corners, then what concave rounds add.
    let mut out = s.clone();
    for (e, t) in tools.iter().filter(|t| t.1.convex) {
        out = boolean(&out, &t.solid, Op::Difference)
            .map_err(|x| format!("{what} on edge {}: {x}", e + 1))?;
    }
    for c in &corners {
        out = boolean(&out, c, Op::Difference).map_err(|x| format!("{what} at a corner: {x}"))?;
    }
    for (e, t) in tools.iter().filter(|t| !t.1.convex) {
        out = boolean(&out, &t.solid, Op::Union)
            .map_err(|x| format!("{what} on edge {}: {x}", e + 1))?;
    }
    Ok(out)
}

/// The corner of three planes at `v` beyond the ball (or the chamfer's corner flat).
fn corner_tool(
    s: &Solid,
    prep: &Prepared,
    v: usize,
    faces: &[usize],
    at: &[&(usize, Tool)],
    dress: Dress,
) -> Result<Solid, String> {
    let p = s.vertices[v].p;
    let normals: Vec<V3> = faces
        .iter()
        .map(|&f| {
            let (u, w) = s.faces[f].surface.project(p);
            s.faces[f].surface.normal(u, w) * prep.uvs[f].sense
        })
        .collect();
    let region = Box3 { min: p, max: p }.grow(match dress {
        Dress::Fillet(r) => r * 6.0,
        Dress::Chamfer(a, b) => (a + b) * 6.0,
    });
    // Start from a box round the corner, keep the material side of each face.
    let h = region.max - region.min;
    let mut body = super::build::cuboid(region.min, region.min + h);
    for &f in faces {
        let hs = inner_half_space(s, prep, f, p, &region).ok_or("corner face is not planar")?;
        body = boolean(&body, &hs, Op::Intersection)?;
    }
    let half = |q: V3, n: V3, region: &Box3| -> Solid {
        // Points with (x − q)·n ≤ 0.
        let size = region.diagonal() * 4.0 + 10.0;
        let c = region.min.lerp(region.max, 0.5);
        let c = c - n * (c - q).dot(n);
        let fr = Frame::from_normal(c, -n, n.any_perp());
        let r = super::build::polygon_region(&[
            v2(-size, -size),
            v2(size, -size),
            v2(size, size),
            v2(-size, size),
        ]);
        extrude(&[r], &fr, 0.0, size)
    };
    match dress {
        Dress::Fillet(r) => {
            // The ball's centre: r inside each plane.
            let rows = [normals[0], normals[1], normals[2]];
            let rhs = [
                normals[0].dot(p) - r,
                normals[1].dot(p) - r,
                normals[2].dot(p) - r,
            ];
            let c = super::num::solve3v(rows, rhs).ok_or("the corner's faces do not meet at a point")?;
            // The vertex side of the planes through the centre across each edge.
            for (e, _) in at {
                let ed = &s.edges[*e];
                let other = if ed.v0 == v { ed.end() } else { ed.start() };
                let dir = (other - p).norm();
                body = boolean(&body, &half(c, dir, &region), Op::Intersection)?;
            }
            let ball = super::build::sphere(c, r);
            boolean(&body, &ball, Op::Difference)
        }
        Dress::Chamfer(d, _) => {
            // The plane through the three points where the chamfers' flats meet on the
            // faces: on each face, its two edges' contact lines cross.
            let mut pts = Vec::new();
            for (fi, &f) in faces.iter().enumerate() {
                let _ = fi;
                // The two edges at v bounding f.
                let es: Vec<&&(usize, Tool)> = at
                    .iter()
                    .filter(|(_, t)| t.faces.0 == f || t.faces.1 == f)
                    .collect();
                if es.len() != 2 {
                    return Err("corner edges do not pair up".into());
                }
                // Each edge's direction away from v; the contact line on f is offset d
                // along the other edge's direction.
                let dir = |e: usize| {
                    let ed = &s.edges[e];
                    let other = if ed.v0 == v { ed.end() } else { ed.start() };
                    (other - p).norm()
                };
                let (d1, d2) = (dir(es[0].0), dir(es[1].0));
                // Offsets in the face: perpendicular to each edge, into the face.
                let nf = normals[faces.iter().position(|x| *x == f).unwrap()];
                let o1 = nf.cross(d1).norm();
                let o1 = if o1.dot(d2) < 0.0 { -o1 } else { o1 };
                let o2 = nf.cross(d2).norm();
                let o2 = if o2.dot(d1) < 0.0 { -o2 } else { o2 };
                // p + o1 d + s d1 = p + o2 d + t d2.
                let a = o1 * d - o2 * d;
                let m = d1.cross(d2);
                let s1 = (-a).cross(d2).dot(m) / m.len2();
                pts.push(p + o1 * d + d1 * s1);
            }
            let n = (pts[1] - pts[0]).cross(pts[2] - pts[0]).norm();
            let n = if n.dot(p - pts[0]) < 0.0 { -n } else { n };
            // Keep the vertex side of the corner plane (`n` points towards the vertex).
            boolean(&body, &half(pts[0], -n, &region), Op::Intersection)
        }
    }
}
