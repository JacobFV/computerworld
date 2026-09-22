//! Numerical building blocks for the exact kernel: Gauss–Legendre and Gauss–Kronrod
//! quadrature, bracketed root finding and small dense solves. Everything is built from
//! correctly rounded operations (and the kernel's own trigonometry), so results are
//! bit-identical on every target.
use crate::math::{self, V3};
use std::sync::OnceLock;

/// Nodes and weights of the `n`-point Gauss–Legendre rule on [-1, 1], found by Newton's
/// method on the Legendre polynomial (deterministic: the same operations everywhere).
pub fn gauss_legendre(n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut x = vec![0.0; n];
    let mut w = vec![0.0; n];
    let m = n.div_ceil(2);
    for i in 0..m {
        // Tricomi's initial guess.
        let mut z = math::cos(math::PI * (i as f64 + 0.75) / (n as f64 + 0.5));
        let mut dp = 1.0;
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, z);
            for k in 2..=n {
                let p2 = ((2 * k - 1) as f64 * z * p1 - (k - 1) as f64 * p0) / k as f64;
                p0 = p1;
                p1 = p2;
            }
            let pn = if n == 1 { z } else { p1 };
            let pn1 = if n == 1 { 1.0 } else { p0 };
            dp = n as f64 * (z * pn - pn1) / (z * z - 1.0);
            let dz = pn / dp;
            z -= dz;
            if dz.abs() < 1e-16 {
                break;
            }
        }
        x[i] = -z;
        x[n - 1 - i] = z;
        let wi = 2.0 / ((1.0 - z * z) * dp * dp);
        w[i] = wi;
        w[n - 1 - i] = wi;
    }
    if n % 2 == 1 {
        x[n / 2] = 0.0;
    }
    (x, w)
}

/// The 16-point rule, computed once.
pub fn gl16() -> &'static (Vec<f64>, Vec<f64>) {
    static R: OnceLock<(Vec<f64>, Vec<f64>)> = OnceLock::new();
    R.get_or_init(|| gauss_legendre(16))
}

/// ∫ₐᵇ f by composite 16-point Gauss–Legendre over `pieces` equal parts.
pub fn integrate_gl<const K: usize>(
    a: f64,
    b: f64,
    pieces: usize,
    mut f: impl FnMut(f64) -> [f64; K],
) -> [f64; K] {
    let (x, w) = gl16();
    let mut acc = [0.0; K];
    let pieces = pieces.max(1);
    let h = (b - a) / pieces as f64;
    for p in 0..pieces {
        let lo = a + h * p as f64;
        let mid = lo + h / 2.0;
        for (xi, wi) in x.iter().zip(w) {
            let v = f(mid + xi * h / 2.0);
            for k in 0..K {
                acc[k] += v[k] * wi * h / 2.0;
            }
        }
    }
    acc
}

// Gauss–Kronrod 7–15 nodes and weights (QUADPACK's qk15).
const XGK: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_4,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_5,
    0.0,
];
const WGK: [f64; 8] = [
    0.022_935_322_010_529_22,
    0.063_092_092_629_978_55,
    0.104_790_010_322_250_2,
    0.140_653_259_715_525_9,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_4,
    0.204_432_940_075_298_9,
    0.209_482_141_084_727_8,
];
const WG: [f64; 4] = [
    0.129_484_966_168_869_7,
    0.279_705_391_489_276_7,
    0.381_830_050_505_118_9,
    0.417_959_183_673_469_4,
];

fn gk15<const K: usize>(a: f64, b: f64, f: &mut impl FnMut(f64) -> [f64; K]) -> ([f64; K], f64) {
    let c = (a + b) / 2.0;
    let h = (b - a) / 2.0;
    let mut k = [0.0; K];
    let mut g = [0.0; K];
    let fc = f(c);
    for i in 0..K {
        k[i] = fc[i] * WGK[7];
        g[i] = fc[i] * WG[3];
    }
    for j in 0..7 {
        let dx = h * XGK[j];
        let f1 = f(c - dx);
        let f2 = f(c + dx);
        for i in 0..K {
            k[i] += (f1[i] + f2[i]) * WGK[j];
            if j % 2 == 1 {
                g[i] += (f1[i] + f2[i]) * WG[j / 2];
            }
        }
    }
    let mut err: f64 = 0.0;
    for i in 0..K {
        k[i] *= h;
        g[i] *= h;
        err = err.max((k[i] - g[i]).abs());
    }
    (k, err)
}

/// ∫ₐᵇ f, adaptively bisecting until each piece's Gauss–Kronrod error estimate is below
/// its share of `tol` (absolute), at most `depth` levels deep. Iterative, in a fixed order.
pub fn integrate_adaptive<const K: usize>(
    a: f64,
    b: f64,
    tol: f64,
    depth: u32,
    mut f: impl FnMut(f64) -> [f64; K],
) -> [f64; K] {
    let mut acc = [0.0; K];
    if a == b {
        return acc;
    }
    let total = (b - a).abs();
    let mut stack: Vec<(f64, f64, u32)> = vec![(a, b, 0)];
    while let Some((lo, hi, d)) = stack.pop() {
        let (v, err) = gk15(lo, hi, &mut f);
        let share = tol * ((hi - lo).abs() / total).max(1e-6);
        if err <= share || d >= depth {
            for k in 0..K {
                acc[k] += v[k];
            }
        } else {
            let mid = (lo + hi) / 2.0;
            stack.push((mid, hi, d + 1));
            stack.push((lo, mid, d + 1));
        }
    }
    acc
}

/// A root of `f` in [a, b] where `f(a)` and `f(b)` differ in sign (Brent's method).
pub fn brent(mut f: impl FnMut(f64) -> f64, a: f64, b: f64, fa: f64, fb: f64, tol: f64) -> f64 {
    let (mut a, mut b, mut fa, mut fb) = (a, b, fa, fb);
    if fa == 0.0 {
        return a;
    }
    if fb == 0.0 {
        return b;
    }
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }
    let (mut c, mut fc) = (a, fa);
    let mut mflag = true;
    let mut d = 0.0;
    for _ in 0..200 {
        if fb == 0.0 || (b - a).abs() <= tol {
            return b;
        }
        let mut s = if fa != fc && fb != fc {
            a * fb * fc / ((fa - fb) * (fa - fc))
                + b * fa * fc / ((fb - fa) * (fb - fc))
                + c * fa * fb / ((fc - fa) * (fc - fb))
        } else {
            b - fb * (b - a) / (fb - fa)
        };
        let lo = (3.0 * a + b) / 4.0;
        let between = if lo < b {
            s > lo && s < b
        } else {
            s > b && s < lo
        };
        if !between
            || (mflag && (s - b).abs() >= (b - c).abs() / 2.0)
            || (!mflag && (s - b).abs() >= (c - d).abs() / 2.0)
            || (mflag && (b - c).abs() < tol)
            || (!mflag && (c - d).abs() < tol)
        {
            s = (a + b) / 2.0;
            mflag = true;
        } else {
            mflag = false;
        }
        let fs = f(s);
        d = c;
        c = b;
        fc = fb;
        if fa * fs < 0.0 {
            b = s;
            fb = fs;
        } else {
            a = s;
            fa = fs;
        }
        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
        }
    }
    b
}

/// Local minimum of `f` on [a, b] by golden-section search.
pub fn golden_min(mut f: impl FnMut(f64) -> f64, a: f64, b: f64, iters: usize) -> f64 {
    let g = (math::sqrt(5.0) - 1.0) / 2.0;
    let (mut a, mut b) = (a, b);
    let mut c = b - g * (b - a);
    let mut d = a + g * (b - a);
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..iters {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - g * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + g * (b - a);
            fd = f(d);
        }
    }
    (a + b) / 2.0
}

/// Roots of `f` on [a, b]: sign changes between `samples` equal steps refined by Brent,
/// plus touching roots (a sampled local minimum of |f| that Newton-like golden search
/// drives below `ftol`). Sorted, de-duplicated within `xtol`.
pub fn roots(
    mut f: impl FnMut(f64) -> f64,
    a: f64,
    b: f64,
    samples: usize,
    ftol: f64,
    xtol: f64,
) -> Vec<f64> {
    let n = samples.max(2);
    let xs: Vec<f64> = (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect();
    let fs: Vec<f64> = xs.iter().map(|x| f(*x)).collect();
    let mut out = Vec::new();
    for i in 0..=n {
        if fs[i].abs() <= ftol {
            out.push(xs[i]);
        }
    }
    for i in 0..n {
        if fs[i].abs() <= ftol || fs[i + 1].abs() <= ftol {
            continue;
        }
        if (fs[i] < 0.0) != (fs[i + 1] < 0.0) {
            out.push(brent(
                &mut f,
                xs[i],
                xs[i + 1],
                fs[i],
                fs[i + 1],
                xtol * 1e-3,
            ));
        }
    }
    // Touching roots: |f| has a local minimum between samples with no sign change.
    for i in 1..n {
        let (l, m, r) = (fs[i - 1].abs(), fs[i].abs(), fs[i + 1].abs());
        if m <= l
            && m <= r
            && (fs[i - 1] < 0.0) == (fs[i] < 0.0)
            && (fs[i] < 0.0) == (fs[i + 1] < 0.0)
        {
            let x = golden_min(|x| f(x).abs(), xs[i - 1], xs[i + 1], 80);
            if f(x).abs() <= ftol {
                out.push(x);
            }
        }
    }
    out.sort_by(|p, q| p.total_cmp(q));
    let mut dedup: Vec<f64> = Vec::new();
    for x in out {
        if dedup.last().is_none_or(|l| (x - l).abs() > xtol) {
            dedup.push(x);
        }
    }
    dedup
}

/// Like [`roots`], with the derivative: touching roots are found as roots of `df` (well
/// conditioned) rather than minima of |f| (a double root is only known to √ε).
pub fn roots_d(
    mut f: impl FnMut(f64) -> f64,
    mut df: impl FnMut(f64) -> f64,
    a: f64,
    b: f64,
    samples: usize,
    ftol: f64,
    xtol: f64,
) -> Vec<f64> {
    let n = samples.max(2);
    let xs: Vec<f64> = (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect();
    let fs: Vec<f64> = xs.iter().map(|x| f(*x)).collect();
    let mut out = Vec::new();
    for i in 0..=n {
        if fs[i] == 0.0 {
            out.push(xs[i]);
        }
    }
    for i in 0..n {
        if fs[i] == 0.0 || fs[i + 1] == 0.0 {
            continue;
        }
        if (fs[i] < 0.0) != (fs[i + 1] < 0.0) {
            out.push(brent(
                &mut f,
                xs[i],
                xs[i + 1],
                fs[i],
                fs[i + 1],
                xtol * 1e-3,
            ));
        }
    }
    // Extrema of f that come close to zero without crossing.
    let ds: Vec<f64> = xs.iter().map(|x| df(*x)).collect();
    for i in 0..n {
        if (ds[i] < 0.0) != (ds[i + 1] < 0.0) {
            let x = brent(&mut df, xs[i], xs[i + 1], ds[i], ds[i + 1], xtol * 1e-3);
            let v = f(x);
            let near_crossing = out.iter().any(|r| (r - x).abs() < (b - a).abs() / n as f64);
            if v.abs() <= ftol && !near_crossing {
                out.push(x);
            }
        }
    }
    out.sort_by(|p, q| p.total_cmp(q));
    let mut dedup: Vec<f64> = Vec::new();
    for x in out {
        if dedup.last().is_none_or(|l| (x - l).abs() > xtol) {
            dedup.push(x);
        }
    }
    dedup
}

/// Solve a 3×3 system (rows) by Gaussian elimination with partial pivoting.
pub fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let mut a = [
        [m[0][0], m[0][1], m[0][2], b[0]],
        [m[1][0], m[1][1], m[1][2], b[1]],
        [m[2][0], m[2][1], m[2][2], b[2]],
    ];
    for col in 0..3 {
        let piv = (col..3)
            .max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()).then(j.cmp(&i)))
            .unwrap();
        if a[piv][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, piv);
        for r in 0..3 {
            if r != col {
                let k = a[r][col] / a[col][col];
                let pivot = a[col];
                for (c, cell) in a[r].iter_mut().enumerate().skip(col) {
                    *cell -= k * pivot[c];
                }
            }
        }
    }
    Some([a[0][3] / a[0][0], a[1][3] / a[1][1], a[2][3] / a[2][2]])
}
pub fn solve3v(rows: [V3; 3], b: [f64; 3]) -> Option<V3> {
    let m = rows.map(|r| [r.x, r.y, r.z]);
    solve3(m, b).map(|x| V3 {
        x: x[0],
        y: x[1],
        z: x[2],
    })
}

/// Solve [a b; c d] x = r.
pub fn solve2(a: f64, b: f64, c: f64, d: f64, r0: f64, r1: f64) -> Option<(f64, f64)> {
    let det = a * d - b * c;
    let scale = (a.abs() + b.abs()) * (c.abs() + d.abs());
    if det.abs() <= 1e-300 || det.abs() <= scale * 1e-18 {
        return None;
    }
    Some(((r0 * d - b * r1) / det, (a * r1 - r0 * c) / det))
}

/// Real roots of a x² + b x + c = 0, ascending (a double root once).
pub fn quadratic(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() < 1e-300 {
        if b.abs() < 1e-300 {
            return vec![];
        }
        return vec![-c / b];
    }
    let disc = b * b - 4.0 * a * c;
    let scale = b * b + (4.0 * a * c).abs();
    if disc < -1e-14 * scale {
        return vec![];
    }
    if disc <= 1e-14 * scale {
        return vec![-b / (2.0 * a)];
    }
    let s = math::sqrt(disc);
    let q = -0.5 * (b + if b >= 0.0 { s } else { -s });
    let (x1, x2) = (q / a, c / q);
    if x1 < x2 {
        vec![x1, x2]
    } else {
        vec![x2, x1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quadrature_is_exact_on_trig_and_polynomials() {
        let (x, w) = gauss_legendre(16);
        let s: f64 = w.iter().sum();
        assert!((s - 2.0).abs() < 1e-14);
        let p: f64 = x.iter().zip(&w).map(|(x, w)| x.powi(10) * w).sum();
        assert!((p - 2.0 / 11.0).abs() < 1e-15);
        let [v] = integrate_gl(0.0, math::PI, 2, |t| [math::sin(t)]);
        assert!((v - 2.0).abs() < 1e-14);
        let [v] = integrate_adaptive(0.0, 1.0, 1e-14, 30, |t| [math::sqrt(t)]);
        assert!((v - 2.0 / 3.0).abs() < 1e-12, "{v}");
        let r = roots(|x| (x - 0.3) * (x - 0.7), 0.0, 1.0, 16, 1e-14, 1e-12);
        assert_eq!(r.len(), 2);
        assert!((r[0] - 0.3).abs() < 1e-12 && (r[1] - 0.7).abs() < 1e-12);
        let t = roots(|x| (x - 0.4) * (x - 0.4), 0.0, 1.0, 16, 1e-14, 1e-6);
        assert_eq!(t.len(), 1, "{t:?}");
    }
}
