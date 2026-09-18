//! Exact geometry: the curves edges lie on and the surfaces faces lie on.
//!
//! Analytic types (lines, circles, ellipses; planes, cylinders, cones, spheres, tori) are
//! closed-form. Curves no closed form describes — the intersection of two cylinders, the
//! spine and contact curves of a rolling-ball fillet — are *traced* curves: a point is
//! found by Newton's method on the defining equations (two surfaces' distance functions
//! and a plane through a guide polyline), so every point lies on both surfaces to
//! rounding error and the derivative follows exactly from implicit differentiation. The
//! guide only chooses which point, never where it is.
//!
//! Every surface offers a parametrisation `S(u, v)` with first derivatives, the inverse
//! (projection), and a signed distance field with its gradient; the analytic ones' fields
//! are true Euclidean distances, which is what makes offset surfaces (for fillets) exact.
use super::num::{self, solve3v};
use crate::math::{self, v3, Frame, Xform, PI, TAU, V3};
use serde::{Deserialize, Serialize};

/// Frames under a transform, kept right-handed (a reflection flips `z`).
pub fn frame_xf(f: &Frame, x: &Xform) -> Frame {
    let o = x.point(f.origin);
    let ax = x.dir(f.x).norm();
    let ay = x.dir(f.y).norm();
    Frame {
        origin: o,
        x: ax,
        y: ay,
        z: ax.cross(ay).norm(),
    }
}

fn local(f: &Frame, p: V3) -> V3 {
    f.to_local(p)
}

// ---------------------------------------------------------------------------------
// B-splines.

/// A (rational) B-spline curve with a full knot vector.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BSplineCurve {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub poles: Vec<V3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<f64>>,
}

fn find_span(n: usize, p: usize, u: f64, knots: &[f64]) -> usize {
    // n = number of poles - 1.
    if u >= knots[n + 1] {
        return n;
    }
    if u <= knots[p] {
        return p;
    }
    let (mut lo, mut hi) = (p, n + 1);
    let mut mid = (lo + hi) / 2;
    while u < knots[mid] || u >= knots[mid + 1] {
        if u < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
        mid = (lo + hi) / 2;
    }
    mid
}

/// Highest B-spline degree the fixed-size working arrays handle.
const MAX_DEGREE: usize = 15;
/// Basis functions and their first derivatives at `u` (Piegl & Tiller A2.3), without
/// allocating: B-splines are evaluated in the innermost loops of the mass integrals.
type Ders = [[f64; MAX_DEGREE + 1]; 2];
fn basis_ders1(span: usize, u: f64, p: usize, knots: &[f64]) -> Ders {
    let mut ndu = [[0.0f64; MAX_DEGREE + 1]; MAX_DEGREE + 1];
    let mut left = [0.0f64; MAX_DEGREE + 1];
    let mut right = [0.0f64; MAX_DEGREE + 1];
    ndu[0][0] = 1.0;
    for j in 1..=p {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            ndu[j][r] = right[r + 1] + left[j - r];
            let temp = if ndu[j][r] != 0.0 {
                ndu[r][j - 1] / ndu[j][r]
            } else {
                0.0
            };
            ndu[r][j] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        ndu[j][j] = saved;
    }
    let mut out = [[0.0f64; MAX_DEGREE + 1]; 2];
    for j in 0..=p {
        out[0][j] = ndu[j][p];
    }
    // First derivative (the k = 1 row of A2.3).
    for r in 0..=p {
        let mut d = 0.0;
        let rk = r as i64 - 1;
        let pk = p as i64 - 1;
        let mut a0 = 0.0;
        if r >= 1 {
            a0 = 1.0 / ndu[(pk + 1) as usize][rk as usize];
            d = a0 * ndu[rk as usize][pk as usize];
        }
        let j1 = if rk >= -1 { 1 } else { (-rk) as usize };
        let j2 = if (r as i64 - 1) <= pk { 0 } else { p - r };
        let _ = (j1, j2);
        if r as i64 <= pk {
            let ak = -1.0 / ndu[(pk + 1) as usize][r];
            d += ak * ndu[r][pk as usize];
        }
        let _ = a0;
        out[1][r] = d * p as f64;
    }
    out
}

/// Basis functions and their derivatives up to `nd` at `u` (Piegl & Tiller A2.3).
fn basis_ders(span: usize, u: f64, p: usize, nd: usize, knots: &[f64]) -> Vec<Vec<f64>> {
    let mut ndu = vec![vec![0.0; p + 1]; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    ndu[0][0] = 1.0;
    for j in 1..=p {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            ndu[j][r] = right[r + 1] + left[j - r];
            let temp = if ndu[j][r] != 0.0 {
                ndu[r][j - 1] / ndu[j][r]
            } else {
                0.0
            };
            ndu[r][j] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        ndu[j][j] = saved;
    }
    let mut ders = vec![vec![0.0; p + 1]; nd + 1];
    for j in 0..=p {
        ders[0][j] = ndu[j][p];
    }
    let mut a = vec![vec![0.0; p + 1]; 2];
    for r in 0..=p {
        let (mut s1, mut s2) = (0usize, 1usize);
        a[0][0] = 1.0;
        for k in 1..=nd.min(p) {
            let mut d = 0.0;
            let rk = r as i64 - k as i64;
            let pk = p as i64 - k as i64;
            if r >= k {
                a[s2][0] = a[s1][0] / ndu[(pk + 1) as usize][rk as usize];
                d = a[s2][0] * ndu[rk as usize][pk as usize];
            }
            let j1 = if rk >= -1 { 1 } else { (-rk) as usize };
            let j2 = if (r as i64 - 1) <= pk { k - 1 } else { p - r };
            for j in j1..=j2 {
                let idx = (rk + j as i64) as usize;
                a[s2][j] = (a[s1][j] - a[s1][j - 1]) / ndu[(pk + 1) as usize][idx];
                d += a[s2][j] * ndu[idx][pk as usize];
            }
            if r as i64 <= pk {
                a[s2][k] = -a[s1][k - 1] / ndu[(pk + 1) as usize][r];
                d += a[s2][k] * ndu[r][pk as usize];
            }
            ders[k][r] = d;
            std::mem::swap(&mut s1, &mut s2);
        }
    }
    let mut r = p as f64;
    for k in 1..=nd.min(p) {
        for j in 0..=p {
            ders[k][j] *= r;
        }
        r *= (p - k) as f64;
    }
    ders
}

impl BSplineCurve {
    pub fn range(&self) -> (f64, f64) {
        (
            self.knots[self.degree],
            self.knots[self.knots.len() - 1 - self.degree],
        )
    }
    fn w(&self, i: usize) -> f64 {
        self.weights.as_ref().map_or(1.0, |w| w[i])
    }
    /// Point and first derivative.
    pub fn d1(&self, u: f64) -> (V3, V3) {
        let p = self.degree;
        let n = self.poles.len() - 1;
        let span = find_span(n, p, u, &self.knots);
        let fast = (p <= MAX_DEGREE).then(|| basis_ders1(span, u, p, &self.knots));
        let slow = fast.is_none().then(|| basis_ders(span, u, p, 1, &self.knots));
        let b = |k: usize, j: usize| match (&fast, &slow) {
            (Some(f), _) => f[k][j],
            (_, Some(s)) => s[k][j],
            _ => 0.0,
        };
        let (mut a, mut da) = (V3::ZERO, V3::ZERO);
        let (mut w, mut dw) = (0.0, 0.0);
        for j in 0..=p {
            let i = span - p + j;
            let wi = self.w(i);
            a += self.poles[i] * (b(0, j) * wi);
            da += self.poles[i] * (b(1, j) * wi);
            w += b(0, j) * wi;
            dw += b(1, j) * wi;
        }
        let c = a / w;
        (c, (da - c * dw) / w)
    }
}

/// A (rational) tensor-product B-spline surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BSplineSurface {
    pub du: usize,
    pub dv: usize,
    pub ku: Vec<f64>,
    pub kv: Vec<f64>,
    /// `poles[i][j]`: i along u, j along v.
    pub poles: Vec<Vec<V3>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<Vec<f64>>>,
}
impl BSplineSurface {
    pub fn range(&self) -> (f64, f64, f64, f64) {
        (
            self.ku[self.du],
            self.ku[self.ku.len() - 1 - self.du],
            self.kv[self.dv],
            self.kv[self.kv.len() - 1 - self.dv],
        )
    }
    pub fn d1(&self, u: f64, v: f64) -> (V3, V3, V3) {
        let nu = self.poles.len() - 1;
        let nv = self.poles[0].len() - 1;
        let su = find_span(nu, self.du, u, &self.ku);
        let sv = find_span(nv, self.dv, v, &self.kv);
        let (bu, bv) = if self.du <= MAX_DEGREE && self.dv <= MAX_DEGREE {
            (
                basis_ders1(su, u, self.du, &self.ku),
                basis_ders1(sv, v, self.dv, &self.kv),
            )
        } else {
            let (a, b) = (
                basis_ders(su, u, self.du, 1, &self.ku),
                basis_ders(sv, v, self.dv, 1, &self.kv),
            );
            let mut fa = [[0.0; MAX_DEGREE + 1]; 2];
            let mut fb = [[0.0; MAX_DEGREE + 1]; 2];
            for k in 0..2 {
                for j in 0..=self.du.min(MAX_DEGREE) {
                    fa[k][j] = a[k][j];
                }
                for j in 0..=self.dv.min(MAX_DEGREE) {
                    fb[k][j] = b[k][j];
                }
            }
            (fa, fb)
        };
        let (mut a, mut au, mut av) = (V3::ZERO, V3::ZERO, V3::ZERO);
        let (mut w, mut wu, mut wv) = (0.0, 0.0, 0.0);
        for i in 0..=self.du {
            for j in 0..=self.dv {
                let (ii, jj) = (su - self.du + i, sv - self.dv + j);
                let wt = self.weights.as_ref().map_or(1.0, |w| w[ii][jj]);
                let p = self.poles[ii][jj] * wt;
                a += p * (bu[0][i] * bv[0][j]);
                au += p * (bu[1][i] * bv[0][j]);
                av += p * (bu[0][i] * bv[1][j]);
                w += wt * bu[0][i] * bv[0][j];
                wu += wt * bu[1][i] * bv[0][j];
                wv += wt * bu[0][i] * bv[1][j];
            }
        }
        let s = a / w;
        (s, (au - s * wu) / w, (av - s * wv) / w)
    }
}

// ---------------------------------------------------------------------------------
// Curves.

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Curve {
    /// `o + t d`, `d` unit.
    Line { o: V3, d: V3 },
    /// `o + r (cos t x + sin t y)`.
    Circle { f: Frame, r: f64 },
    /// `o + a cos t x + b sin t y`, `a ≥ b`.
    Ellipse { f: Frame, a: f64, b: f64 },
    BSpline(Box<BSplineCurve>),
    Traced(Box<Traced>),
}

/// A curve defined implicitly, evaluated by Newton's method.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Traced {
    pub kind: TraceKind,
    /// Guide points and their parameters (for `Inter`, cumulative chord length; else the
    /// base curve's parameter). A closed guide repeats its first point at the end.
    pub pts: Vec<V3>,
    pub ts: Vec<f64>,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "trace", rename_all = "snake_case")]
pub enum TraceKind {
    /// Points where `a`'s distance is `oa` and `b`'s is `ob`: the intersection of two
    /// surfaces (offsets zero) or of two offset surfaces (a fillet's spine).
    Inter {
        a: Surface,
        oa: f64,
        b: Surface,
        ob: f64,
    },
    /// The foot of the perpendicular from `spine(t)` onto `s`: a fillet's contact curve.
    Foot { spine: Curve, s: Surface },
    /// The point of `s` at chord distance `d` from `base(t)`, in the plane normal to the
    /// base there, on the guide's side: a chamfer's contact curve.
    Chord { base: Curve, s: Surface, d: f64 },
}

impl Traced {
    pub fn range(&self) -> (f64, f64) {
        (self.ts[0], *self.ts.last().unwrap())
    }
    /// Guide position and its derivative at `t`.
    fn guide(&self, t: f64) -> (V3, V3) {
        let n = self.pts.len();
        let (t0, t1) = self.range();
        let t = if self.closed && t1 > t0 {
            let l = t1 - t0;
            t0 + (t - t0).rem_euclid(l)
        } else {
            t
        };
        // Binary search for the segment.
        let mut lo = 0usize;
        let mut hi = n - 1;
        if t <= self.ts[0] {
            hi = 1;
        } else if t >= self.ts[n - 1] {
            lo = n - 2;
        } else {
            while hi - lo > 1 {
                let mid = (lo + hi) / 2;
                if self.ts[mid] <= t {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
        }
        let (a, b) = (self.pts[lo], self.pts[lo + 1]);
        let (ta, tb) = (self.ts[lo], self.ts[lo + 1]);
        let dt = tb - ta;
        if dt <= 0.0 {
            return (a, (b - a).norm());
        }
        let s = (t - ta) / dt;
        (a + (b - a) * s, (b - a) / dt)
    }
    pub fn d1(&self, t: f64) -> (V3, V3) {
        match &self.kind {
            TraceKind::Inter { a, oa, b, ob } => {
                let (q, dq) = self.guide(t);
                let d = dq.norm();
                let mut p = q;
                let scale = 1.0 + q.len();
                for _ in 0..40 {
                    let (fa, ga) = (a.sd(p) - oa, a.grad(p));
                    let (fb, gb) = (b.sd(p) - ob, b.grad(p));
                    let fc = (p - q).dot(d);
                    let Some(step) = solve3v([ga, gb, d], [-fa, -fb, -fc]) else {
                        break;
                    };
                    p += step;
                    if step.len() <= 1e-15 * scale {
                        break;
                    }
                }
                let tan = a.grad(p).cross(b.grad(p));
                // p'·d = q'·d  ⇒  λ (tan·d) = q'·d.
                let lam = if tan.dot(d).abs() > 1e-300 {
                    dq.dot(d) / tan.dot(d)
                } else {
                    0.0
                };
                (p, tan * lam)
            }
            TraceKind::Foot { spine, s } => {
                let (c, dc) = spine.d1(t);
                foot_point(s, c, dc)
            }
            TraceKind::Chord { base, s, d } => {
                let (e, de) = base.d1(t);
                let dd = base.d2(t);
                let (q, _) = self.guide(t);
                let mut p = q;
                let scale = 1.0 + e.len();
                for _ in 0..40 {
                    let f1 = s.sd(p);
                    let f2 = (p - e).len2() - d * d;
                    let f3 = (p - e).dot(de);
                    let Some(step) =
                        solve3v([s.grad(p), (p - e) * 2.0, de], [-f1, -f2, -f3])
                    else {
                        break;
                    };
                    p += step;
                    if step.len() <= 1e-15 * scale {
                        break;
                    }
                }
                let rhs = [0.0, (p - e).dot(de), de.dot(de) - (p - e).dot(dd)];
                let dp = solve3v([s.grad(p), p - e, de], rhs).unwrap_or(de);
                (p, dp)
            }
        }
    }
}

/// Foot of the perpendicular from `c` onto `s`, and its derivative when `c` moves at `dc`.
fn foot_point(s: &Surface, c: V3, dc: V3) -> (V3, V3) {
    if s.exact_distance() {
        let d = s.sd(c);
        let g = s.grad(c);
        let h = s.hess(c);
        let hc = v3(
            h[0][0] * dc.x + h[0][1] * dc.y + h[0][2] * dc.z,
            h[1][0] * dc.x + h[1][1] * dc.y + h[1][2] * dc.z,
            h[2][0] * dc.x + h[2][1] * dc.y + h[2][2] * dc.z,
        );
        (c - g * d, dc - g * g.dot(dc) - hc * d)
    } else {
        let (u, v) = s.project(c);
        let p = s.eval(u, v);
        let h = 1e-6 * (1.0 + c.len());
        let (u2, v2) = s.project_near(c + dc * h, (u, v));
        let (u1, v1) = s.project_near(c - dc * h, (u, v));
        (p, (s.eval(u2, v2) - s.eval(u1, v1)) / (2.0 * h))
    }
}

impl Curve {
    pub fn name(&self) -> &'static str {
        match self {
            Curve::Line { .. } => "Line",
            Curve::Circle { .. } => "Circle",
            Curve::Ellipse { .. } => "Ellipse",
            Curve::BSpline(_) => "BSplineCurve",
            Curve::Traced(_) => "BSplineCurve",
        }
    }
    pub fn period(&self) -> Option<f64> {
        match self {
            Curve::Circle { .. } | Curve::Ellipse { .. } => Some(TAU),
            Curve::Traced(t) if t.closed => {
                let (a, b) = t.range();
                Some(b - a)
            }
            _ => None,
        }
    }
    pub fn eval(&self, t: f64) -> V3 {
        self.d1(t).0
    }
    pub fn d1(&self, t: f64) -> (V3, V3) {
        match self {
            Curve::Line { o, d } => (*o + *d * t, *d),
            Curve::Circle { f, r } => {
                let (s, c) = math::sin_cos(t);
                (
                    f.origin + f.x * (r * c) + f.y * (r * s),
                    f.x * (-r * s) + f.y * (r * c),
                )
            }
            Curve::Ellipse { f, a, b } => {
                let (s, c) = math::sin_cos(t);
                (
                    f.origin + f.x * (a * c) + f.y * (b * s),
                    f.x * (-a * s) + f.y * (b * c),
                )
            }
            Curve::BSpline(b) => b.d1(t),
            Curve::Traced(tr) => tr.d1(t),
        }
    }
    /// Second derivative (numerical for B-splines and traced curves).
    pub fn d2(&self, t: f64) -> V3 {
        match self {
            Curve::Line { .. } => V3::ZERO,
            Curve::Circle { f, r } => {
                let (s, c) = math::sin_cos(t);
                f.x * (-r * c) + f.y * (-r * s)
            }
            Curve::Ellipse { f, a, b } => {
                let (s, c) = math::sin_cos(t);
                f.x * (-a * c) + f.y * (-b * s)
            }
            _ => {
                let h = 1e-5 * (1.0 + t.abs());
                (self.d1(t + h).1 - self.d1(t - h).1) / (2.0 * h)
            }
        }
    }
    /// The parameter of the point of the curve nearest `p`.
    pub fn project(&self, p: V3) -> f64 {
        match self {
            Curve::Line { o, d } => (p - *o).dot(*d),
            Curve::Circle { f, .. } => {
                let l = local(f, p);
                math::atan2(l.y, l.x)
            }
            Curve::Ellipse { f, a, b } => {
                let l = local(f, p);
                let t0 = math::atan2(l.y / b, l.x / a);
                self.newton_project(p, t0)
            }
            Curve::BSpline(b) => {
                let (lo, hi) = b.range();
                let n = b.poles.len() * 8 + 16;
                let t0 = self.sampled_nearest(p, lo, hi, n);
                self.newton_project(p, t0).clamp(lo, hi)
            }
            Curve::Traced(tr) if tr.pts.len() < 2 => {
                let (lo, hi) = tr.range();
                let t0 = self.sampled_nearest(p, lo, hi, 256);
                let t = self.newton_project(p, t0);
                if tr.closed {
                    t
                } else {
                    t.clamp(lo, hi)
                }
            }
            Curve::Traced(tr) => {
                // Nearest guide segment point, then Newton.
                let mut best = (f64::INFINITY, tr.ts[0]);
                for i in 0..tr.pts.len() - 1 {
                    let (a, b) = (tr.pts[i], tr.pts[i + 1]);
                    let ab = b - a;
                    let l2 = ab.len2();
                    let s = if l2 > 0.0 {
                        ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let d = (a + ab * s).dist(p);
                    if d < best.0 {
                        best = (d, tr.ts[i] + (tr.ts[i + 1] - tr.ts[i]) * s);
                    }
                }
                let (lo, hi) = tr.range();
                let t = self.newton_project(p, best.1);
                if tr.closed {
                    t
                } else {
                    t.clamp(lo, hi)
                }
            }
        }
    }
    fn sampled_nearest(&self, p: V3, lo: f64, hi: f64, n: usize) -> f64 {
        let mut best = (f64::INFINITY, lo);
        for i in 0..=n {
            let t = lo + (hi - lo) * i as f64 / n as f64;
            let d = self.eval(t).dist(p);
            if d < best.0 {
                best = (d, t);
            }
        }
        best.1
    }
    fn newton_project(&self, p: V3, t0: f64) -> f64 {
        let mut t = t0;
        for _ in 0..30 {
            let (c, d) = self.d1(t);
            let dd = self.d2(t);
            let f = (c - p).dot(d);
            let df = d.dot(d) + (c - p).dot(dd);
            if df.abs() < 1e-300 {
                break;
            }
            let step = f / df;
            let step = if df <= 0.0 { f / d.dot(d).max(1e-300) } else { step };
            t -= step;
            if step.abs() <= 1e-15 * (1.0 + t.abs()) {
                break;
            }
        }
        t
    }
    /// The parameter of `p` (on the curve) taken within `[lo, hi]` for periodic curves.
    pub fn param_in(&self, p: V3, lo: f64, hi: f64) -> f64 {
        let t = self.project(p);
        match self.period() {
            Some(per) => {
                let mid = (lo + hi) / 2.0;
                let k = ((mid - t) / per).round();
                let mut t = t + k * per;
                let tol = 1e-9 * per;
                if t < lo - tol {
                    t += per;
                }
                if t > hi + tol {
                    t -= per;
                }
                t
            }
            None => t,
        }
    }
    pub fn transformed(&self, x: &Xform) -> Curve {
        match self {
            Curve::Line { o, d } => Curve::Line {
                o: x.point(*o),
                d: x.dir(*d).norm(),
            },
            Curve::Circle { f, r } => {
                // A reflection reverses the sense: keep the parameter's direction by
                // mirroring the frame's y instead of letting z flip.
                Curve::Circle {
                    f: frame_keep_sense(f, x),
                    r: *r,
                }
            }
            Curve::Ellipse { f, a, b } => Curve::Ellipse {
                f: frame_keep_sense(f, x),
                a: *a,
                b: *b,
            },
            Curve::BSpline(b) => Curve::BSpline(Box::new(BSplineCurve {
                poles: b.poles.iter().map(|p| x.point(*p)).collect(),
                ..(**b).clone()
            })),
            Curve::Traced(t) => Curve::Traced(Box::new(Traced {
                kind: match &t.kind {
                    TraceKind::Inter { a, oa, b, ob } => {
                        let (sa, sb) = (a.transformed(x), b.transformed(x));
                        // Distance signs follow each surface's own convention, which a
                        // reflection may turn round.
                        let fa = a.sign_after(x, &sa);
                        let fb = b.sign_after(x, &sb);
                        TraceKind::Inter {
                            a: sa,
                            oa: oa * fa,
                            b: sb,
                            ob: ob * fb,
                        }
                    }
                    TraceKind::Foot { spine, s } => TraceKind::Foot {
                        spine: spine.transformed(x),
                        s: s.transformed(x),
                    },
                    TraceKind::Chord { base, s, d } => TraceKind::Chord {
                        base: base.transformed(x),
                        s: s.transformed(x),
                        d: *d,
                    },
                },
                pts: t.pts.iter().map(|p| x.point(*p)).collect(),
                ts: t.ts.clone(),
                closed: t.closed,
            })),
        }
    }
    /// Whether `other` is the same curve (same point set), possibly parametrised
    /// differently.
    pub fn same(&self, other: &Curve, tol: f64) -> bool {
        match (self, other) {
            (Curve::Line { o, d }, Curve::Line { o: o2, d: d2 }) => {
                d.cross(*d2).len() < 1e-9 && {
                    let w = *o2 - *o;
                    (w - *d * w.dot(*d)).len() < tol
                }
            }
            (Curve::Circle { f, r }, Curve::Circle { f: f2, r: r2 }) => {
                (r - r2).abs() < tol
                    && f.origin.dist(f2.origin) < tol
                    && f.z.cross(f2.z).len() < 1e-9
            }
            _ => false,
        }
    }
}

/// A frame moved by `x`, with `y` taken from the transform too so a reflected circle is
/// still traversed the same way relative to its (moved) points.
fn frame_keep_sense(f: &Frame, x: &Xform) -> Frame {
    let o = x.point(f.origin);
    let ax = x.dir(f.x).norm();
    let ay = x.dir(f.y).norm();
    // For a reflection x×y points the other way; z records the plane normal regardless.
    Frame {
        origin: o,
        x: ax,
        y: ay,
        z: ax.cross(ay).norm(),
    }
}

// ---------------------------------------------------------------------------------
// Surfaces.

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Surface {
    /// `o + u x + v y`.
    Plane { f: Frame },
    /// `o + r (cos u x + sin u y) + v z`.
    Cylinder { f: Frame, r: f64 },
    /// `o + (r + v tan a)(cos u x + sin u y) + v z`: radius `r` at `v = 0`, semi-angle `a`.
    Cone { f: Frame, r: f64, a: f64 },
    /// `o + r cos v (cos u x + sin u y) + r sin v z`.
    Sphere { f: Frame, r: f64 },
    /// `o + (major + minor cos v)(cos u x + sin u y) + minor sin v z`.
    Torus { f: Frame, major: f64, minor: f64 },
    /// `curve` (the generatrix at `u = 0`) turned by `u` about the frame's z axis.
    Revolution {
        f: Frame,
        curve: Box<Curve>,
        t0: f64,
        t1: f64,
    },
    /// `curve(u) + v dir`.
    Extrusion {
        curve: Box<Curve>,
        dir: V3,
        t0: f64,
        t1: f64,
    },
    /// A tube of radius `r` round `spine`: a rolling-ball fillet. `u` runs along the
    /// spine, `v` is the angle from the direction towards `toward` (the surface the
    /// ball touches first), turning towards the right-hand side of the spine.
    Pipe {
        spine: Box<Curve>,
        r: f64,
        toward: Box<Surface>,
        t0: f64,
        t1: f64,
    },
    /// Straight lines from `a(u)` to `b(u)`, `v` in [0, 1]: a chamfer between curved faces.
    Ruled {
        a: Box<Curve>,
        b: Box<Curve>,
        t0: f64,
        t1: f64,
    },
    BSpline(Box<BSplineSurface>),
}

/// 3×3 matrix rows.
pub type M3 = [[f64; 3]; 3];
fn outer(a: V3, b: V3) -> M3 {
    [
        [a.x * b.x, a.x * b.y, a.x * b.z],
        [a.y * b.x, a.y * b.y, a.y * b.z],
        [a.z * b.x, a.z * b.y, a.z * b.z],
    ]
}
fn madd(a: M3, b: M3, s: f64) -> M3 {
    let mut o = a;
    for i in 0..3 {
        for j in 0..3 {
            o[i][j] += b[i][j] * s;
        }
    }
    o
}
const I3: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

impl Surface {
    pub fn name(&self) -> &'static str {
        match self {
            Surface::Plane { .. } => "Plane",
            Surface::Cylinder { .. } => "Cylinder",
            Surface::Cone { .. } => "Cone",
            Surface::Sphere { .. } => "Sphere",
            Surface::Torus { .. } => "Toroid",
            Surface::Revolution { .. } => "SurfaceOfRevolution",
            Surface::Extrusion { .. } => "SurfaceOfExtrusion",
            Surface::Pipe { .. } | Surface::Ruled { .. } | Surface::BSpline(_) => "BSplineSurface",
        }
    }
    pub fn frame(&self) -> Option<&Frame> {
        match self {
            Surface::Plane { f }
            | Surface::Cylinder { f, .. }
            | Surface::Cone { f, .. }
            | Surface::Sphere { f, .. }
            | Surface::Torus { f, .. }
            | Surface::Revolution { f, .. } => Some(f),
            _ => None,
        }
    }
    /// Whether `sd` is the true Euclidean signed distance (so offsets are exact).
    pub fn exact_distance(&self) -> bool {
        matches!(
            self,
            Surface::Plane { .. }
                | Surface::Cylinder { .. }
                | Surface::Cone { .. }
                | Surface::Sphere { .. }
                | Surface::Torus { .. }
                | Surface::Pipe { .. }
        )
    }
    pub fn period_u(&self) -> Option<f64> {
        match self {
            Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
            | Surface::Revolution { .. } => Some(TAU),
            Surface::Pipe { spine, .. } => spine.period(),
            _ => None,
        }
    }
    pub fn period_v(&self) -> Option<f64> {
        match self {
            Surface::Torus { .. } => Some(TAU),
            Surface::Pipe { .. } => Some(TAU),
            _ => None,
        }
    }
    pub fn eval(&self, u: f64, v: f64) -> V3 {
        self.d1(u, v).0
    }
    /// `S`, `∂S/∂u`, `∂S/∂v`.
    pub fn d1(&self, u: f64, v: f64) -> (V3, V3, V3) {
        match self {
            Surface::Plane { f } => (f.origin + f.x * u + f.y * v, f.x, f.y),
            Surface::Cylinder { f, r } => {
                let (s, c) = math::sin_cos(u);
                let er = f.x * c + f.y * s;
                let et = f.y * c - f.x * s;
                (f.origin + er * *r + f.z * v, et * *r, f.z)
            }
            Surface::Cone { f, r, a } => {
                let (s, c) = math::sin_cos(u);
                let t = math::tan(*a);
                let rho = r + v * t;
                let er = f.x * c + f.y * s;
                let et = f.y * c - f.x * s;
                (f.origin + er * rho + f.z * v, et * rho, er * t + f.z)
            }
            Surface::Sphere { f, r } => {
                let (su, cu) = math::sin_cos(u);
                let (sv, cv) = math::sin_cos(v);
                let er = f.x * cu + f.y * su;
                let et = f.y * cu - f.x * su;
                (
                    f.origin + er * (r * cv) + f.z * (r * sv),
                    et * (r * cv),
                    er * (-r * sv) + f.z * (r * cv),
                )
            }
            Surface::Torus { f, major, minor } => {
                let (su, cu) = math::sin_cos(u);
                let (sv, cv) = math::sin_cos(v);
                let er = f.x * cu + f.y * su;
                let et = f.y * cu - f.x * su;
                let rho = major + minor * cv;
                (
                    f.origin + er * rho + f.z * (minor * sv),
                    et * rho,
                    er * (-minor * sv) + f.z * (minor * cv),
                )
            }
            Surface::Revolution { f, curve, .. } => {
                let (c, dc) = curve.d1(v);
                let rot = Xform::rotate(f.origin, f.z, u);
                let p = rot.point(c);
                (p, f.z.cross(p - f.origin), rot.dir(dc))
            }
            Surface::Extrusion { curve, dir, .. } => {
                let (c, dc) = curve.d1(u);
                (c + *dir * v, dc, *dir)
            }
            Surface::Pipe {
                spine, r, toward, ..
            } => {
                let (c, dc) = spine.d1(u);
                let (n, b, dn, db) = pipe_frame(spine, toward, u, c, dc);
                let (s, co) = math::sin_cos(v);
                let radial = n * co + b * s;
                (
                    c + radial * *r,
                    dc + (dn * co + db * s) * *r,
                    (b * co - n * s) * *r,
                )
            }
            Surface::Ruled { a, b, .. } => {
                let (pa, da) = a.d1(u);
                let (pb, db) = b.d1(u);
                (pa + (pb - pa) * v, da + (db - da) * v, pb - pa)
            }
            Surface::BSpline(b) => b.d1(u, v),
        }
    }
    /// Unit normal of the parametrisation, `∂S/∂u × ∂S/∂v` normalised.
    pub fn normal(&self, u: f64, v: f64) -> V3 {
        let (_, su, sv) = self.d1(u, v);
        su.cross(sv).norm()
    }
    /// Parameter range for sampling surfaces with no closed-form projection.
    pub fn domain(&self) -> (f64, f64, f64, f64) {
        match self {
            Surface::Revolution { t0, t1, .. } => (0.0, TAU, *t0, *t1),
            Surface::Extrusion { t0, t1, .. } => (*t0, *t1, -1.0, 1.0),
            Surface::Pipe { t0, t1, .. } => (*t0, *t1, -PI, PI),
            Surface::Ruled { t0, t1, .. } => (*t0, *t1, 0.0, 1.0),
            Surface::BSpline(b) => b.range(),
            Surface::Sphere { .. } => (-PI, PI, -PI / 2.0, PI / 2.0),
            Surface::Torus { .. } => (-PI, PI, -PI, PI),
            _ => (-PI, PI, -1.0, 1.0),
        }
    }
    /// Parameters of the point of the surface nearest `p`.
    pub fn project(&self, p: V3) -> (f64, f64) {
        match self {
            Surface::Plane { f } => {
                let l = local(f, p);
                (l.x, l.y)
            }
            Surface::Cylinder { f, .. } => {
                let l = local(f, p);
                (math::atan2(l.y, l.x), l.z)
            }
            Surface::Cone { f, r, a } => {
                let l = local(f, p);
                let t = math::tan(*a);
                let rho = math::hypot(l.x, l.y);
                let h = (l.z + t * (rho - r)) / (1.0 + t * t);
                (math::atan2(l.y, l.x), h)
            }
            Surface::Sphere { f, .. } => {
                let l = local(f, p);
                (
                    math::atan2(l.y, l.x),
                    math::atan2(l.z, math::hypot(l.x, l.y)),
                )
            }
            Surface::Torus { f, major, .. } => {
                let l = local(f, p);
                let rho = math::hypot(l.x, l.y);
                (math::atan2(l.y, l.x), math::atan2(l.z, rho - major))
            }
            Surface::Revolution { f, curve, t0, t1 } => {
                let l = local(f, p);
                let u = math::atan2(l.y, l.x);
                let back = Xform::rotate(f.origin, f.z, -u).point(p);
                let t = curve.project(back).clamp(*t0, *t1);
                self.refine_projection(p, (u, t))
            }
            Surface::Pipe { spine, t0, t1, .. } => {
                let mut t = spine.project(p);
                if spine.period().is_none() {
                    t = t.clamp(*t0, *t1);
                }
                let (c, dc) = spine.d1(t);
                let (n, b, _, _) = self.pipe_frame_at(t, c, dc);
                let w = p - c;
                (t, math::atan2(w.dot(b), w.dot(n)))
            }
            _ => {
                let seed = self.sample_nearest(p);
                self.refine_projection(p, seed)
            }
        }
    }
    /// Projection starting from a known nearby parameter pair.
    pub fn project_near(&self, p: V3, seed: (f64, f64)) -> (f64, f64) {
        match self {
            Surface::Plane { .. }
            | Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. } => self.project(p),
            _ => self.refine_projection(p, seed),
        }
    }
    fn pipe_frame_at(&self, t: f64, c: V3, dc: V3) -> (V3, V3, V3, V3) {
        match self {
            Surface::Pipe { spine, toward, .. } => pipe_frame(spine, toward, t, c, dc),
            _ => unreachable!(),
        }
    }
    fn sample_nearest(&self, p: V3) -> (f64, f64) {
        let (u0, u1, v0, v1) = self.domain();
        let n = 24;
        let mut best = (f64::INFINITY, (u0, v0));
        for i in 0..=n {
            for j in 0..=n {
                let u = u0 + (u1 - u0) * i as f64 / n as f64;
                let v = v0 + (v1 - v0) * j as f64 / n as f64;
                let d = self.eval(u, v).dist(p);
                if d < best.0 {
                    best = (d, (u, v));
                }
            }
        }
        best.1
    }
    /// Gauss–Newton on |S(u, v) − p|².
    fn refine_projection(&self, p: V3, seed: (f64, f64)) -> (f64, f64) {
        let (mut u, mut v) = seed;
        let scale = 1.0 + p.len();
        for _ in 0..60 {
            let (s, su, sv) = self.d1(u, v);
            let r = p - s;
            let (a, b, d) = (su.dot(su), su.dot(sv), sv.dot(sv));
            let Some((du, dv)) = num::solve2(a, b, b, d, su.dot(r), sv.dot(r)) else {
                break;
            };
            u += du;
            v += dv;
            if (su * du + sv * dv).len() <= 1e-15 * scale {
                break;
            }
        }
        (u, v)
    }
    /// Signed distance, positive on the side the convention calls outside (away from a
    /// plane's normal side is negative; outside a cylinder, cone, sphere or torus tube is
    /// positive; other surfaces follow their parametric normal).
    pub fn sd(&self, p: V3) -> f64 {
        match self {
            Surface::Plane { f } => (p - f.origin).dot(f.z),
            Surface::Cylinder { f, r } => {
                let l = local(f, p);
                math::hypot(l.x, l.y) - r
            }
            Surface::Cone { f, r, a } => {
                let l = local(f, p);
                let (sa, ca) = math::sin_cos(*a);
                let rho = math::hypot(l.x, l.y);
                (rho - r) * ca - l.z * sa
            }
            Surface::Sphere { f, r } => (p - f.origin).len() - r,
            Surface::Torus { f, major, minor } => {
                let l = local(f, p);
                let rho = math::hypot(l.x, l.y);
                math::hypot(rho - major, l.z) - minor
            }
            Surface::Pipe { spine, r, t0, t1, .. } => {
                let mut t = spine.project(p);
                if spine.period().is_none() {
                    t = t.clamp(*t0, *t1);
                }
                spine.eval(t).dist(p) - r
            }
            _ => {
                let (u, v) = self.project(p);
                let (s, su, sv) = self.d1(u, v);
                (p - s).dot(su.cross(sv).norm())
            }
        }
    }
    /// Gradient of `sd` (unit for true distances).
    pub fn grad(&self, p: V3) -> V3 {
        match self {
            Surface::Plane { f } => f.z,
            Surface::Cylinder { f, .. } => {
                let l = local(f, p);
                let rho = math::hypot(l.x, l.y);
                if rho < 1e-300 {
                    return f.x;
                }
                (f.x * l.x + f.y * l.y) / rho
            }
            Surface::Cone { f, a, .. } => {
                let l = local(f, p);
                let (sa, ca) = math::sin_cos(*a);
                let rho = math::hypot(l.x, l.y);
                let er = if rho < 1e-300 {
                    f.x
                } else {
                    (f.x * l.x + f.y * l.y) / rho
                };
                er * ca - f.z * sa
            }
            Surface::Sphere { f, .. } => {
                let d = p - f.origin;
                if d.len() < 1e-300 {
                    f.z
                } else {
                    d.norm()
                }
            }
            Surface::Torus { f, major, .. } => {
                let l = local(f, p);
                let rho = math::hypot(l.x, l.y);
                let er = if rho < 1e-300 {
                    f.x
                } else {
                    (f.x * l.x + f.y * l.y) / rho
                };
                let q = math::hypot(rho - major, l.z);
                if q < 1e-300 {
                    return er;
                }
                er * ((rho - major) / q) + f.z * (l.z / q)
            }
            Surface::Pipe { spine, t0, t1, .. } => {
                let mut t = spine.project(p);
                if spine.period().is_none() {
                    t = t.clamp(*t0, *t1);
                }
                (p - spine.eval(t)).norm()
            }
            _ => {
                let (u, v) = self.project(p);
                self.normal(u, v)
            }
        }
    }
    /// Hessian of `sd` (the gradient's Jacobian).
    pub fn hess(&self, p: V3) -> M3 {
        match self {
            Surface::Plane { .. } => [[0.0; 3]; 3],
            Surface::Cylinder { f, .. } => {
                let l = local(f, p);
                let rho = math::hypot(l.x, l.y).max(1e-300);
                let g = self.grad(p);
                let m = madd(madd(I3, outer(f.z, f.z), -1.0), outer(g, g), -1.0);
                m.map(|r| r.map(|x| x / rho))
            }
            Surface::Sphere { f, .. } => {
                let d = (p - f.origin).len().max(1e-300);
                let g = self.grad(p);
                madd(I3, outer(g, g), -1.0).map(|r| r.map(|x| x / d))
            }
            _ => {
                let h = 1e-6 * (1.0 + p.len());
                let mut m = [[0.0; 3]; 3];
                for (j, e) in [V3::X, V3::Y, V3::Z].into_iter().enumerate() {
                    let d = (self.grad(p + e * h) - self.grad(p - e * h)) / (2.0 * h);
                    m[0][j] = d.x;
                    m[1][j] = d.y;
                    m[2][j] = d.z;
                }
                m
            }
        }
    }
    pub fn transformed(&self, x: &Xform) -> Surface {
        match self {
            Surface::Plane { f } => Surface::Plane { f: frame_xf(f, x) },
            Surface::Cylinder { f, r } => Surface::Cylinder {
                f: frame_xf(f, x),
                r: *r,
            },
            Surface::Cone { f, r, a } => {
                // The frame's z decides which way the radius grows; keep it so.
                let nf = frame_xf(f, x);
                let z = x.dir(f.z).norm();
                if nf.z.dot(z) < 0.0 {
                    Surface::Cone {
                        f: Frame {
                            origin: nf.origin,
                            x: nf.x,
                            y: -nf.y,
                            z,
                        },
                        r: *r,
                        a: *a,
                    }
                } else {
                    Surface::Cone { f: nf, r: *r, a: *a }
                }
            }
            Surface::Sphere { f, r } => Surface::Sphere {
                f: frame_xf(f, x),
                r: *r,
            },
            Surface::Torus { f, major, minor } => {
                let nf = frame_xf(f, x);
                Surface::Torus {
                    f: nf,
                    major: *major,
                    minor: *minor,
                }
            }
            Surface::Revolution { f, curve, t0, t1 } => Surface::Revolution {
                f: frame_xf(f, x),
                curve: Box::new(curve.transformed(x)),
                t0: *t0,
                t1: *t1,
            },
            Surface::Extrusion {
                curve,
                dir,
                t0,
                t1,
            } => Surface::Extrusion {
                curve: Box::new(curve.transformed(x)),
                dir: x.dir(*dir).norm(),
                t0: *t0,
                t1: *t1,
            },
            Surface::Pipe {
                spine,
                r,
                toward,
                t0,
                t1,
            } => Surface::Pipe {
                spine: Box::new(spine.transformed(x)),
                r: *r,
                toward: Box::new(toward.transformed(x)),
                t0: *t0,
                t1: *t1,
            },
            Surface::Ruled { a, b, t0, t1 } => Surface::Ruled {
                a: Box::new(a.transformed(x)),
                b: Box::new(b.transformed(x)),
                t0: *t0,
                t1: *t1,
            },
            Surface::BSpline(b) => Surface::BSpline(Box::new(BSplineSurface {
                poles: b
                    .poles
                    .iter()
                    .map(|row| row.iter().map(|p| x.point(*p)).collect())
                    .collect(),
                ..(**b).clone()
            })),
        }
    }
    /// The factor a distance measured by `self` becomes when measured by `moved`, the
    /// same surface after `x`: −1 where a reflection turned the convention round.
    pub fn sign_after(&self, x: &Xform, moved: &Surface) -> f64 {
        match self {
            Surface::Plane { f } => {
                if x.dir(f.z).dot(moved.frame().map(|g| g.z).unwrap_or(f.z)) < 0.0 {
                    -1.0
                } else {
                    1.0
                }
            }
            Surface::Cylinder { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
            | Surface::Cone { .. }
            | Surface::Pipe { .. } => 1.0,
            _ => {
                if x.det() < 0.0 {
                    -1.0
                } else {
                    1.0
                }
            }
        }
    }
    /// Whether two surfaces are the same point set.
    pub fn same(&self, o: &Surface, tol: f64) -> bool {
        let axis_same = |f: &Frame, g: &Frame| {
            f.z.cross(g.z).len() < 1e-9 && {
                let w = g.origin - f.origin;
                (w - f.z * w.dot(f.z)).len() < tol
            }
        };
        match (self, o) {
            (Surface::Plane { f }, Surface::Plane { f: g }) => {
                f.z.cross(g.z).len() < 1e-9 && (g.origin - f.origin).dot(f.z).abs() < tol
            }
            (Surface::Cylinder { f, r }, Surface::Cylinder { f: g, r: r2 }) => {
                (r - r2).abs() < tol && axis_same(f, g)
            }
            (Surface::Sphere { f, r }, Surface::Sphere { f: g, r: r2 }) => {
                (r - r2).abs() < tol && f.origin.dist(g.origin) < tol
            }
            (
                Surface::Torus { f, major, minor },
                Surface::Torus {
                    f: g,
                    major: m2,
                    minor: n2,
                },
            ) => {
                (major - m2).abs() < tol
                    && (minor - n2).abs() < tol
                    && f.z.cross(g.z).len() < 1e-9
                    && f.origin.dist(g.origin) < tol
            }
            (Surface::Cone { f, r, a }, Surface::Cone { f: g, r: r2, a: a2 }) => {
                // Same axis, same apex, same angle (either nappe orientation).
                if !axis_same(f, g) {
                    return false;
                }
                let apex = |f: &Frame, r: f64, a: f64| f.origin - f.z * (r / math::tan(a));
                let (p1, p2) = (apex(f, *r, *a), apex(g, *r2, *a2));
                let same_dir = f.z.dot(g.z) > 0.0;
                p1.dist(p2) < tol
                    && if same_dir {
                        (a - a2).abs() < 1e-9
                    } else {
                        (a + a2).abs() < 1e-9
                    }
            }
            _ => false,
        }
    }
    /// Whether the surface is a plane, cylinder, extrusion… swept along `d` (so a
    /// section across `d` is the same everywhere).
    pub fn extruded_along(&self, d: V3) -> bool {
        match self {
            Surface::Plane { f } => f.z.dot(d).abs() < 1e-9,
            Surface::Cylinder { f, .. } => f.z.cross(d).len() < 1e-9,
            Surface::Extrusion { dir, .. } => dir.cross(d).len() < 1e-9,
            _ => false,
        }
    }
    /// Whether the surface is one of revolution about the axis through `o` along `d`.
    pub fn revolved_about(&self, o: V3, d: V3) -> bool {
        let on_axis = |f: &Frame| {
            f.z.cross(d).len() < 1e-9 && {
                let w = f.origin - o;
                (w - d * w.dot(d)).len() < 1e-7
            }
        };
        match self {
            Surface::Plane { f } => f.z.cross(d).len() < 1e-9,
            Surface::Cylinder { f, .. }
            | Surface::Cone { f, .. }
            | Surface::Torus { f, .. }
            | Surface::Revolution { f, .. } => on_axis(f),
            Surface::Sphere { f, .. } => {
                let w = f.origin - o;
                (w - d * w.dot(d)).len() < 1e-7
            }
            _ => false,
        }
    }
}

/// The pipe's moving frame: `n` towards the surface the ball touches first, `b = t × n`,
/// and their derivatives along the spine.
fn pipe_frame(spine: &Curve, toward: &Surface, u: f64, c: V3, dc: V3) -> (V3, V3, V3, V3) {
    let g = toward.grad(c);
    let sign = if toward.sd(c) > 0.0 { -1.0 } else { 1.0 };
    let n0 = g * sign;
    let l = dc.len().max(1e-300);
    let t = dc / l;
    let n = (n0 - t * n0.dot(t)).norm();
    let b = t.cross(n);
    let h = toward.hess(c);
    let hc = v3(
        h[0][0] * dc.x + h[0][1] * dc.y + h[0][2] * dc.z,
        h[1][0] * dc.x + h[1][1] * dc.y + h[1][2] * dc.z,
        h[2][0] * dc.x + h[2][1] * dc.y + h[2][2] * dc.z,
    );
    let dn = hc * sign;
    let dd = spine.d2(u);
    let dt = (dd - t * dd.dot(t)) / l;
    let db = dt.cross(n) + t.cross(dn);
    (n, b, dn, db)
}
