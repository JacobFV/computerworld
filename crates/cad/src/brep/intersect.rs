//! Intersections: a curve with a surface, two surfaces with each other (closed forms for
//! the analytic pairs a Part Design model meets — planes with anything round, coaxial
//! surfaces of revolution, parallel cylinders — and marching for the rest), and a ray
//! with a solid for inside/outside classification.
use super::geom::{Curve, Surface, TraceKind, Traced};
use super::num::{self, solve3v};
use super::topo::{Box3, Solid, TOL};
use super::uv::{classify, face_box, face_uv, FaceUV, Where};
use crate::math::{self, v2, Frame, TAU, V2, V3};

/// How a curve meets a surface.
#[derive(Clone, Debug, PartialEq)]
pub enum CurveHit {
    /// Parameters of the crossing (or touching) points.
    Points(Vec<f64>),
    /// The curve lies in the surface over the whole range.
    OnSurface,
}

fn sample_count(c: &Curve, t0: f64, t1: f64) -> usize {
    match c {
        Curve::Line { .. } => 48,
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            (((t1 - t0).abs() / TAU * 96.0).ceil() as usize).max(24)
        }
        Curve::BSpline(b) => b.poles.len() * 8 + 32,
        Curve::Traced(t) => (t.pts.len() * 3).clamp(32, 1500),
    }
}

/// Where a curve (over `[t0, t1]`) meets a surface.
pub fn curve_surface(c: &Curve, t0: f64, t1: f64, s: &Surface, tol: f64) -> CurveHit {
    // Closed forms: a line with a plane, a circle with a plane.
    match (c, s) {
        (Curve::Line { o, d }, Surface::Plane { f }) => {
            let den = d.dot(f.z);
            let dist = (*o - f.origin).dot(f.z);
            if den.abs() < 1e-12 {
                let at0 = (c.eval(t0) - f.origin).dot(f.z);
                return if at0.abs() <= tol && dist.abs() <= tol {
                    CurveHit::OnSurface
                } else {
                    CurveHit::Points(vec![])
                };
            }
            let t = -dist / den;
            let lo = t0.min(t1);
            let hi = t0.max(t1);
            let slack = tol / d.len().max(1e-300) / den.abs().max(1e-12) * den.abs();
            return CurveHit::Points(if t >= lo - slack && t <= hi + slack {
                vec![t.clamp(lo, hi)]
            } else {
                vec![]
            });
        }
        (Curve::Circle { f: cf, r }, Surface::Plane { f }) => {
            // r (cos t x + sin t y)·n = −(o − p)·n.
            let a = cf.x.dot(f.z) * r;
            let b = cf.y.dot(f.z) * r;
            let k = -(cf.origin - f.origin).dot(f.z);
            let amp = math::hypot(a, b);
            if amp < tol {
                return if k.abs() <= tol {
                    CurveHit::OnSurface
                } else {
                    CurveHit::Points(vec![])
                };
            }
            if k.abs() > amp + tol {
                return CurveHit::Points(vec![]);
            }
            let phi = math::atan2(b, a);
            let ratio = (k / amp).clamp(-1.0, 1.0);
            let delta = math::acos(ratio);
            let mut out = Vec::new();
            let cands = if delta.abs() < 1e-12 {
                vec![phi]
            } else {
                vec![phi - delta, phi + delta]
            };
            for t in cands {
                // Into the range, by whole turns.
                let mut x = t;
                let lo = t0.min(t1);
                let hi = t0.max(t1);
                while x < lo - 1e-12 {
                    x += TAU;
                }
                while x > hi + 1e-12 {
                    x -= TAU;
                }
                if x >= lo - 1e-12 && x <= hi + 1e-12 {
                    out.push(x.clamp(lo, hi));
                    // A full circle meets the point twice (start and end): once is enough.
                }
            }
            out.sort_by(|a, b| a.total_cmp(b));
            out.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
            return CurveHit::Points(out);
        }
        _ => {}
    }
    // Lines with quadrics: a quadratic, so touching points are exact double roots.
    if let Curve::Line { o, d } = c {
        let quad = match s {
            Surface::Cylinder { f, r } => {
                let w = *o - f.origin;
                let (wp, dp) = (w - f.z * w.dot(f.z), *d - f.z * d.dot(f.z));
                Some((dp.dot(dp), 2.0 * wp.dot(dp), wp.dot(wp) - r * r, None))
            }
            Surface::Sphere { f, r } => {
                let w = *o - f.origin;
                Some((d.dot(*d), 2.0 * w.dot(*d), w.dot(w) - r * r, None))
            }
            Surface::Cone { f, r, a } => {
                let tn = math::tan(*a);
                let w = *o - f.origin;
                let (h0, dz) = (w.dot(f.z), d.dot(f.z));
                let (wp, dp) = (w - f.z * h0, *d - f.z * dz);
                let k0 = r + tn * h0;
                Some((
                    dp.dot(dp) - tn * tn * dz * dz,
                    2.0 * (wp.dot(dp) - tn * k0 * dz),
                    wp.dot(wp) - k0 * k0,
                    Some((*f, *r, tn)),
                ))
            }
            _ => None,
        };
        if let Some((qa, qb, qc, cone)) = quad {
            let scale = qa.abs() + qb.abs() + qc.abs();
            if qa.abs() <= 1e-14 * scale.max(1e-300)
                && qb.abs() <= 1e-12 * scale
                && qc.abs() <= tol * tol * 4.0 + 1e-14 * scale
            {
                return CurveHit::OnSurface;
            }
            let lo = t0.min(t1);
            let hi = t0.max(t1);
            let slack = tol / d.len().max(1e-300);
            let roots = num::quadratic(qa, qb, qc)
                .into_iter()
                .filter(|t| *t >= lo - slack && *t <= hi + slack)
                .filter(|t| match cone {
                    // The nappe the surface is on: radius r + h tan ≥ 0.
                    Some((f, r, tn)) => r + tn * (c.eval(*t) - f.origin).dot(f.z) >= -tol,
                    None => true,
                })
                .map(|t| t.clamp(lo, hi))
                .collect();
            return CurveHit::Points(roots);
        }
    }
    let n = sample_count(c, t0, t1);
    let f = |t: f64| s.sd(c.eval(t));
    // Lying on it: every sample within tolerance.
    let all_on = (0..=n).all(|i| f(t0 + (t1 - t0) * i as f64 / n as f64).abs() <= tol);
    if all_on {
        return CurveHit::OnSurface;
    }
    let df = |t: f64| {
        let (p, dp) = c.d1(t);
        s.grad(p).dot(dp)
    };
    let span = (t1 - t0).abs().max(1e-300);
    CurveHit::Points(num::roots_d(f, df, t0, t1, n, tol * 1e-2, span * 1e-10))
}

/// A section curve of two surfaces: the curve and the parameter range that covers the
/// region of interest (the whole period for closed curves).
#[derive(Clone, Debug)]
pub struct Section {
    pub curve: Curve,
    pub t0: f64,
    pub t1: f64,
    pub closed: bool,
}

fn line_in_box(o: V3, d: V3, b: &Box3) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for ax in 0..3 {
        let (oo, dd, mn, mx) = (o.get(ax), d.get(ax), b.min.get(ax), b.max.get(ax));
        if dd.abs() < 1e-15 {
            if oo < mn || oo > mx {
                return None;
            }
            continue;
        }
        let (a, c) = ((mn - oo) / dd, (mx - oo) / dd);
        lo = lo.max(a.min(c));
        hi = hi.min(a.max(c));
    }
    (lo <= hi).then_some((lo, hi))
}

fn line_section(o: V3, d: V3, b: &Box3) -> Option<Section> {
    let d = d.norm();
    let (lo, hi) = line_in_box(o, d, b)?;
    Some(Section {
        curve: Curve::Line { o, d },
        t0: lo,
        t1: hi,
        closed: false,
    })
}

fn circle_section(center: V3, axis: V3, x_hint: V3, r: f64) -> Section {
    let f = Frame::from_normal(center, axis, x_hint);
    Section {
        curve: Curve::Circle { f, r },
        t0: 0.0,
        t1: TAU,
        closed: true,
    }
}

/// The meridian of a surface of revolution in its (ρ, h) half-plane.
#[derive(Clone, Copy, Debug)]
enum Meridian {
    /// Points `p + t d`.
    Line {
        p: V2,
        d: V2,
    },
    Circle {
        c: V2,
        r: f64,
    },
}

fn revolution_meridian(s: &Surface) -> Option<(V3, V3, Meridian)> {
    match s {
        Surface::Cylinder { f, r } => Some((
            f.origin,
            f.z,
            Meridian::Line {
                p: v2(*r, 0.0),
                d: v2(0.0, 1.0),
            },
        )),
        Surface::Cone { f, r, a } => {
            let t = math::tan(*a);
            Some((
                f.origin,
                f.z,
                Meridian::Line {
                    p: v2(*r, 0.0),
                    d: v2(t, 1.0).norm(),
                },
            ))
        }
        Surface::Sphere { f, r } => Some((f.origin, f.z, Meridian::Circle { c: V2::ZERO, r: *r })),
        Surface::Torus { f, major, minor } => Some((
            f.origin,
            f.z,
            Meridian::Circle {
                c: v2(*major, 0.0),
                r: *minor,
            },
        )),
        Surface::Plane { f } => Some((
            f.origin,
            f.z,
            Meridian::Line {
                p: v2(0.0, 0.0),
                d: v2(1.0, 0.0),
            },
        )),
        _ => None,
    }
}

/// 2D intersections of two meridians.
fn meridian_hits(a: Meridian, b: Meridian) -> Vec<V2> {
    match (a, b) {
        (Meridian::Line { p, d }, Meridian::Line { p: q, d: e }) => {
            let den = d.cross(e);
            if den.abs() < 1e-12 {
                return vec![];
            }
            let t = (q - p).cross(e) / den;
            vec![p + d * t]
        }
        (Meridian::Line { p, d }, Meridian::Circle { c, r })
        | (Meridian::Circle { c, r }, Meridian::Line { p, d }) => {
            let d = d.norm();
            let w = p - c;
            let bq = w.dot(d);
            let cq = w.dot(w) - r * r;
            num::quadratic(1.0, 2.0 * bq, cq)
                .into_iter()
                .map(|t| p + d * t)
                .collect()
        }
        (Meridian::Circle { c: c1, r: r1 }, Meridian::Circle { c: c2, r: r2 }) => {
            let dv = c2 - c1;
            let dist = dv.len();
            if dist < 1e-12 || dist > r1 + r2 + 1e-12 || dist < (r1 - r2).abs() - 1e-12 {
                return vec![];
            }
            let a = (r1 * r1 - r2 * r2 + dist * dist) / (2.0 * dist);
            let h2 = r1 * r1 - a * a;
            let m = c1 + dv * (a / dist);
            if h2 <= 1e-20 * r1 * r1 {
                return vec![m];
            }
            let h = math::sqrt(h2);
            let perp = dv.perp() / dist;
            vec![m + perp * h, m - perp * h]
        }
    }
}

/// Whether two surfaces of revolution (or a plane across the axis) share an axis.
fn coaxial(sa: &Surface, sb: &Surface) -> Option<(V3, V3, Meridian, Meridian)> {
    let (oa, ka, ma) = revolution_meridian(sa)?;
    let (ob, kb, mb) = revolution_meridian(sb)?;
    let plane_a = matches!(sa, Surface::Plane { .. });
    let plane_b = matches!(sb, Surface::Plane { .. });
    if plane_a && plane_b {
        return None;
    }
    if ka.cross(kb).len() > 1e-9 {
        return None;
    }
    // Axis of the round one(s); a plane only needs to be perpendicular.
    let (o, k) = if plane_a { (ob, kb) } else { (oa, ka) };
    let on_axis = |p: V3| {
        let w = p - o;
        (w - k * w.dot(k)).len() < 1e-9 * (1.0 + w.len())
    };
    if !plane_a && !on_axis(oa) {
        return None;
    }
    if !plane_b && !on_axis(ob) {
        return None;
    }
    // Express both meridians in the common (ρ, h) frame from `o` along `k`.
    let rebase = |m: Meridian, origin: V3, axis: V3, is_plane: bool| -> Meridian {
        let shift = (origin - o).dot(k);
        let flip = if axis.dot(k) < 0.0 { -1.0 } else { 1.0 };
        match m {
            Meridian::Line { d, .. } if is_plane => Meridian::Line {
                p: v2(0.0, shift),
                d,
            },
            Meridian::Line { p, d } => Meridian::Line {
                p: v2(p.x, p.y * flip + shift),
                d: v2(d.x, d.y * flip),
            },
            Meridian::Circle { c, r } => Meridian::Circle {
                c: v2(c.x, c.y * flip + shift),
                r,
            },
        }
    };
    Some((
        o,
        k,
        rebase(ma, oa, ka, plane_a),
        rebase(mb, ob, kb, plane_b),
    ))
}

/// Sphere pairs are coaxial about the line through their centres.
fn sphere_axis(s: &Surface, other: &Surface) -> Option<Surface> {
    if let (Surface::Sphere { f, r }, Surface::Sphere { f: g, .. }) = (s, other) {
        let d = g.origin - f.origin;
        if d.len() < 1e-12 {
            return None;
        }
        return Some(Surface::Sphere {
            f: Frame::from_normal(f.origin, d, d.any_perp()),
            r: *r,
        });
    }
    None
}

/// Analytic intersection of two surfaces, `None` when no closed form applies.
pub fn analytic_sections(sa: &Surface, sb: &Surface, b: &Box3) -> Option<Vec<Section>> {
    // Two spheres: a circle about the line of centres.
    if let (Surface::Sphere { f, r: r1 }, Surface::Sphere { f: g, r: r2 }) = (sa, sb) {
        let dv = g.origin - f.origin;
        let d = dv.len();
        if d < 1e-12 || d > r1 + r2 || d < (r1 - r2).abs() {
            return Some(vec![]);
        }
        let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
        let h2 = r1 * r1 - a * a;
        if h2 <= TOL * TOL {
            return Some(vec![]);
        }
        let k = dv / d;
        return Some(vec![circle_section(
            f.origin + k * a,
            k,
            k.any_perp(),
            math::sqrt(h2),
        )]);
    }
    // A sphere whose centre is on the axis of a surface of revolution: coaxial.
    let realign = |s: &Surface, other: &Surface| -> Option<Surface> {
        if let (Surface::Sphere { f, r }, Some((o, k, _))) = (s, revolution_meridian(other)) {
            if matches!(other, Surface::Plane { .. } | Surface::Sphere { .. }) {
                return None;
            }
            let w = f.origin - o;
            if (w - k * w.dot(k)).len() < 1e-9 * (1.0 + w.len()) {
                return Some(Surface::Sphere {
                    f: Frame::from_normal(f.origin, k, k.any_perp()),
                    r: *r,
                });
            }
        }
        None
    };
    let aligned_a = realign(sa, sb);
    let aligned_b = realign(sb, sa);
    let (sa, sb) = (
        aligned_a.as_ref().unwrap_or(sa),
        aligned_b.as_ref().unwrap_or(sb),
    );
    let _ = sphere_axis;
    // Plane and sphere: always a circle.
    match (sa, sb) {
        (Surface::Plane { f }, Surface::Sphere { f: g, r })
        | (Surface::Sphere { f: g, r }, Surface::Plane { f }) => {
            let d = (g.origin - f.origin).dot(f.z);
            if d.abs() > r + TOL {
                return Some(vec![]);
            }
            let rr = r * r - d * d;
            if rr <= TOL * TOL {
                return Some(vec![]);
            }
            let c = g.origin - f.z * d;
            return Some(vec![circle_section(c, f.z, f.x, math::sqrt(rr))]);
        }
        _ => {}
    }
    if let Some((o, k, ma, mb)) = coaxial(sa, sb) {
        let x = k.any_perp();
        let mut out = Vec::new();
        for p in meridian_hits(ma, mb) {
            if p.x > TOL {
                out.push(circle_section(o + k * p.y, k, x, p.x));
            }
        }
        return Some(out);
    }
    match (sa, sb) {
        (Surface::Plane { f }, Surface::Plane { f: g }) => {
            let d = f.z.cross(g.z);
            if d.len() < 1e-12 {
                return Some(vec![]);
            }
            // A point on both planes: solve n1·p = c1, n2·p = c2, d·p = 0.
            let p = solve3v([f.z, g.z, d], [f.z.dot(f.origin), g.z.dot(g.origin), 0.0])?;
            Some(line_section(p, d, b).into_iter().collect())
        }
        (Surface::Plane { f }, Surface::Cylinder { f: g, r })
        | (Surface::Cylinder { f: g, r }, Surface::Plane { f }) => {
            let cosang = f.z.dot(g.z);
            if cosang.abs() < 1e-12 {
                // Axis parallel to the plane: 0, 1 or 2 lines.
                let d = (g.origin - f.origin).dot(f.z);
                if d.abs() > r + TOL {
                    return Some(vec![]);
                }
                let foot = g.origin - f.z * d;
                let side = f.z.cross(g.z).norm();
                let h2 = r * r - d * d;
                if h2 <= TOL * TOL {
                    return Some(line_section(foot, g.z, b).into_iter().collect());
                }
                let h = math::sqrt(h2);
                return Some(
                    [foot + side * h, foot - side * h]
                        .into_iter()
                        .filter_map(|p| line_section(p, g.z, b))
                        .collect(),
                );
            }
            // Axis meets the plane: circle (perpendicular) or ellipse.
            let t = (f.origin - g.origin).dot(f.z) / cosang;
            let c = g.origin + g.z * t;
            if (cosang.abs() - 1.0).abs() < 1e-12 {
                return Some(vec![circle_section(c, f.z, g.x, *r)]);
            }
            // Major axis: the axis's direction projected into the plane.
            let maj = (g.z - f.z * cosang).norm();
            let a = r / cosang.abs();
            let fr = Frame::from_normal(c, f.z, maj);
            Some(vec![Section {
                curve: Curve::Ellipse { f: fr, a, b: *r },
                t0: 0.0,
                t1: TAU,
                closed: true,
            }])
        }
        (Surface::Plane { f }, Surface::Cone { f: g, r, a })
        | (Surface::Cone { f: g, r, a }, Surface::Plane { f }) => {
            // Through the apex and containing the axis: two generator lines.
            let apex = g.origin - g.z * (r / math::tan(*a));
            if (apex - f.origin).dot(f.z).abs() < TOL && f.z.dot(g.z).abs() < 1e-12 {
                let side = f.z.cross(g.z).norm();
                let t = math::tan(*a);
                return Some(
                    [side, -side]
                        .into_iter()
                        .filter_map(|s| line_section(apex, (g.z + s * t).norm(), b))
                        .collect(),
                );
            }
            None
        }
        (Surface::Cylinder { f, r }, Surface::Cylinder { f: g, r: r2 }) => {
            if f.z.cross(g.z).len() > 1e-9 {
                // Equal radii with crossing axes (a mitre): two ellipses, in the planes
                // bisecting the axes.
                let m = f.z.cross(g.z);
                let w = g.origin - f.origin;
                let gap = w.dot(m.norm());
                if (r - r2).abs() > TOL || gap.abs() > TOL {
                    return None;
                }
                // The axes' crossing point.
                let den = m.len2();
                let s = w.cross(g.z).dot(m) / den;
                let c = f.origin + f.z * s;
                let cyl = Surface::Cylinder { f: *f, r: *r };
                let mut out = Vec::new();
                for n in [f.z - g.z, f.z + g.z] {
                    let plane = Surface::Plane {
                        f: Frame::from_normal(c, n, n.any_perp()),
                    };
                    out.extend(analytic_sections(&plane, &cyl, b)?);
                }
                return Some(out);
            }
            // Parallel axes: lines where the cross-section circles meet.
            let w = g.origin - f.origin;
            let off = w - f.z * w.dot(f.z);
            let dist = off.len();
            if dist < 1e-12 {
                return Some(vec![]);
            }
            let hits = meridian_hits(
                Meridian::Circle { c: V2::ZERO, r: *r },
                Meridian::Circle {
                    c: v2(dist, 0.0),
                    r: *r2,
                },
            );
            let ex = off / dist;
            let ey = f.z.cross(ex);
            Some(
                hits.into_iter()
                    .filter_map(|p| line_section(f.origin + ex * p.x + ey * p.y, f.z, b))
                    .collect(),
            )
        }
        _ => None,
    }
}

/// Points where two section curves cross (or touch).
pub fn section_crossings(a: &Section, b: &Section, tol: f64) -> Vec<V3> {
    let n = 128;
    let mut out: Vec<V3> = Vec::new();
    let dist = |t: f64| -> (f64, V3) {
        let p = a.curve.eval(t);
        let s = b.curve.param_in(p, b.t0, b.t1);
        let s = if b.closed { s } else { s.clamp(b.t0, b.t1) };
        (p.dist(b.curve.eval(s)), p)
    };
    let ts: Vec<f64> = (0..=n)
        .map(|i| a.t0 + (a.t1 - a.t0) * i as f64 / n as f64)
        .collect();
    let ds: Vec<f64> = ts.iter().map(|t| dist(*t).0).collect();
    for i in 1..n {
        if ds[i] <= ds[i - 1] && ds[i] <= ds[i + 1] {
            let t = num::golden_min(|t| dist(t).0, ts[i - 1], ts[i + 1], 120);
            let (d, p) = dist(t);
            if d <= tol * 10.0 && out.iter().all(|q| q.dist(p) > tol * 10.0) {
                out.push(p);
            }
        }
    }
    out
}

/// Newton onto the intersection of two surfaces from `p` (minimum-norm steps).
pub fn settle(sa: &Surface, sb: &Surface, p: V3) -> Option<V3> {
    let mut p = p;
    let scale = 1.0 + p.len();
    for _ in 0..50 {
        let (fa, ga) = (sa.sd(p), sa.grad(p));
        let (fb, gb) = (sb.sd(p), sb.grad(p));
        // p -= Jᵀ (J Jᵀ)⁻¹ f.
        let (a, b, d) = (ga.dot(ga), ga.dot(gb), gb.dot(gb));
        let (x, y) = num::solve2(a, b, b, d, fa, fb)?;
        let step = ga * x + gb * y;
        p -= step;
        if step.len() <= 1e-15 * scale {
            break;
        }
    }
    (sa.sd(p).abs() < TOL * 1e-2 && sb.sd(p).abs() < TOL * 1e-2).then_some(p)
}

/// Trace the intersection of two surfaces from `seed` both ways, staying in `b`.
pub fn march(sa: &Surface, sb: &Surface, seed: V3, b: &Box3) -> Option<(Vec<V3>, bool)> {
    let p0 = settle(sa, sb, seed)?;
    let tangent = |p: V3| sa.grad(p).cross(sb.grad(p));
    if tangent(p0).len() < 1e-7 {
        return None;
    }
    let size = b.diagonal().max(1e-6);
    let walk = |dir: f64| -> (Vec<V3>, bool) {
        let mut pts = vec![p0];
        let mut p = p0;
        let mut h = size / 200.0;
        let mut closed = false;
        let mut travelled = 0.0;
        for _ in 0..20_000 {
            let t0 = tangent(p).norm() * dir;
            let q = p + t0 * h;
            // Correct in the plane through q normal to t0.
            let mut x = q;
            let mut ok = false;
            for _ in 0..30 {
                let (fa, ga) = (sa.sd(x), sa.grad(x));
                let (fb, gb) = (sb.sd(x), sb.grad(x));
                let fc = (x - q).dot(t0);
                let Some(step) = solve3v([ga, gb, t0], [-fa, -fb, -fc]) else {
                    break;
                };
                x += step;
                if step.len() <= 1e-14 * (1.0 + x.len()) {
                    ok = true;
                    break;
                }
            }
            let t1 = tangent(x);
            let turn = if t1.len() > 0.0 {
                t1.norm().dot(t0 * dir * dir).clamp(-1.0, 1.0)
            } else {
                -1.0
            };
            // At most 4° of turn per step, and no jump to another branch.
            if !ok || turn < math::cos(math::radians(4.0)) || x.dist(q) > h * 0.3 {
                h *= 0.5;
                if h < size * 1e-9 {
                    break;
                }
                continue;
            }
            travelled += x.dist(p);
            p = x;
            // Back at the start: a closed loop.
            if travelled > h * 3.0 && p.dist(p0) < h * 1.2 && pts.len() > 3 {
                closed = true;
                break;
            }
            pts.push(p);
            if !b.contains(p) {
                break;
            }
            if turn > math::cos(math::radians(1.0)) {
                h = (h * 1.5).min(size / 30.0);
            }
        }
        if closed {
            pts.push(p0);
        }
        (pts, closed)
    };
    let (fwd, closed) = walk(1.0);
    if closed {
        return Some((fwd, true));
    }
    let (mut back, _) = walk(-1.0);
    back.reverse();
    back.pop();
    back.extend(fwd);
    (back.len() >= 2).then_some((back, false))
}

/// A traced intersection curve through the given guide points.
pub fn traced_curve(sa: &Surface, sb: &Surface, pts: Vec<V3>, closed: bool) -> Curve {
    let mut ts = Vec::with_capacity(pts.len());
    let mut acc = 0.0;
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            acc += p.dist(pts[i - 1]);
        }
        ts.push(acc);
    }
    Curve::Traced(Box::new(Traced {
        kind: TraceKind::Inter {
            a: sa.clone(),
            oa: 0.0,
            b: sb.clone(),
            ob: 0.0,
        },
        pts,
        ts,
        closed,
    }))
}

/// Intersections of two surfaces near two faces: analytic when possible, else marched
/// from `seeds` (points known to lie on both) and from a search over both faces.
#[allow(clippy::too_many_arguments)]
pub fn surface_sections(
    solid_a: &Solid,
    fa: usize,
    ua: &FaceUV,
    solid_b: &Solid,
    fb: usize,
    ub: &FaceUV,
    seeds: &[V3],
    region: &Box3,
) -> Vec<Section> {
    let sa = &solid_a.faces[fa].surface;
    let sb = &solid_b.faces[fb].surface;
    if let Some(s) = analytic_sections(sa, sb, region) {
        return s;
    }
    let mut out: Vec<Section> = Vec::new();
    let mut guides: Vec<Vec<V3>> = Vec::new();
    let on_known = |p: V3, guides: &Vec<Vec<V3>>| {
        guides.iter().any(|g| {
            g.windows(2).any(|w| {
                let d = w[1] - w[0];
                let l2 = d.len2();
                let s = if l2 > 0.0 {
                    ((p - w[0]).dot(d) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (w[0] + d * s).dist(p) < (d.len() * 0.05).max(TOL * 100.0)
            })
        })
    };
    let mut candidates: Vec<V3> = seeds.to_vec();
    // Interior search: points of each face pulled onto the intersection.
    for (solid, f, fu) in [(solid_a, fa, ua), (solid_b, fb, ub)] {
        let s = &solid.faces[f].surface;
        let n = 10;
        for i in 0..=n {
            for j in 0..=n {
                let q = v2(
                    fu.lo.x + (fu.hi.x - fu.lo.x) * i as f64 / n as f64,
                    fu.lo.y + (fu.hi.y - fu.lo.y) * j as f64 / n as f64,
                );
                let p = s.eval(q.x, q.y);
                if let Some(x) = settle(sa, sb, p) {
                    if region.contains(x) && x.dist(p) < region.diagonal() * 0.2 {
                        candidates.push(x);
                    }
                }
            }
        }
    }
    for c in candidates {
        if on_known(c, &guides) {
            continue;
        }
        if let Some((pts, closed)) = march(sa, sb, c, region) {
            guides.push(pts.clone());
            let curve = traced_curve(sa, sb, pts, closed);
            let (t0, t1) = match &curve {
                Curve::Traced(t) => t.range(),
                _ => unreachable!(),
            };
            out.push(Section {
                curve,
                t0,
                t1,
                closed,
            });
        }
    }
    out
}

/// Real roots of a ray `o + t d` (t > 0) with a surface, within `b`.
fn ray_surface(s: &Surface, o: V3, d: V3, b: &Box3) -> Vec<f64> {
    let Some((lo, hi)) = line_in_box(o, d, b) else {
        return vec![];
    };
    let lo = lo.max(0.0);
    if hi <= lo {
        return vec![];
    }
    match s {
        Surface::Plane { f } => {
            let den = d.dot(f.z);
            if den.abs() < 1e-15 {
                return vec![];
            }
            let t = (f.origin - o).dot(f.z) / den;
            if t > lo && t <= hi {
                vec![t]
            } else {
                vec![]
            }
        }
        Surface::Sphere { f, r } => {
            let w = o - f.origin;
            num::quadratic(d.dot(d), 2.0 * w.dot(d), w.dot(w) - r * r)
                .into_iter()
                .filter(|t| *t > lo && *t <= hi)
                .collect()
        }
        Surface::Cylinder { f, r } => {
            let w = o - f.origin;
            let (wp, dp) = (w - f.z * w.dot(f.z), d - f.z * d.dot(f.z));
            num::quadratic(dp.dot(dp), 2.0 * wp.dot(dp), wp.dot(wp) - r * r)
                .into_iter()
                .filter(|t| *t > lo && *t <= hi)
                .collect()
        }
        _ => {
            let span = hi - lo;
            num::roots(|t| s.sd(o + d * t), lo, hi, 400, TOL * 1e-3, span * 1e-12)
        }
    }
}

/// Prepared faces of a solid for repeated classification.
pub struct Prepared<'a> {
    pub solid: &'a Solid,
    pub uvs: Vec<FaceUV>,
    pub boxes: Vec<Box3>,
}
impl<'a> Prepared<'a> {
    pub fn new(solid: &'a Solid) -> Prepared<'a> {
        let uvs: Vec<FaceUV> = (0..solid.faces.len()).map(|f| face_uv(solid, f)).collect();
        let boxes = (0..solid.faces.len())
            .map(|f| face_box(solid, &uvs[f], f))
            .collect();
        Prepared { solid, uvs, boxes }
    }
    /// Whether `p` is inside the solid (by ray parity; `None` if every ray was
    /// ambiguous, which only happens for points on the boundary).
    pub fn contains(&self, p: V3) -> Option<bool> {
        let dirs = [
            V3 {
                x: 0.577_215_664_9,
                y: 0.316_227_766_0,
                z: 0.752_441_1,
            },
            V3 {
                x: -0.213_462_4,
                y: 0.871_311_9,
                z: 0.441_876_2,
            },
            V3 {
                x: 0.707_106_2,
                y: -0.505_971_3,
                z: 0.494_139_7,
            },
            V3 {
                x: -0.391_625_1,
                y: -0.617_993_2,
                z: -0.681_311_5,
            },
            V3 {
                x: 0.911_274_3,
                y: 0.131_428_8,
                z: -0.390_249_1,
            },
            V3 {
                x: -0.639_215_7,
                y: 0.264_197_3,
                z: -0.722_411_9,
            },
            V3 {
                x: 0.183_926_4,
                y: -0.947_113_2,
                z: 0.262_947_5,
            },
            V3 {
                x: -0.512_398_1,
                y: -0.337_129_4,
                z: 0.789_914_6,
            },
        ];
        let total = self
            .boxes
            .iter()
            .fold(Box3::empty(), |a, b| a.union(b))
            .grow(1.0);
        'dir: for d in dirs {
            let d = d.norm();
            let mut count = 0;
            for (fi, fb) in self.boxes.iter().enumerate() {
                if line_in_box(p, d, fb).is_none_or(|(_, hi)| hi < 0.0) {
                    continue;
                }
                let s = &self.solid.faces[fi].surface;
                for t in ray_surface(s, p, d, &fb.union(&Box3 { min: p, max: p }).grow(TOL)) {
                    if t < TOL * 10.0 {
                        continue 'dir;
                    }
                    let q = p + d * t;
                    if !total.contains(q) {
                        continue;
                    }
                    match classify(self.solid, &self.uvs[fi], fi, q, TOL * 10.0) {
                        Where::Inside => {
                            // Grazing hits are ambiguous.
                            let (u, v) = s.project(q);
                            if s.normal(u, v).dot(d).abs() < 1e-6 {
                                continue 'dir;
                            }
                            count += 1;
                        }
                        Where::Boundary => continue 'dir,
                        Where::Outside => {}
                    }
                }
            }
            return Some(count % 2 == 1);
        }
        None
    }
}
