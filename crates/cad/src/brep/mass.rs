//! Mass properties from the exact boundary: volume, area and centres by the divergence
//! theorem, integrated over each face's region of its parameter plane.
//!
//! For a face `S(u, v)` over the region `D`, a flux `∫∫_D g(u, v) du dv` becomes, by
//! Green's theorem, the boundary integral `−∮ H du` with `H(u, v) = ∫ g(u, s) ds` from a
//! fixed `v₀`. The inner integral is composite Gauss–Legendre (exact for the low-degree
//! polynomial and trigonometric integrands analytic surfaces give), the outer one
//! adaptive Gauss–Kronrod along each coedge's exact curve mapped to (u, v). Nothing is
//! tessellated: results match closed-form values to rounding.
use super::geom::Surface;
use super::num;
use super::topo::Solid;
use super::uv::{face_uv, CoUV, FaceUV};
use crate::math::{v2, V2, V3, FRAC_PI_2, PI};

/// Integrals of one face: [volume flux, x, y, z first-moment fluxes, area, area-weighted
/// x, y, z].
pub type FaceIntegrals = [f64; 8];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProps {
    pub volume: f64,
    pub area: f64,
    /// Centre of mass at uniform density.
    pub center: V3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceProps {
    pub area: f64,
    pub centroid: V3,
}

fn inner_pieces(s: &Surface, span: f64) -> usize {
    let span = span.abs();
    match s {
        Surface::Plane { .. } | Surface::Cylinder { .. } | Surface::Cone { .. } => 1,
        Surface::Sphere { .. } | Surface::Torus { .. } | Surface::Pipe { .. } => {
            ((span / (PI / 4.0)).ceil() as usize).max(1)
        }
        _ => {
            let (_, _, v0, v1) = s.domain();
            ((span / ((v1 - v0).abs() / 4.0).max(1e-9)).ceil() as usize).clamp(1, 8)
        }
    }
}

fn integrand(s: &Surface, u: f64, v: f64, inv: [f64; 3]) -> [f64; 8] {
    let (p, su, sv) = s.d1(u, v);
    let n = su.cross(sv);
    let a = n.len();
    [
        p.dot(n) / 3.0 * inv[0],
        p.x * p.x / 2.0 * n.x * inv[1],
        p.y * p.y / 2.0 * n.y * inv[1],
        p.z * p.z / 2.0 * n.z * inv[1],
        a * inv[2],
        p.x * a * inv[0],
        p.y * a * inv[0],
        p.z * a * inv[0],
    ]
}

/// `∫_{v0}^{v} g(u, s) ds`.
fn inner(s: &Surface, u: f64, v0: f64, v: f64, inv: [f64; 3]) -> [f64; 8] {
    if v == v0 {
        return [0.0; 8];
    }
    num::integrate_gl(v0, v, inner_pieces(s, v - v0), |w| integrand(s, u, w, inv))
}

/// (u, v) of the coedge at edge parameter `t`, and its derivative.
fn coedge_uv(s: &Surface, c: &CoUV, curve_d1: (V3, V3), t: f64) -> (V2, V2) {
    let (p, dp) = curve_d1;
    // Reference from the samples, to pick the period copy.
    let n = c.ts.len();
    let mut i = 0;
    let increasing = c.ts[n - 1] >= c.ts[0];
    while i + 2 < n
        && if increasing {
            c.ts[i + 1] < t
        } else {
            c.ts[i + 1] > t
        }
    {
        i += 1;
    }
    let (ta, tb) = (c.ts[i], c.ts[i + 1]);
    let f = if tb != ta { (t - ta) / (tb - ta) } else { 0.0 };
    let reference = c.uv[i] + (c.uv[i + 1] - c.uv[i]) * f;
    let (u, v) = s.project_near(p, (reference.x, reference.y));
    let wrap = |x: f64, r: f64, per: Option<f64>| match per {
        Some(pp) => x + ((r - x) / pp).round() * pp,
        None => x,
    };
    let q = v2(
        wrap(u, reference.x, s.period_u()),
        wrap(v, reference.y, s.period_v()),
    );
    let (_, su, sv) = s.d1(q.x, q.y);
    let (a, b, d) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let dq = match num::solve2(a, b, b, d, su.dot(dp), sv.dot(dp)) {
        Some((du, dv)) => v2(du, dv),
        None => V2::ZERO,
    };
    (q, dq)
}

/// How hard to work on a face: analytic surfaces are smooth and exact, so the quadrature
/// converges at once; a fitted B-spline (a blend written to and read back from a file)
/// carries its own approximation error, and chasing it to rounding would never end.
fn effort(s: &Surface) -> (f64, u32) {
    match s {
        Surface::Plane { .. }
        | Surface::Cylinder { .. }
        | Surface::Cone { .. }
        | Surface::Sphere { .. }
        | Surface::Torus { .. } => (1e-13, 18),
        Surface::Pipe { .. } | Surface::Ruled { .. } => (1e-11, 10),
        _ => (1e-9, 7),
    }
}

/// A face's flux and area integrals, scaled by `inv` = [1/L³, 1/L⁴, 1/L²].
fn face_integrals(solid: &Solid, fu: &FaceUV, f: usize, inv: [f64; 3]) -> FaceIntegrals {
    let s = &solid.faces[f].surface;
    let (tol, depth) = effort(s);
    let v0 = fu.lo.y;
    let mut acc = [0.0; 8];
    for l in &fu.loops {
        for c in l {
            let e = &solid.edges[c.co.edge];
            let part = if e.degenerate {
                // u runs along the pole at constant v.
                let (a, b) = (c.uv[0], c.uv[1]);
                let du = b.x - a.x;
                if du == 0.0 {
                    continue;
                }
                num::integrate_adaptive(0.0, 1.0, tol, depth, |w| {
                    let q = a + (b - a) * w;
                    let h = inner(s, q.x, v0, q.y, inv);
                    h.map(|x| -x * du)
                })
            } else {
                let (t0, t1) = (c.ts[0], *c.ts.last().unwrap());
                num::integrate_adaptive(t0, t1, tol, depth, |t| {
                    let d = e.curve.d1(t);
                    let (q, dq) = coedge_uv(s, c, d, t);
                    if dq.x == 0.0 {
                        return [0.0; 8];
                    }
                    let h = inner(s, q.x, v0, q.y, inv);
                    h.map(|x| -x * dq.x)
                })
            };
            for k in 0..8 {
                acc[k] += part[k];
            }
        }
    }
    // Area terms are unsigned: the loops' sense carries the orientation.
    for k in 4..8 {
        acc[k] *= fu.sense;
    }
    acc
}

fn scale_of(solid: &Solid) -> f64 {
    let b = solid.bounds();
    let d = b.diagonal();
    if d.is_finite() && d > 0.0 {
        d
    } else {
        1.0
    }
}

/// Area and centroid of every face.
pub fn face_props(solid: &Solid) -> Vec<FaceProps> {
    let l = scale_of(solid);
    let inv = [1.0 / (l * l * l), 1.0 / (l * l * l * l), 1.0 / (l * l)];
    (0..solid.faces.len())
        .map(|f| {
            let fu = face_uv(solid, f);
            let r = face_integrals(solid, &fu, f, inv);
            let area = r[4] * l * l;
            let c = V3 {
                x: r[5],
                y: r[6],
                z: r[7],
            } * (l * l * l);
            FaceProps {
                area,
                centroid: if area.abs() > 0.0 { c / area } else { V3::ZERO },
            }
        })
        .collect()
}

/// Volume, area and centre of mass of a closed solid.
pub fn mass_props(solid: &Solid) -> MassProps {
    let l = scale_of(solid);
    let inv = [1.0 / (l * l * l), 1.0 / (l * l * l * l), 1.0 / (l * l)];
    let mut tot = [0.0; 8];
    for f in 0..solid.faces.len() {
        let fu = face_uv(solid, f);
        let r = face_integrals(solid, &fu, f, inv);
        for k in 0..8 {
            tot[k] += r[k];
        }
    }
    let volume = tot[0] * l * l * l;
    let m = V3 {
        x: tot[1],
        y: tot[2],
        z: tot[3],
    } * (l * l * l * l);
    MassProps {
        volume,
        area: tot[4] * l * l,
        center: if volume.abs() > 0.0 { m / volume } else { V3::ZERO },
    }
}

/// Length of an edge.
pub fn edge_length(solid: &Solid, e: usize) -> f64 {
    let ed = &solid.edges[e];
    if ed.degenerate {
        return 0.0;
    }
    match &ed.curve {
        super::geom::Curve::Line { d, .. } => (ed.t1 - ed.t0).abs() * d.len(),
        super::geom::Curve::Circle { r, .. } => (ed.t1 - ed.t0).abs() * r,
        _ => {
            let scale = 1.0 + solid.edge_box(e).diagonal();
            let [len] = num::integrate_adaptive(ed.t0, ed.t1, 1e-13 * scale, 20, |t| {
                [ed.curve.d1(t).1.len()]
            });
            len.abs()
        }
    }
}

/// Mid-parameter point of an edge, used as its reference position.
pub fn edge_mid(solid: &Solid, e: usize) -> V3 {
    solid.edges[e].mid()
}

pub const HALF_PI: f64 = FRAC_PI_2;
