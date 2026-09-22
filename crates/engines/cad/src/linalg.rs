//! Small dense linear algebra for the sketch solver. Every routine visits rows and
//! columns in a fixed order and breaks pivot ties by index, so the same inputs give the
//! same bits on every platform.

/// Row-major dense matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct Mat {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}
impl Mat {
    pub fn zeros(rows: usize, cols: usize) -> Mat {
        Mat {
            rows,
            cols,
            data: vec![0.0; rows * cols],
        }
    }
    #[inline]
    pub fn at(&self, r: usize, c: usize) -> f64 {
        self.data[r * self.cols + c]
    }
    #[inline]
    pub fn set(&mut self, r: usize, c: usize, v: f64) {
        self.data[r * self.cols + c] = v;
    }
    #[inline]
    pub fn add(&mut self, r: usize, c: usize, v: f64) {
        self.data[r * self.cols + c] += v;
    }
    pub fn row(&self, r: usize) -> &[f64] {
        &self.data[r * self.cols..(r + 1) * self.cols]
    }
}

/// Solve the square system `a x = b` by Gaussian elimination with partial pivoting.
/// `None` when the matrix is singular to working precision.
pub fn solve(a: &Mat, b: &[f64]) -> Option<Vec<f64>> {
    let n = a.rows;
    debug_assert_eq!(a.cols, n);
    let mut m = a.data.clone();
    let mut x = b.to_vec();
    let scale = m.iter().fold(0.0f64, |s, v| s.max(v.abs())).max(1e-300);
    for col in 0..n {
        let mut pivot = col;
        let mut best = m[col * n + col].abs();
        for r in col + 1..n {
            let v = m[r * n + col].abs();
            if v > best {
                best = v;
                pivot = r;
            }
        }
        if best <= scale * 1e-14 {
            return None;
        }
        if pivot != col {
            for c in 0..n {
                m.swap(col * n + c, pivot * n + c);
            }
            x.swap(col, pivot);
        }
        let d = m[col * n + col];
        for r in col + 1..n {
            let f = m[r * n + col] / d;
            if f == 0.0 {
                continue;
            }
            for c in col..n {
                m[r * n + c] -= f * m[col * n + c];
            }
            x[r] -= f * x[col];
        }
    }
    for col in (0..n).rev() {
        let mut s = x[col];
        for c in col + 1..n {
            s -= m[col * n + c] * x[c];
        }
        x[col] = s / m[col * n + col];
    }
    Some(x)
}

/// Rank of `j` and a basis of its null space, by reduced row echelon form with partial
/// pivoting over columns in order. `tol` is relative to the largest entry.
pub fn null_space(j: &Mat, tol: f64) -> (usize, Vec<Vec<f64>>) {
    let (m, n) = (j.rows, j.cols);
    let mut a = j.data.clone();
    let scale = a.iter().fold(0.0f64, |s, v| s.max(v.abs())).max(1e-300);
    let eps = scale * tol;
    let mut pivots: Vec<usize> = Vec::new();
    let mut row = 0;
    for col in 0..n {
        if row >= m {
            break;
        }
        let mut pivot = row;
        let mut best = a[row * n + col].abs();
        for r in row + 1..m {
            let v = a[r * n + col].abs();
            if v > best {
                best = v;
                pivot = r;
            }
        }
        if best <= eps {
            // Clear the negligible remainder so it cannot pollute later columns.
            for r in row..m {
                a[r * n + col] = 0.0;
            }
            continue;
        }
        if pivot != row {
            for c in 0..n {
                a.swap(row * n + c, pivot * n + c);
            }
        }
        let d = a[row * n + col];
        for c in col..n {
            a[row * n + c] /= d;
        }
        for r in 0..m {
            if r == row {
                continue;
            }
            let f = a[r * n + col];
            if f == 0.0 {
                continue;
            }
            for c in col..n {
                a[r * n + c] -= f * a[row * n + c];
            }
        }
        pivots.push(col);
        row += 1;
    }
    let rank = pivots.len();
    let mut basis = Vec::new();
    for free in 0..n {
        if pivots.contains(&free) {
            continue;
        }
        let mut v = vec![0.0; n];
        v[free] = 1.0;
        for (r, &p) in pivots.iter().enumerate() {
            v[p] = -a[r * n + free];
        }
        basis.push(v);
    }
    (rank, basis)
}

/// Which rows of `j` depend on the rows before them. For every dependent row, the
/// earlier rows it is a combination of (including itself), found by Gram–Schmidt with
/// re-orthogonalisation over unit-normalised rows.
pub fn dependent_rows(j: &Mat, tol: f64) -> Vec<(usize, Vec<usize>)> {
    let (m, n) = (j.rows, j.cols);
    // Orthonormal basis vectors, and each one as a combination of original rows.
    let mut basis: Vec<Vec<f64>> = Vec::new();
    let mut combos: Vec<Vec<f64>> = Vec::new();
    let mut out = Vec::new();
    for i in 0..m {
        let row = j.row(i);
        let norm = row.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm == 0.0 {
            // An equation that constrains nothing at all: dependent on its own.
            out.push((i, vec![i]));
            continue;
        }
        let mut v: Vec<f64> = row.iter().map(|x| x / norm).collect();
        let mut combo = vec![0.0; m];
        combo[i] = 1.0 / norm;
        for _pass in 0..2 {
            for (q, cq) in basis.iter().zip(&combos) {
                let d: f64 = v.iter().zip(q).map(|(a, b)| a * b).sum();
                if d == 0.0 {
                    continue;
                }
                for k in 0..n {
                    v[k] -= d * q[k];
                }
                for k in 0..m {
                    combo[k] -= d * cq[k];
                }
            }
        }
        let rest = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if rest <= tol {
            // row_i/norm - Σ d q = ~0, so row_i is a combination of the rows in `combo`.
            let big = combo.iter().fold(0.0f64, |s, c| s.max(c.abs()));
            let members = (0..m)
                .filter(|&k| combo[k].abs() > big * 1e-6)
                .collect::<Vec<_>>();
            out.push((i, members));
            continue;
        }
        for x in &mut v {
            *x /= rest;
        }
        for c in &mut combo {
            *c /= rest;
        }
        basis.push(v);
        combos.push(combo);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mat(rows: usize, cols: usize, v: &[f64]) -> Mat {
        Mat {
            rows,
            cols,
            data: v.to_vec(),
        }
    }
    #[test]
    fn solves_and_detects_singularity() {
        let a = mat(3, 3, &[2.0, 1.0, -1.0, -3.0, -1.0, 2.0, -2.0, 1.0, 2.0]);
        let x = solve(&a, &[8.0, -11.0, -3.0]).unwrap();
        for (got, want) in x.iter().zip([2.0, 3.0, -1.0]) {
            assert!((got - want).abs() < 1e-12);
        }
        assert!(solve(&mat(2, 2, &[1.0, 2.0, 2.0, 4.0]), &[1.0, 2.0]).is_none());
    }
    #[test]
    fn rank_null_space_and_dependencies() {
        // Third row is first + second.
        let j = mat(
            3,
            4,
            &[1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        );
        let (rank, null) = null_space(&j, 1e-10);
        assert_eq!(rank, 2);
        assert_eq!(null.len(), 2);
        for v in &null {
            for r in 0..3 {
                let s: f64 = j.row(r).iter().zip(v).map(|(a, b)| a * b).sum();
                assert!(s.abs() < 1e-12);
            }
        }
        let deps = dependent_rows(&j, 1e-9);
        assert_eq!(deps, vec![(2, vec![0, 1, 2])]);
    }
}
