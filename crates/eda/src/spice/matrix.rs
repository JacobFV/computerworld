//! Dense LU factorisation with partial pivoting, real and complex. Circuits drawn in a
//! schematic have tens of unknowns, where a dense solver is both simplest and fastest;
//! pivoting keeps voltage-source rows (zero on the diagonal) solvable.

#[derive(Clone, Debug)]
pub struct Real {
    pub n: usize,
    pub a: Vec<f64>,
    pub b: Vec<f64>,
}
impl Real {
    pub fn new(n: usize) -> Self {
        Self {
            n,
            a: vec![0.0; n * n],
            b: vec![0.0; n],
        }
    }
    pub fn clear(&mut self) {
        self.a.iter_mut().for_each(|v| *v = 0.0);
        self.b.iter_mut().for_each(|v| *v = 0.0);
    }
    /// Add to entry (row, col); index 0 is ground and is dropped, so callers stamp
    /// with node numbers directly and unknown `k` lives at `k - 1`.
    pub fn add(&mut self, row: usize, col: usize, v: f64) {
        if row == 0 || col == 0 {
            return;
        }
        self.a[(row - 1) * self.n + (col - 1)] += v;
    }
    pub fn rhs(&mut self, row: usize, v: f64) {
        if row == 0 {
            return;
        }
        self.b[row - 1] += v;
    }
    /// Solve in place; returns the solution with a leading 0 for ground, or the index
    /// (1-based, as stamped) of the unknown the matrix cannot determine.
    pub fn solve(&mut self) -> Result<Vec<f64>, usize> {
        let n = self.n;
        let a = &mut self.a;
        let b = &mut self.b;
        let mut perm: Vec<usize> = (0..n).collect();
        let scale = a.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-300);
        for k in 0..n {
            let mut best = k;
            let mut big = a[perm[k] * n + k].abs();
            for (i, row) in perm.iter().enumerate().skip(k + 1) {
                let v = a[row * n + k].abs();
                if v > big {
                    big = v;
                    best = i;
                }
            }
            if big <= scale * 1e-18 {
                return Err(k + 1);
            }
            perm.swap(k, best);
            let pk = perm[k];
            let pivot = a[pk * n + k];
            for &pi in &perm[k + 1..] {
                let factor = a[pi * n + k] / pivot;
                if factor == 0.0 {
                    continue;
                }
                a[pi * n + k] = factor;
                for j in k + 1..n {
                    a[pi * n + j] -= factor * a[pk * n + j];
                }
                b[pi] -= factor * b[pk];
            }
        }
        let mut x = vec![0.0; n + 1];
        for k in (0..n).rev() {
            let pk = perm[k];
            let mut sum = b[pk];
            for j in k + 1..n {
                sum -= a[pk * n + j] * x[j + 1];
            }
            x[k + 1] = sum / a[pk * n + k];
        }
        Ok(x)
    }
}

/// Complex number as a plain pair; arithmetic written out so it stays deterministic.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct C64 {
    pub re: f64,
    pub im: f64,
}
#[allow(clippy::should_implement_trait)]
impl C64 {
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };
    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    pub fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
    pub fn sub(self, o: Self) -> Self {
        Self::new(self.re - o.re, self.im - o.im)
    }
    pub fn mul(self, o: Self) -> Self {
        Self::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
    pub fn div(self, o: Self) -> Self {
        // Smith's algorithm: no overflow for large components.
        if o.re.abs() >= o.im.abs() {
            let r = o.im / o.re;
            let d = o.re + o.im * r;
            Self::new((self.re + self.im * r) / d, (self.im - self.re * r) / d)
        } else {
            let r = o.re / o.im;
            let d = o.re * r + o.im;
            Self::new((self.re * r + self.im) / d, (self.im * r - self.re) / d)
        }
    }
    pub fn abs(self) -> f64 {
        let (a, b) = (self.re.abs(), self.im.abs());
        if a == 0.0 && b == 0.0 {
            return 0.0;
        }
        let (big, small) = if a > b { (a, b) } else { (b, a) };
        let r = small / big;
        big * (1.0 + r * r).sqrt()
    }
}

#[derive(Clone, Debug)]
pub struct Complex {
    pub n: usize,
    pub a: Vec<C64>,
    pub b: Vec<C64>,
}
impl Complex {
    pub fn new(n: usize) -> Self {
        Self {
            n,
            a: vec![C64::ZERO; n * n],
            b: vec![C64::ZERO; n],
        }
    }
    pub fn add(&mut self, row: usize, col: usize, v: C64) {
        if row == 0 || col == 0 {
            return;
        }
        let e = &mut self.a[(row - 1) * self.n + (col - 1)];
        *e = e.add(v);
    }
    pub fn rhs(&mut self, row: usize, v: C64) {
        if row == 0 {
            return;
        }
        self.b[row - 1] = self.b[row - 1].add(v);
    }
    pub fn solve(&mut self) -> Result<Vec<C64>, usize> {
        let n = self.n;
        let a = &mut self.a;
        let b = &mut self.b;
        let mut perm: Vec<usize> = (0..n).collect();
        let scale = a.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-300);
        for k in 0..n {
            let mut best = k;
            let mut big = a[perm[k] * n + k].abs();
            for (i, row) in perm.iter().enumerate().skip(k + 1) {
                let v = a[row * n + k].abs();
                if v > big {
                    big = v;
                    best = i;
                }
            }
            if big <= scale * 1e-18 {
                return Err(k + 1);
            }
            perm.swap(k, best);
            let pk = perm[k];
            let pivot = a[pk * n + k];
            for &pi in &perm[k + 1..] {
                let factor = a[pi * n + k].div(pivot);
                if factor == C64::ZERO {
                    continue;
                }
                a[pi * n + k] = factor;
                for j in k + 1..n {
                    a[pi * n + j] = a[pi * n + j].sub(factor.mul(a[pk * n + j]));
                }
                b[pi] = b[pi].sub(factor.mul(b[pk]));
            }
        }
        let mut x = vec![C64::ZERO; n + 1];
        for k in (0..n).rev() {
            let pk = perm[k];
            let mut sum = b[pk];
            for j in k + 1..n {
                sum = sum.sub(a[pk * n + j].mul(x[j + 1]));
            }
            x[k + 1] = sum.div(a[pk * n + k]);
        }
        Ok(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pivoting_solves_a_zero_diagonal_system() {
        // [0 1; 1 0] x = [2; 3] needs a row swap.
        let mut m = Real::new(2);
        m.add(1, 2, 1.0);
        m.add(2, 1, 1.0);
        m.rhs(1, 2.0);
        m.rhs(2, 3.0);
        assert_eq!(m.solve().unwrap(), vec![0.0, 3.0, 2.0]);
        let mut s = Real::new(2);
        s.add(1, 1, 1.0);
        s.add(1, 2, 1.0);
        s.add(2, 1, 1.0);
        s.add(2, 2, 1.0);
        assert!(s.solve().is_err(), "a singular matrix must be refused");
    }
    #[test]
    fn complex_solution_matches_hand_computation() {
        // (1 + j) x = 2 → x = 1 - j
        let mut m = Complex::new(1);
        m.add(1, 1, C64::new(1.0, 1.0));
        m.rhs(1, C64::new(2.0, 0.0));
        let x = m.solve().unwrap()[1];
        assert!((x.re - 1.0).abs() < 1e-15 && (x.im + 1.0).abs() < 1e-15);
    }
}
