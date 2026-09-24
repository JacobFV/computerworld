//! SVG geometry: affine transforms, the `transform` attribute, path data, and the
//! flattening of lines, curves and arcs into polylines.
//!
//! Everything is `f64` with only `+ - * /` and `sqrt`, which IEEE 754 fixes to the
//! bit, so a native and a Wasm build draw the same pixels; `sin`, `cos` and `atan2`
//! are the polynomial versions below rather than the platform's libm.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub const fn new(x: f64, y: f64) -> Pt {
        Pt { x, y }
    }
}

/// `[a c e; b d f]`: x' = a x + c y + e, y' = b x + d y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Affine {
    fn default() -> Self {
        Affine::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn translate(x: f64, y: f64) -> Affine {
        Affine {
            e: x,
            f: y,
            ..Affine::IDENTITY
        }
    }

    pub fn scale(sx: f64, sy: f64) -> Affine {
        Affine {
            a: sx,
            d: sy,
            ..Affine::IDENTITY
        }
    }

    pub fn rotate_deg(deg: f64) -> Affine {
        let (s, c) = sin_cos(deg * PI / 180.0);
        Affine {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        }
    }

    /// `self ∘ other`: apply `other` first, then `self`.
    pub fn then(&self, other: &Affine) -> Affine {
        let m = self;
        let n = other;
        Affine {
            a: m.a * n.a + m.c * n.b,
            b: m.b * n.a + m.d * n.b,
            c: m.a * n.c + m.c * n.d,
            d: m.b * n.c + m.d * n.d,
            e: m.a * n.e + m.c * n.f + m.e,
            f: m.b * n.e + m.d * n.f + m.f,
        }
    }

    pub fn apply(&self, p: Pt) -> Pt {
        Pt::new(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }

    pub fn invert(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        let a = self.d / det;
        let b = -self.b / det;
        let c = -self.c / det;
        let d = self.a / det;
        Some(Affine {
            a,
            b,
            c,
            d,
            e: -(a * self.e + c * self.f),
            f: -(b * self.e + d * self.f),
        })
    }

    /// The factor lengths scale by (the square root of the determinant), for stroke
    /// widths and dash lengths under a non-uniform scale.
    pub fn mean_scale(&self) -> f64 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }
}

/// `sin` and `cos` from a reduction to `[-π/4, π/4]` and Taylor series to the
/// 17th/16th power (error below 1e-16 there).
pub fn sin_cos(x: f64) -> (f64, f64) {
    if !x.is_finite() {
        return (0.0, 1.0);
    }
    // x = k·(π/2) + r.
    let k = (x / FRAC_PI_2 + 0.5).floor();
    let r = x - k * FRAC_PI_2;
    let r2 = r * r;
    let mut s = 0.0;
    let mut c = 0.0;
    let mut term_s = r;
    let mut term_c = 1.0;
    for i in 0..9 {
        s += term_s;
        c += term_c;
        let n = (2 * i + 2) as f64;
        term_s *= -r2 / (n * (n + 1.0));
        term_c *= -r2 / ((n - 1.0) * n);
    }
    match (k as i64).rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

/// `atan2(y, x)` in `(-π, π]`, from an argument reduction to `|t| ≤ tan(π/8)` and
/// the arctangent series.
pub fn atan2(y: f64, x: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let (ax, ay) = (x.abs(), y.abs());
    // Angle of (ax, ay) in [0, π/2].
    let (t, base, flip) = if ay <= ax {
        (ay / ax, 0.0, false)
    } else {
        (ax / ay, FRAC_PI_2, true)
    };
    // atan(t) for t in [0, 1]: reduce with atan(t) = π/4 + atan((t-1)/(t+1)).
    let (u, off) = if t > 0.41421356237309503 {
        ((t - 1.0) / (t + 1.0), FRAC_PI_4)
    } else {
        (t, 0.0)
    };
    let u2 = u * u;
    let mut sum = 0.0;
    let mut term = u;
    for i in 0..24 {
        sum += term / (2 * i + 1) as f64;
        term *= -u2;
    }
    let a = off + sum;
    let a = if flip { base - a } else { a };
    match (x < 0.0, y < 0.0) {
        (false, false) => a,
        (true, false) => PI - a,
        (true, true) => -(PI - a),
        (false, true) => -a,
    }
}

/// A number in SVG attribute syntax at the start of `s`, and the rest.
pub fn number(s: &str) -> Option<(f64, &str)> {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == digits_start || (i == digits_start + 1 && b[digits_start] == b'.') {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    s[..i].parse().ok().map(|v| (v, &s[i..]))
}

fn skip_sep(s: &str) -> &str {
    let s = s.trim_start();
    let s = s.strip_prefix(',').unwrap_or(s);
    s.trim_start()
}

/// Every number in a list (`points`, `viewBox`, `stroke-dasharray`), separated by
/// whitespace and/or commas.
pub fn numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut rest = skip_sep(s);
    while let Some((v, r)) = number(rest) {
        out.push(v);
        rest = skip_sep(r);
    }
    out
}

/// A length attribute (`12`, `12px`, `50%`) in user units; a percentage resolves
/// against `base`.
pub fn length(s: &str, base: f64) -> Option<f64> {
    let s = s.trim();
    let (v, rest) = number(s)?;
    match rest.trim() {
        "" | "px" => Some(v),
        "%" => Some(v * base / 100.0),
        "em" => Some(v * 16.0),
        "pt" => Some(v * 4.0 / 3.0),
        _ => None,
    }
}

/// The `transform` attribute.
pub fn parse_transform(s: &str) -> Affine {
    let mut m = Affine::IDENTITY;
    let mut rest = s.trim();
    while !rest.is_empty() {
        let Some(open) = rest.find('(') else { break };
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args = numbers(&rest[open + 1..open + close]);
        rest = rest[open + close + 1..].trim_start();
        rest = rest.strip_prefix(',').unwrap_or(rest).trim_start();
        let t = match (name, args.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => Affine {
                a: *a,
                b: *b,
                c: *c,
                d: *d,
                e: *e,
                f: *f,
            },
            ("translate", [x]) => Affine::translate(*x, 0.0),
            ("translate", [x, y]) => Affine::translate(*x, *y),
            ("scale", [s]) => Affine::scale(*s, *s),
            ("scale", [x, y]) => Affine::scale(*x, *y),
            ("rotate", [a]) => Affine::rotate_deg(*a),
            ("rotate", [a, cx, cy]) => Affine::translate(*cx, *cy)
                .then(&Affine::rotate_deg(*a))
                .then(&Affine::translate(-cx, -cy)),
            ("skewX", [a]) => {
                let (s, c) = sin_cos(a * PI / 180.0);
                Affine {
                    c: s / c,
                    ..Affine::IDENTITY
                }
            }
            ("skewY", [a]) => {
                let (s, c) = sin_cos(a * PI / 180.0);
                Affine {
                    b: s / c,
                    ..Affine::IDENTITY
                }
            }
            _ => return m,
        };
        m = m.then(&t);
    }
    m
}

/// One subpath, flattened: points in order and whether it was closed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Poly {
    pub pts: Vec<Pt>,
    pub closed: bool,
}

/// A path in user space: its subpaths as curves are met, flattened after mapping
/// through `m` so the tolerance is in device pixels.
pub struct Flattener {
    m: Affine,
    out: Vec<Poly>,
    cur: Poly,
    /// `cur` holds only the start point a `Z` left behind.
    pending_start: bool,
}

/// Device-pixel flatness: curves are split until each chord is within this.
const TOLERANCE: f64 = 0.1;

impl Flattener {
    pub fn new(m: Affine) -> Flattener {
        Flattener {
            m,
            out: Vec::new(),
            cur: Poly::default(),
            pending_start: false,
        }
    }

    pub fn move_to(&mut self, p: Pt) {
        self.flush();
        self.cur.pts.push(self.m.apply(p));
    }

    pub fn line_to(&mut self, p: Pt) {
        self.pending_start = false;
        self.cur.pts.push(self.m.apply(p));
    }

    pub fn cubic_to(&mut self, p0: Pt, c1: Pt, c2: Pt, p3: Pt) {
        self.pending_start = false;
        let (a, b, c, d) = (
            self.m.apply(p0),
            self.m.apply(c1),
            self.m.apply(c2),
            self.m.apply(p3),
        );
        let dd = |p: Pt, q: Pt| ((p.x - q.x) * (p.x - q.x) + (p.y - q.y) * (p.y - q.y)).sqrt();
        let len = dd(a, b) + dd(b, c) + dd(c, d);
        let n = ((len / (8.0 * TOLERANCE)).sqrt().ceil() as usize).clamp(1, 100);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.cur.pts.push(Pt::new(
                w0 * a.x + w1 * b.x + w2 * c.x + w3 * d.x,
                w0 * a.y + w1 * b.y + w2 * c.y + w3 * d.y,
            ));
        }
    }

    pub fn quad_to(&mut self, p0: Pt, c: Pt, p2: Pt) {
        let c1 = Pt::new(
            p0.x + 2.0 / 3.0 * (c.x - p0.x),
            p0.y + 2.0 / 3.0 * (c.y - p0.y),
        );
        let c2 = Pt::new(
            p2.x + 2.0 / 3.0 * (c.x - p2.x),
            p2.y + 2.0 / 3.0 * (c.y - p2.y),
        );
        self.cubic_to(p0, c1, c2, p2);
    }

    /// An elliptical arc in endpoint form (SVG 1.1 F.6.5).
    #[allow(clippy::too_many_arguments)]
    pub fn arc_to(
        &mut self,
        p0: Pt,
        rx: f64,
        ry: f64,
        rot_deg: f64,
        large: bool,
        sweep: bool,
        p1: Pt,
    ) {
        let (mut rx, mut ry) = (rx.abs(), ry.abs());
        if rx == 0.0 || ry == 0.0 || (p0.x == p1.x && p0.y == p1.y) {
            self.line_to(p1);
            return;
        }
        let (sphi, cphi) = sin_cos(rot_deg * PI / 180.0);
        let dx2 = (p0.x - p1.x) / 2.0;
        let dy2 = (p0.y - p1.y) / 2.0;
        let x1p = cphi * dx2 + sphi * dy2;
        let y1p = -sphi * dx2 + cphi * dy2;
        let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
        if lambda > 1.0 {
            let s = lambda.sqrt();
            rx *= s;
            ry *= s;
        }
        let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
        let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
        let mut coef = if den == 0.0 {
            0.0
        } else {
            (num / den).max(0.0).sqrt()
        };
        if large == sweep {
            coef = -coef;
        }
        let cxp = coef * rx * y1p / ry;
        let cyp = -coef * ry * x1p / rx;
        let cx = cphi * cxp - sphi * cyp + (p0.x + p1.x) / 2.0;
        let cy = sphi * cxp + cphi * cyp + (p0.y + p1.y) / 2.0;
        let ux = (x1p - cxp) / rx;
        let uy = (y1p - cyp) / ry;
        let vx = (-x1p - cxp) / rx;
        let vy = (-y1p - cyp) / ry;
        let theta1 = atan2(uy, ux);
        let mut dtheta = atan2(vy, vx) - theta1;
        if sweep && dtheta < 0.0 {
            dtheta += 2.0 * PI;
        } else if !sweep && dtheta > 0.0 {
            dtheta -= 2.0 * PI;
        }
        // Segments from the device-space radius.
        let r_dev = rx.max(ry) * self.m.mean_scale();
        let per = if r_dev > TOLERANCE {
            2.0 * (1.0 - TOLERANCE / r_dev).clamp(-1.0, 1.0).acos_det()
        } else {
            PI / 2.0
        };
        let n = ((dtheta.abs() / per.max(1e-3)).ceil() as usize).clamp(1, 200);
        self.pending_start = false;
        for i in 1..=n {
            let t = theta1 + dtheta * i as f64 / n as f64;
            let (st, ct) = sin_cos(t);
            let x = cphi * rx * ct - sphi * ry * st + cx;
            let y = sphi * rx * ct + cphi * ry * st + cy;
            let p = if i == n { p1 } else { Pt::new(x, y) };
            self.cur.pts.push(self.m.apply(p));
        }
    }

    pub fn close(&mut self) {
        if !self.cur.pts.is_empty() {
            self.cur.closed = true;
            let start = self.cur.pts[0];
            self.flush();
            // A command after `Z` starts from the subpath's start.
            self.cur.pts.push(start);
            self.cur.closed = false;
            self.pending_start = true;
        }
    }

    fn flush(&mut self) {
        if self.pending_start {
            // Only the implied start point after a `Z`: nothing drawn from it.
            self.pending_start = false;
            if self.cur.pts.len() <= 1 {
                self.cur = Poly::default();
                return;
            }
        }
        if !self.cur.pts.is_empty() {
            self.out.push(std::mem::take(&mut self.cur));
        }
    }

    pub fn finish(mut self) -> Vec<Poly> {
        self.flush();
        self.out
    }
}

trait AcosDet {
    fn acos_det(self) -> f64;
}

impl AcosDet for f64 {
    /// `acos` for `x` in `[-1, 1]`, as `atan2(√(1-x²), x)`.
    fn acos_det(self) -> f64 {
        atan2((1.0 - self * self).max(0.0).sqrt(), self)
    }
}

/// Parses path data into `f` (SVG 1.1 §8.3; an error ends the path where it is).
pub fn path_data(d: &str, f: &mut Flattener) {
    let mut rest = d.trim_start();
    let mut cmd = ' ';
    let mut cur = Pt::default();
    let mut start = Pt::default();
    let mut last_ctrl: Option<(char, Pt)> = None;
    loop {
        rest = skip_sep(rest);
        if rest.is_empty() {
            break;
        }
        let c = rest.as_bytes()[0] as char;
        if c.is_ascii_alphabetic() {
            cmd = c;
            rest = &rest[1..];
            if cmd == 'z' || cmd == 'Z' {
                f.close();
                cur = start;
                last_ctrl = None;
                continue;
            }
        } else if cmd == ' ' {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur } else { Pt::default() };
        macro_rules! nums {
            ($n:expr) => {{
                let mut v = [0.0f64; $n];
                for slot in v.iter_mut() {
                    rest = skip_sep(rest);
                    match number(rest) {
                        Some((x, r)) => {
                            *slot = x;
                            rest = r;
                        }
                        None => return,
                    }
                }
                v
            }};
        }
        match cmd.to_ascii_uppercase() {
            'M' => {
                let [x, y] = nums!(2);
                cur = Pt::new(base.x + x, base.y + y);
                start = cur;
                f.move_to(cur);
                // Further pairs are implicit line-tos.
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' => {
                let [x, y] = nums!(2);
                cur = Pt::new(base.x + x, base.y + y);
                f.line_to(cur);
                last_ctrl = None;
            }
            'H' => {
                let [x] = nums!(1);
                cur = Pt::new(if rel { cur.x + x } else { x }, cur.y);
                f.line_to(cur);
                last_ctrl = None;
            }
            'V' => {
                let [y] = nums!(1);
                cur = Pt::new(cur.x, if rel { cur.y + y } else { y });
                f.line_to(cur);
                last_ctrl = None;
            }
            'C' => {
                let [x1, y1, x2, y2, x, y] = nums!(6);
                let c1 = Pt::new(base.x + x1, base.y + y1);
                let c2 = Pt::new(base.x + x2, base.y + y2);
                let p = Pt::new(base.x + x, base.y + y);
                f.cubic_to(cur, c1, c2, p);
                last_ctrl = Some(('C', c2));
                cur = p;
            }
            'S' => {
                let [x2, y2, x, y] = nums!(4);
                let c1 = match last_ctrl {
                    Some(('C', c)) => Pt::new(2.0 * cur.x - c.x, 2.0 * cur.y - c.y),
                    _ => cur,
                };
                let c2 = Pt::new(base.x + x2, base.y + y2);
                let p = Pt::new(base.x + x, base.y + y);
                f.cubic_to(cur, c1, c2, p);
                last_ctrl = Some(('C', c2));
                cur = p;
            }
            'Q' => {
                let [x1, y1, x, y] = nums!(4);
                let c = Pt::new(base.x + x1, base.y + y1);
                let p = Pt::new(base.x + x, base.y + y);
                f.quad_to(cur, c, p);
                last_ctrl = Some(('Q', c));
                cur = p;
            }
            'T' => {
                let [x, y] = nums!(2);
                let c = match last_ctrl {
                    Some(('Q', c)) => Pt::new(2.0 * cur.x - c.x, 2.0 * cur.y - c.y),
                    _ => cur,
                };
                let p = Pt::new(base.x + x, base.y + y);
                f.quad_to(cur, c, p);
                last_ctrl = Some(('Q', c));
                cur = p;
            }
            'A' => {
                let [rx, ry, rot] = nums!(3);
                // The two flags may be written without separators (`a1 1 0 01 2 2`).
                let mut flag = || -> Option<bool> {
                    rest = skip_sep(rest);
                    let b = *rest.as_bytes().first()?;
                    rest = &rest[1..];
                    match b {
                        b'0' => Some(false),
                        b'1' => Some(true),
                        _ => None,
                    }
                };
                let (Some(large), Some(sweep)) = (flag(), flag()) else {
                    return;
                };
                let [x, y] = nums!(2);
                let p = Pt::new(base.x + x, base.y + y);
                f.arc_to(cur, rx, ry, rot, large, sweep, p);
                cur = p;
                last_ctrl = None;
            }
            _ => return,
        }
    }
}

/// The four cubic segments of an ellipse (the usual 0.5523 control distance),
/// starting at its rightmost point, clockwise in y-down coordinates.
pub fn ellipse(f: &mut Flattener, cx: f64, cy: f64, rx: f64, ry: f64) {
    const K: f64 = 0.552_284_749_830_793_4;
    let (kx, ky) = (rx * K, ry * K);
    let p = |x: f64, y: f64| Pt::new(cx + x, cy + y);
    f.move_to(p(rx, 0.0));
    f.cubic_to(p(rx, 0.0), p(rx, ky), p(kx, ry), p(0.0, ry));
    f.cubic_to(p(0.0, ry), p(-kx, ry), p(-rx, ky), p(-rx, 0.0));
    f.cubic_to(p(-rx, 0.0), p(-rx, -ky), p(-kx, -ry), p(0.0, -ry));
    f.cubic_to(p(0.0, -ry), p(kx, -ry), p(rx, -ky), p(rx, 0.0));
    f.close();
}

/// A rectangle with optional rounded corners (SVG 1.1 §9.2 radius rules).
pub fn rect(f: &mut Flattener, x: f64, y: f64, w: f64, h: f64, rx: f64, ry: f64) {
    let rx = rx.min(w / 2.0).max(0.0);
    let ry = ry.min(h / 2.0).max(0.0);
    if rx == 0.0 || ry == 0.0 {
        f.move_to(Pt::new(x, y));
        f.line_to(Pt::new(x + w, y));
        f.line_to(Pt::new(x + w, y + h));
        f.line_to(Pt::new(x, y + h));
        f.close();
        return;
    }
    const K: f64 = 0.552_284_749_830_793_4;
    let (kx, ky) = (rx * (1.0 - K), ry * (1.0 - K));
    let (r, b) = (x + w, y + h);
    f.move_to(Pt::new(x + rx, y));
    f.line_to(Pt::new(r - rx, y));
    f.cubic_to(
        Pt::new(r - rx, y),
        Pt::new(r - kx, y),
        Pt::new(r, y + ky),
        Pt::new(r, y + ry),
    );
    f.line_to(Pt::new(r, b - ry));
    f.cubic_to(
        Pt::new(r, b - ry),
        Pt::new(r, b - ky),
        Pt::new(r - kx, b),
        Pt::new(r - rx, b),
    );
    f.line_to(Pt::new(x + rx, b));
    f.cubic_to(
        Pt::new(x + rx, b),
        Pt::new(x + kx, b),
        Pt::new(x, b - ky),
        Pt::new(x, b - ry),
    );
    f.line_to(Pt::new(x, y + ry));
    f.cubic_to(
        Pt::new(x, y + ry),
        Pt::new(x, y + ky),
        Pt::new(x + kx, y),
        Pt::new(x + rx, y),
    );
    f.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trig_matches_the_platform_closely() {
        for i in -40..=40 {
            let x = i as f64 * 0.37;
            let (s, c) = sin_cos(x);
            assert!((s - x.sin()).abs() < 1e-12, "sin {x}");
            assert!((c - x.cos()).abs() < 1e-12, "cos {x}");
            for j in -5..=5 {
                let y = j as f64 * 0.9;
                assert!((atan2(y, x) - y.atan2(x)).abs() < 1e-12, "atan2 {y} {x}");
            }
        }
    }

    #[test]
    fn path_data_parses_every_command() {
        let mut f = Flattener::new(Affine::IDENTITY);
        path_data("M3 3v16a2 2 0 0 0 2 2h16M18 17V9m-5 8V5", &mut f);
        let polys = f.finish();
        assert_eq!(polys.len(), 3);
        let first = &polys[0];
        assert_eq!(first.pts[0], Pt::new(3.0, 3.0));
        assert_eq!(first.pts[1], Pt::new(3.0, 19.0));
        let end = *first.pts.last().unwrap();
        assert!((end.x - 21.0).abs() < 1e-9 && (end.y - 21.0).abs() < 1e-9);
        // The arc passes near the corner's quarter circle midpoint.
        let mid = first.pts[first.pts.len() / 2];
        assert!(mid.x > 3.0 && mid.x < 5.0 && mid.y > 19.0 && mid.y < 21.0);
        assert_eq!(polys[2].pts, vec![Pt::new(13.0, 17.0), Pt::new(13.0, 5.0)]);
    }

    #[test]
    fn transforms_compose_in_attribute_order() {
        let m = parse_transform("translate(10, 20) scale(2)");
        assert_eq!(m.apply(Pt::new(1.0, 1.0)), Pt::new(12.0, 22.0));
        let r = parse_transform("rotate(90 5 5)").apply(Pt::new(10.0, 5.0));
        assert!((r.x - 5.0).abs() < 1e-12 && (r.y - 10.0).abs() < 1e-12);
    }
}
