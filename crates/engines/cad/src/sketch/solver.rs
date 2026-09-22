//! The sketch solver.
//!
//! Every constraint contributes residual equations `f(x) = 0` over the geometry's
//! parameters `x`. Residuals are written once, over forward-mode dual numbers, so the
//! Jacobian is exact rather than finite-differenced. The system is solved by
//! Levenberg–Marquardt with minimum-norm steps — among all corrections that satisfy the
//! linearised constraints it takes the smallest, so an under-constrained sketch moves as
//! little as possible, and a dragged point (weighted heavily) stays under the pointer
//! while the rest of the sketch follows.
//!
//! After solving, the Jacobian's rank gives the remaining degrees of freedom (FreeCAD's
//! "Under constrained: n DoF"), its null space which parameters are still free (so fully
//! constrained geometry can be drawn green), and Gram–Schmidt over its rows which
//! equations depend on earlier ones: redundant when the sketch still solves, conflicting
//! ("Over-constrained") when it does not.
use super::{Constraint, ConstraintType as T, Geom, Pos, Sketch, GEO_UNDEF, H_AXIS, V_AXIS};
use crate::linalg::{dependent_rows, null_space, solve as lin_solve, Mat};
use crate::math::{self, PI, TAU, V2};
use serde::{Deserialize, Serialize};
use std::ops::{Add, Div, Mul, Neg, Sub};

/// Local parameters one constraint may touch: three arcs' worth, plus slack.
const N: usize = 16;

#[derive(Clone, Copy, Debug)]
pub(crate) struct D {
    v: f64,
    g: [f64; N],
}
impl D {
    fn c(v: f64) -> D {
        D { v, g: [0.0; N] }
    }
    fn var(v: f64, slot: usize) -> D {
        let mut g = [0.0; N];
        g[slot] = 1.0;
        D { v, g }
    }
    fn map(self, v: f64, dv: f64) -> D {
        let mut g = self.g;
        for x in &mut g {
            *x *= dv;
        }
        D { v, g }
    }
    fn sqrt(self) -> D {
        let s = math::sqrt(self.v);
        self.map(s, if s > 0.0 { 0.5 / s } else { 0.0 })
    }
    fn sin(self) -> D {
        let (s, c) = math::sin_cos(self.v);
        self.map(s, c)
    }
    fn cos(self) -> D {
        let (s, c) = math::sin_cos(self.v);
        self.map(c, -s)
    }
    fn abs(self) -> D {
        if self.v < 0.0 {
            -self
        } else {
            self
        }
    }
    fn atan2(y: D, x: D) -> D {
        let v = math::atan2(y.v, x.v);
        let d = x.v * x.v + y.v * y.v;
        let mut g = [0.0; N];
        if d > 0.0 {
            for (i, gi) in g.iter_mut().enumerate() {
                *gi = (x.v * y.g[i] - y.v * x.g[i]) / d;
            }
        }
        D { v, g }
    }
    /// Shift by a whole number of turns so the value lies in `(-π, π]`. The derivative
    /// is unchanged: wrapping only moves the branch.
    fn wrap(self) -> D {
        D {
            v: math::wrap_angle(self.v),
            g: self.g,
        }
    }
}
impl Add for D {
    type Output = D;
    fn add(self, o: D) -> D {
        let mut g = self.g;
        for (a, b) in g.iter_mut().zip(o.g) {
            *a += b;
        }
        D { v: self.v + o.v, g }
    }
}
impl Sub for D {
    type Output = D;
    fn sub(self, o: D) -> D {
        let mut g = self.g;
        for (a, b) in g.iter_mut().zip(o.g) {
            *a -= b;
        }
        D { v: self.v - o.v, g }
    }
}
impl Mul for D {
    type Output = D;
    fn mul(self, o: D) -> D {
        let mut g = [0.0; N];
        for (i, gi) in g.iter_mut().enumerate() {
            *gi = self.g[i] * o.v + self.v * o.g[i];
        }
        D { v: self.v * o.v, g }
    }
}
impl Div for D {
    type Output = D;
    fn div(self, o: D) -> D {
        let mut g = [0.0; N];
        let d = o.v * o.v;
        for (i, gi) in g.iter_mut().enumerate() {
            *gi = (self.g[i] * o.v - self.v * o.g[i]) / d;
        }
        D { v: self.v / o.v, g }
    }
}
impl Neg for D {
    type Output = D;
    fn neg(self) -> D {
        let mut g = self.g;
        for x in &mut g {
            *x = -*x;
        }
        D { v: -self.v, g }
    }
}
impl Mul<f64> for D {
    type Output = D;
    fn mul(self, s: f64) -> D {
        self.map(self.v * s, s)
    }
}
impl Sub<f64> for D {
    type Output = D;
    fn sub(self, s: f64) -> D {
        D {
            v: self.v - s,
            g: self.g,
        }
    }
}
impl Add<f64> for D {
    type Output = D;
    fn add(self, s: f64) -> D {
        D {
            v: self.v + s,
            g: self.g,
        }
    }
}
#[derive(Clone, Copy)]
struct P {
    x: D,
    y: D,
}
impl P {
    fn sub(self, o: P) -> P {
        P {
            x: self.x - o.x,
            y: self.y - o.y,
        }
    }
    fn add(self, o: P) -> P {
        P {
            x: self.x + o.x,
            y: self.y + o.y,
        }
    }
    fn scale(self, s: f64) -> P {
        P {
            x: self.x * s,
            y: self.y * s,
        }
    }
    fn dot(self, o: P) -> D {
        self.x * o.x + self.y * o.y
    }
    fn cross(self, o: P) -> D {
        self.x * o.y - self.y * o.x
    }
    fn len(self) -> D {
        self.dot(self).sqrt()
    }
}

/// One piece of geometry as duals: either variables of this solve or constants.
#[derive(Clone, Copy)]
enum G {
    Point(P),
    Line(P, P),
    Circle(P, D),
    Arc(P, D, D, D),
}

/// The layout of solver variables over the sketch's geometry.
struct Layout {
    /// First variable index of each geometry.
    offset: Vec<usize>,
    n: usize,
}
impl Layout {
    fn new(s: &Sketch) -> Layout {
        let mut offset = Vec::with_capacity(s.geos.len());
        let mut n = 0;
        for g in &s.geos {
            offset.push(n);
            n += g.geom.param_count();
        }
        Layout { offset, n }
    }
}

/// Gathers one constraint's geometry as duals over local slots, remembering which
/// global variable each slot stands for.
struct Local<'a> {
    s: &'a Sketch,
    x: &'a [f64],
    layout: &'a Layout,
    slots: Vec<usize>,
}
impl Local<'_> {
    fn geo(&mut self, id: i32) -> Result<G, String> {
        let axis = |b: V2| {
            G::Line(
                P {
                    x: D::c(0.0),
                    y: D::c(0.0),
                },
                P {
                    x: D::c(b.x),
                    y: D::c(b.y),
                },
            )
        };
        match id {
            H_AXIS => return Ok(axis(math::v2(1.0, 0.0))),
            V_AXIS => return Ok(axis(math::v2(0.0, 1.0))),
            _ => {}
        }
        let geo = self.s.geo(id).ok_or("constraint names missing geometry")?;
        let base = self.layout.offset[id as usize];
        let count = geo.geom.param_count();
        // The same geometry twice in one constraint shares its slots.
        let first = match self.slots.iter().position(|s| *s == base) {
            Some(at) => at,
            None => {
                if self.slots.len() + count > N {
                    return Err("constraint touches too much geometry".into());
                }
                let at = self.slots.len();
                self.slots.extend(base..base + count);
                at
            }
        };
        let v = |k: usize| D::var(self.x[base + k], first + k);
        Ok(match geo.geom {
            Geom::Point { .. } => G::Point(P { x: v(0), y: v(1) }),
            Geom::Line { .. } => G::Line(P { x: v(0), y: v(1) }, P { x: v(2), y: v(3) }),
            Geom::Circle { .. } => G::Circle(P { x: v(0), y: v(1) }, v(2)),
            Geom::Arc { .. } => G::Arc(P { x: v(0), y: v(1) }, v(2), v(3), v(4)),
        })
    }
    fn point(&mut self, id: i32, pos: Pos) -> Result<P, String> {
        let g = self.geo(id)?;
        let polar = |c: P, r: D, a: D| P {
            x: c.x + r * a.cos(),
            y: c.y + r * a.sin(),
        };
        match (g, pos) {
            (G::Point(p), Pos::Start) => Ok(p),
            (G::Line(a, _), Pos::Start) => Ok(a),
            (G::Line(_, b), Pos::End) => Ok(b),
            (G::Circle(c, _), Pos::Center) | (G::Arc(c, ..), Pos::Center) => Ok(c),
            (G::Arc(c, r, a0, _), Pos::Start) => Ok(polar(c, r, a0)),
            (G::Arc(c, r, _, a1), Pos::End) => Ok(polar(c, r, a1)),
            _ => Err("that geometry has no such point".into()),
        }
    }
    fn line(&mut self, id: i32) -> Result<(P, P), String> {
        match self.geo(id)? {
            G::Line(a, b) => Ok((a, b)),
            _ => Err("the constraint needs a line".into()),
        }
    }
    fn round(&mut self, id: i32) -> Result<(P, D), String> {
        match self.geo(id)? {
            G::Circle(c, r) | G::Arc(c, r, ..) => Ok((c, r)),
            _ => Err("the constraint needs a circle or an arc".into()),
        }
    }
}

/// Unit-free "how far off" for direction constraints: sin of the angle between.
fn parallel(d1: P, d2: P) -> D {
    d1.cross(d2) / (d1.len() * d2.len())
}
fn perpendicular(d1: P, d2: P) -> D {
    d1.dot(d2) / (d1.len() * d2.len())
}
/// Signed distance from `p` to the infinite line through `a` and `b`.
fn line_distance(a: P, b: P, p: P) -> D {
    let d = b.sub(a);
    d.cross(p.sub(a)) / d.len()
}
/// Unit radial direction of an arc at one of its ends.
fn radial(g: G, pos: Pos) -> Option<P> {
    match (g, pos) {
        (G::Arc(_, _, a0, _), Pos::Start) => Some(P {
            x: a0.cos(),
            y: a0.sin(),
        }),
        (G::Arc(_, _, _, a1), Pos::End) => Some(P {
            x: a1.cos(),
            y: a1.sin(),
        }),
        _ => None,
    }
}

/// Residual equations of one constraint, as duals over the local slots.
fn residuals(l: &mut Local<'_>, c: &Constraint) -> Result<Vec<D>, String> {
    let two_points = c.second != GEO_UNDEF && c.first_pos != Pos::None && c.second_pos != Pos::None;
    Ok(match c.kind {
        T::Coincident => {
            let a = l.point(c.first, c.first_pos)?;
            let b = l.point(c.second, c.second_pos)?;
            vec![a.x - b.x, a.y - b.y]
        }
        T::PointOnObject => {
            let p = l.point(c.first, c.first_pos)?;
            match l.geo(c.second)? {
                G::Line(a, b) => vec![line_distance(a, b, p)],
                G::Circle(ce, r) | G::Arc(ce, r, ..) => vec![p.sub(ce).len() - r],
                G::Point(_) => return Err("a point cannot lie on a point".into()),
            }
        }
        T::Horizontal | T::Vertical => {
            let (a, b) = if two_points {
                (
                    l.point(c.first, c.first_pos)?,
                    l.point(c.second, c.second_pos)?,
                )
            } else {
                l.line(c.first)?
            };
            vec![if c.kind == T::Horizontal {
                b.y - a.y
            } else {
                b.x - a.x
            }]
        }
        T::Parallel => {
            let (a, b) = l.line(c.first)?;
            let (p, q) = l.line(c.second)?;
            vec![parallel(b.sub(a), q.sub(p))]
        }
        T::Perpendicular => match (l.geo(c.first)?, l.geo(c.second)?) {
            (G::Line(a, b), G::Line(p, q)) => vec![perpendicular(b.sub(a), q.sub(p))],
            // A line normal to a circle passes through its centre.
            (G::Line(a, b), G::Circle(ce, _) | G::Arc(ce, ..))
            | (G::Circle(ce, _) | G::Arc(ce, ..), G::Line(a, b)) => {
                vec![line_distance(a, b, ce)]
            }
            _ => return Err("perpendicular needs two lines, or a line and a circle".into()),
        },
        T::Tangent if c.first_pos != Pos::None && c.second_pos != Pos::None => {
            // Endpoint-to-endpoint tangency: the ends meet and the directions agree.
            let pa = l.point(c.first, c.first_pos)?;
            let pb = l.point(c.second, c.second_pos)?;
            let ga = l.geo(c.first)?;
            let gb = l.geo(c.second)?;
            let dir = match (ga, gb) {
                (G::Line(a, b), G::Line(p, q)) => parallel(b.sub(a), q.sub(p)),
                (G::Line(a, b), arc @ G::Arc(..)) => {
                    perpendicular(b.sub(a), radial(arc, c.second_pos).ok_or("bad arc end")?)
                }
                (arc @ G::Arc(..), G::Line(a, b)) => {
                    perpendicular(b.sub(a), radial(arc, c.first_pos).ok_or("bad arc end")?)
                }
                (arc1 @ G::Arc(c1, ..), G::Arc(c2, ..)) => {
                    let e = radial(arc1, c.first_pos).ok_or("bad arc end")?;
                    let d = c2.sub(c1);
                    e.cross(d) / (d.len() + 1e-300)
                }
                _ => return Err("endpoint tangency needs lines and arcs".into()),
            };
            vec![pa.x - pb.x, pa.y - pb.y, dir]
        }
        T::Tangent => match (l.geo(c.first)?, l.geo(c.second)?) {
            (G::Line(a, b), G::Circle(ce, r) | G::Arc(ce, r, ..))
            | (G::Circle(ce, r) | G::Arc(ce, r, ..), G::Line(a, b)) => {
                vec![line_distance(a, b, ce).abs() - r]
            }
            (G::Circle(c1, r1) | G::Arc(c1, r1, ..), G::Circle(c2, r2) | G::Arc(c2, r2, ..)) => {
                let d = c2.sub(c1).len();
                if c.internal {
                    vec![d - (r1 - r2).abs()]
                } else {
                    vec![d - (r1 + r2)]
                }
            }
            (G::Line(a, b), G::Line(p, q)) => {
                // Two lines are tangent when they are collinear.
                vec![parallel(b.sub(a), q.sub(p)), line_distance(a, b, p)]
            }
            _ => return Err("tangent needs lines, circles or arcs".into()),
        },
        T::Equal => match (l.geo(c.first)?, l.geo(c.second)?) {
            (G::Line(a, b), G::Line(p, q)) => vec![b.sub(a).len() - q.sub(p).len()],
            (G::Circle(_, r1) | G::Arc(_, r1, ..), G::Circle(_, r2) | G::Arc(_, r2, ..)) => {
                vec![r1 - r2]
            }
            _ => return Err("equal needs two lines, or two circles or arcs".into()),
        },
        T::Symmetric => {
            let p1 = l.point(c.first, c.first_pos)?;
            let p2 = l.point(c.second, c.second_pos)?;
            let m = p1.add(p2).scale(0.5);
            if c.third_pos == Pos::None {
                let (a, b) = l.line(c.third)?;
                let d = b.sub(a);
                vec![line_distance(a, b, m), p2.sub(p1).dot(d) / d.len()]
            } else {
                let q = l.point(c.third, c.third_pos)?;
                vec![m.x - q.x, m.y - q.y]
            }
        }
        T::Block => {
            let geo =
                l.s.geo(c.first)
                    .ok_or("constraint names missing geometry")?;
            if c.frozen.len() != geo.geom.param_count() {
                return Err("block constraint does not match its geometry".into());
            }
            let vars: Vec<D> = match l.geo(c.first)? {
                G::Point(p) => vec![p.x, p.y],
                G::Line(a, b) => vec![a.x, a.y, b.x, b.y],
                G::Circle(ce, r) => vec![ce.x, ce.y, r],
                G::Arc(ce, r, a0, a1) => vec![ce.x, ce.y, r, a0, a1],
            };
            vars.into_iter()
                .zip(&c.frozen)
                .map(|(v, f)| v - *f)
                .collect()
        }
        T::Distance => {
            if two_points {
                let a = l.point(c.first, c.first_pos)?;
                let b = l.point(c.second, c.second_pos)?;
                vec![b.sub(a).len() - c.value]
            } else if c.first_pos != Pos::None && c.second != GEO_UNDEF {
                let p = l.point(c.first, c.first_pos)?;
                let (a, b) = l.line(c.second)?;
                vec![line_distance(a, b, p).abs() - c.value]
            } else {
                let (a, b) = l.line(c.first)?;
                vec![b.sub(a).len() - c.value]
            }
        }
        T::DistanceX | T::DistanceY => {
            let (a, b) = if two_points {
                (
                    l.point(c.first, c.first_pos)?,
                    l.point(c.second, c.second_pos)?,
                )
            } else if c.first_pos != Pos::None {
                (l.point(H_AXIS, Pos::Start)?, l.point(c.first, c.first_pos)?)
            } else {
                l.line(c.first)?
            };
            vec![if c.kind == T::DistanceX {
                b.x - a.x - c.value
            } else {
                b.y - a.y - c.value
            }]
        }
        T::Radius => {
            let (_, r) = l.round(c.first)?;
            vec![r - c.value]
        }
        T::Diameter => {
            let (_, r) = l.round(c.first)?;
            vec![r * 2.0 - c.value]
        }
        T::Angle => {
            if c.second != GEO_UNDEF {
                let (a, b) = l.line(c.first)?;
                let (p, q) = l.line(c.second)?;
                let (d1, d2) = (b.sub(a), q.sub(p));
                vec![(D::atan2(d1.cross(d2), d1.dot(d2)) - c.value).wrap()]
            } else {
                match l.geo(c.first)? {
                    G::Line(a, b) => {
                        let d = b.sub(a);
                        vec![(D::atan2(d.y, d.x) - c.value).wrap()]
                    }
                    G::Arc(_, _, a0, a1) => vec![a1 - a0 - c.value],
                    _ => return Err("an angle needs a line, two lines or an arc".into()),
                }
            }
        }
    })
}

/// Reject a constraint whose references do not fit its type, before it joins a sketch.
pub fn validate(s: &Sketch, c: &Constraint) -> Result<(), String> {
    if !c.value.is_finite() {
        return Err("the value is not a number".into());
    }
    if c.kind.is_dimensional()
        && c.kind != T::Angle
        && c.kind != T::DistanceX
        && c.kind != T::DistanceY
        && c.value < 0.0
    {
        return Err("a length cannot be negative".into());
    }
    if matches!(c.kind, T::Radius | T::Diameter) && c.value <= 0.0 {
        return Err("a radius must be positive".into());
    }
    if c.kind == T::Block {
        if c.first < 0 || s.geo(c.first).is_none() {
            return Err("block needs sketch geometry".into());
        }
        return Ok(());
    }
    let layout = Layout::new(s);
    let x = params(s);
    let mut l = Local {
        s,
        x: &x,
        layout: &layout,
        slots: vec![],
    };
    residuals(&mut l, c).map(|_| ())
}

fn params(s: &Sketch) -> Vec<f64> {
    s.geos.iter().flat_map(|g| g.geom.params()).collect()
}

/// Evaluate the driving constraints: residuals, their Jacobian and which constraint
/// each row came from.
fn system(s: &Sketch, layout: &Layout, x: &[f64]) -> Result<(Vec<f64>, Mat, Vec<usize>), String> {
    let mut f = Vec::new();
    let mut rows: Vec<(Vec<(usize, f64)>, usize)> = Vec::new();
    for (index, c) in s.constraints.iter().enumerate() {
        if !c.driving {
            continue;
        }
        let mut l = Local {
            s,
            x,
            layout,
            slots: vec![],
        };
        for r in residuals(&mut l, c).map_err(|e| format!("constraint {}: {e}", index + 1))? {
            f.push(r.v);
            let entries = l
                .slots
                .iter()
                .enumerate()
                .map(|(slot, var)| (*var, r.g[slot]))
                .collect();
            rows.push((entries, index));
        }
    }
    let mut j = Mat::zeros(f.len(), layout.n);
    let mut owner = Vec::with_capacity(f.len());
    for (r, (entries, index)) in rows.into_iter().enumerate() {
        for (var, d) in entries {
            j.add(r, var, d);
        }
        owner.push(index);
    }
    Ok((f, j, owner))
}

fn norm(f: &[f64]) -> f64 {
    f.iter().map(|v| v * v).sum::<f64>()
}

/// Where the solver stands with a sketch, in FreeCAD's words.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolveStatus {
    #[default]
    Empty,
    FullyConstrained,
    UnderConstrained,
    /// Constraints that cannot all hold. Geometry is left as it was.
    Conflicting,
    /// Constraints that hold, but some say the same thing twice.
    Redundant,
    /// A constraint that fails to fit the geometry it names.
    Malformed,
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SolveReport {
    pub status: SolveStatus,
    pub dof: usize,
    /// 1-based constraint numbers, as FreeCAD lists them: the ones to remove.
    pub conflicting: Vec<usize>,
    /// Every constraint taking part in a conflict, for highlighting.
    #[serde(default)]
    pub conflict_groups: Vec<usize>,
    pub redundant: Vec<usize>,
    pub partially_redundant: Vec<usize>,
    pub malformed: Vec<usize>,
    /// Geometry whose every parameter the constraints determine.
    pub fully_constrained_geos: Vec<i32>,
    /// Individual points whose position the constraints determine.
    pub fixed_points: Vec<(i32, Pos)>,
    pub iterations: usize,
    pub residual: f64,
}
impl SolveReport {
    /// The solver message, as FreeCAD's task panel prints it.
    pub fn message(&self) -> String {
        let list = |v: &[usize]| {
            v.iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        match self.status {
            SolveStatus::Empty => "Empty sketch".into(),
            SolveStatus::FullyConstrained => "Fully constrained".into(),
            SolveStatus::UnderConstrained => format!(
                "Under constrained: {} DoF{}",
                self.dof,
                if self.dof == 1 { "" } else { "s" }
            ),
            SolveStatus::Conflicting => format!("Over-constrained: ({})", list(&self.conflicting)),
            SolveStatus::Redundant => {
                if self.redundant.is_empty() {
                    format!("Partially redundant: ({})", list(&self.partially_redundant))
                } else {
                    format!("Redundant constraints: ({})", list(&self.redundant))
                }
            }
            SolveStatus::Malformed => format!("Malformed constraints: ({})", list(&self.malformed)),
            SolveStatus::Failed => "Solver failed to converge".into(),
        }
    }
    pub fn solved(&self) -> bool {
        matches!(
            self.status,
            SolveStatus::Empty
                | SolveStatus::FullyConstrained
                | SolveStatus::UnderConstrained
                | SolveStatus::Redundant
        )
    }
}

/// Levenberg–Marquardt with minimum-norm steps from `x`. Parameters with infinite
/// weight do not move. Returns the point reached, its residuals, whether it solves the
/// system and how many iterations it took.
fn iterate(
    s: &Sketch,
    layout: &Layout,
    mut x: Vec<f64>,
    weight: &[f64],
) -> (Vec<f64>, Vec<f64>, bool, usize) {
    let (mut f, _, _) = system(s, layout, &x).expect("validated system");
    let mut lambda = 1e-9;
    let mut converged = norm(&f) < 1e-20;
    let mut iterations = 0;
    while !converged && iterations < 200 {
        iterations += 1;
        let (_, j, _) = system(s, layout, &x).expect("validated system");
        let (m, n) = (j.rows, j.cols);
        let current = norm(&f);
        let mut improved = false;
        for _attempt in 0..12 {
            // Minimum-norm step: dx = W⁻¹ Jᵀ (J W⁻¹ Jᵀ + λI)⁻¹ (−f).
            let mut a = Mat::zeros(m, m);
            for r in 0..m {
                for c in r..m {
                    let mut v = 0.0;
                    for (k, wk) in weight.iter().enumerate().take(n) {
                        let (jr, jc) = (j.at(r, k), j.at(c, k));
                        if jr != 0.0 && jc != 0.0 && wk.is_finite() {
                            v += jr * jc / wk;
                        }
                    }
                    a.set(r, c, v);
                    a.set(c, r, v);
                }
            }
            let scale = (0..m).map(|i| a.at(i, i)).fold(0.0f64, f64::max).max(1e-12);
            for i in 0..m {
                a.add(i, i, lambda * scale);
            }
            let rhs: Vec<f64> = f.iter().map(|v| -v).collect();
            let Some(y) = lin_solve(&a, &rhs) else {
                lambda *= 10.0;
                continue;
            };
            let mut trial = x.clone();
            for k in 0..n {
                if !weight[k].is_finite() {
                    continue;
                }
                let mut dx = 0.0;
                for (r, yr) in y.iter().enumerate().take(m) {
                    dx += j.at(r, k) * yr;
                }
                trial[k] += dx / weight[k];
            }
            let (ft, _, _) = system(s, layout, &trial).expect("validated system");
            let next = norm(&ft);
            if next.is_finite() && next < current {
                x = trial;
                f = ft;
                lambda = (lambda / 10.0).max(1e-12);
                improved = true;
                break;
            }
            lambda *= 10.0;
        }
        converged = norm(&f) < 1e-20;
        if !improved {
            break;
        }
    }
    (x, f, converged, iterations)
}

pub struct Drag {
    pub geo: i32,
    pub pos: Pos,
    pub target: V2,
}

/// Solve the sketch in place. On failure the geometry is left exactly as it was.
pub fn solve(s: &mut Sketch, drag: Option<Drag>) -> SolveReport {
    let layout = Layout::new(s);
    let mut report = SolveReport::default();
    // Malformed constraints are reported by number and nothing is solved.
    let malformed: Vec<usize> = s
        .constraints
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind != T::Block && validate(s, c).is_err())
        .map(|(i, _)| i + 1)
        .chain(
            s.constraints
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.kind == T::Block
                        && s.geo(c.first)
                            .is_none_or(|g| g.geom.param_count() != c.frozen.len())
                })
                .map(|(i, _)| i + 1),
        )
        .collect();
    if !malformed.is_empty() {
        report.status = SolveStatus::Malformed;
        report.malformed = malformed;
        return report;
    }
    let x0 = params(s);
    let mut x = x0.clone();
    let mut weight = vec![1.0; layout.n];
    // Scale-aware weights: angles move in radians, which are "larger" than millimetres.
    for (gi, g) in s.geos.iter().enumerate() {
        if let Geom::Arc { r, .. } = g.geom {
            let base = layout.offset[gi];
            let w = (r * r).max(1e-6);
            weight[base + 3] = w;
            weight[base + 4] = w;
        }
    }
    if let Some(d) = &drag {
        if let Some(geo) = s.geo(d.geo) {
            let base = layout.offset[d.geo as usize];
            let heavy = f64::INFINITY;
            match (&geo.geom, d.pos) {
                (Geom::Point { .. }, _) | (Geom::Line { .. }, Pos::Start) => {
                    x[base] = d.target.x;
                    x[base + 1] = d.target.y;
                    weight[base] = heavy;
                    weight[base + 1] = heavy;
                }
                (Geom::Line { .. }, Pos::End) => {
                    x[base + 2] = d.target.x;
                    x[base + 3] = d.target.y;
                    weight[base + 2] = heavy;
                    weight[base + 3] = heavy;
                }
                (Geom::Line { a, b }, Pos::None) => {
                    // Grab the edge where it is nearest the pointer: move it bodily.
                    let mid = a.lerp(*b, 0.5);
                    let delta = d.target - mid;
                    for k in 0..4 {
                        x[base + k] += if k % 2 == 0 { delta.x } else { delta.y };
                        weight[base + k] = heavy;
                    }
                }
                (Geom::Circle { .. } | Geom::Arc { .. }, Pos::Center) => {
                    x[base] = d.target.x;
                    x[base + 1] = d.target.y;
                    weight[base] = heavy;
                    weight[base + 1] = heavy;
                }
                (Geom::Circle { c, .. } | Geom::Arc { c, .. }, Pos::None) => {
                    x[base + 2] = d.target.dist(*c);
                    weight[base + 2] = heavy;
                }
                (Geom::Arc { c, .. }, Pos::Start) => {
                    x[base + 3] = (d.target - *c).angle();
                    x[base + 2] = d.target.dist(*c);
                    weight[base + 3] = heavy;
                }
                (Geom::Arc { c, .. }, Pos::End) => {
                    let mut a = (d.target - *c).angle();
                    while a <= x[base + 3] {
                        a += TAU;
                    }
                    x[base + 4] = a;
                    x[base + 2] = d.target.dist(*c);
                    weight[base + 4] = heavy;
                }
                _ => {}
            }
        }
    }
    let x0_drag = x.clone();
    if system(s, &layout, &x).is_err() {
        report.status = SolveStatus::Malformed;
        return report;
    }
    // A drag first pins the dragged parameters exactly; if the constraints will not
    // allow that, it settles for the nearest place they do.
    let (mut x, mut f, mut converged, mut iterations) = iterate(s, &layout, x.clone(), &weight);
    if !converged && drag.is_some() {
        let soft: Vec<f64> = weight
            .iter()
            .map(|w| if w.is_infinite() { 1e6 } else { *w })
            .collect();
        let start = x0.clone();
        let mut moved = start.clone();
        for k in 0..layout.n {
            if weight[k].is_infinite() {
                moved[k] = x0_drag[k];
            }
        }
        let (x2, f2, c2, i2) = iterate(s, &layout, moved, &soft);
        (x, f, converged, iterations) = (x2, f2, c2, iterations + i2);
    }
    report.iterations = iterations;
    report.residual = norm(&f).sqrt();
    // Diagnose at the final point: rank, free parameters, dependent equations.
    let (_, j, owner) = system(s, &layout, &x).expect("validated system");
    let (rank, null) = if j.rows == 0 {
        (0, identity_basis(layout.n))
    } else {
        null_space(&j, 1e-9)
    };
    report.dof = layout.n - rank;
    let free: Vec<bool> = (0..layout.n)
        .map(|k| {
            null.iter().any(|v| {
                let big = v.iter().fold(0.0f64, |s, e| s.max(e.abs()));
                v[k].abs() > big * 1e-7
            })
        })
        .collect();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut dependent_rows_of: Vec<usize> = vec![0; s.constraints.len()];
    let mut rows_of: Vec<usize> = vec![0; s.constraints.len()];
    for &o in &owner {
        rows_of[o] += 1;
    }
    if j.rows > 0 {
        for (row, members) in dependent_rows(&j, 1e-8) {
            dependent_rows_of[owner[row]] += 1;
            let mut g: Vec<usize> = members.iter().map(|r| owner[*r]).collect();
            g.sort_unstable();
            g.dedup();
            groups.push(g);
        }
    }
    if converged {
        // Commit: the sketch now satisfies its constraints.
        let mut at = 0;
        for g in &mut s.geos {
            let count = g.geom.param_count();
            g.geom = g.geom.with_params(&x[at..at + count]);
            at += count;
        }
        report.redundant = (0..s.constraints.len())
            .filter(|&i| rows_of[i] > 0 && dependent_rows_of[i] == rows_of[i])
            .map(|i| i + 1)
            .collect();
        report.partially_redundant = (0..s.constraints.len())
            .filter(|&i| dependent_rows_of[i] > 0 && dependent_rows_of[i] < rows_of[i])
            .map(|i| i + 1)
            .collect();
        report.status = if s.geos.is_empty() {
            SolveStatus::Empty
        } else if !report.redundant.is_empty() || !report.partially_redundant.is_empty() {
            SolveStatus::Redundant
        } else if report.dof == 0 {
            SolveStatus::FullyConstrained
        } else {
            SolveStatus::UnderConstrained
        };
    } else {
        let mut involved: Vec<usize> = groups.iter().flatten().map(|i| i + 1).collect();
        involved.sort_unstable();
        involved.dedup();
        report.conflict_groups = involved;
        // Of each dependent set, the one to remove is the one added last, as FreeCAD
        // suggests: removing it lets the rest hold again.
        let mut conflicting: Vec<usize> = groups
            .iter()
            .filter_map(|g| g.iter().max().map(|i| i + 1))
            .collect();
        conflicting.sort_unstable();
        conflicting.dedup();
        report.status = if conflicting.is_empty() {
            SolveStatus::Failed
        } else {
            SolveStatus::Conflicting
        };
        report.conflicting = conflicting;
    }
    for (gi, g) in s.geos.iter().enumerate() {
        let base = layout.offset[gi];
        let count = g.geom.param_count();
        if (base..base + count).all(|k| !free[k]) {
            report.fully_constrained_geos.push(gi as i32);
        }
        let fixed = |ks: &[usize]| ks.iter().all(|k| !free[base + k]);
        for pos in g.geom.points() {
            let determined = match (&g.geom, pos) {
                (Geom::Point { .. }, _) => fixed(&[0, 1]),
                (Geom::Line { .. }, Pos::Start) => fixed(&[0, 1]),
                (Geom::Line { .. }, Pos::End) => fixed(&[2, 3]),
                (_, Pos::Center) => fixed(&[0, 1]),
                (Geom::Arc { .. }, Pos::Start) => fixed(&[0, 1, 2, 3]),
                (Geom::Arc { .. }, Pos::End) => fixed(&[0, 1, 2, 4]),
                _ => false,
            };
            if determined {
                report.fixed_points.push((gi as i32, pos));
            }
        }
    }
    report
}

fn identity_basis(n: usize) -> Vec<Vec<f64>> {
    (0..n)
        .map(|i| {
            let mut v = vec![0.0; n];
            v[i] = 1.0;
            v
        })
        .collect()
}

/// Current value a dimensional constraint would measure, used to seed a new dimension
/// from the geometry as drawn and to show reference dimensions.
pub fn measure(s: &Sketch, c: &Constraint) -> Result<f64, String> {
    let mut probe = c.clone();
    probe.value = 0.0;
    probe.driving = true;
    let layout = Layout::new(s);
    let x = params(s);
    let mut l = Local {
        s,
        x: &x,
        layout: &layout,
        slots: vec![],
    };
    let r = residuals(&mut l, &probe)?;
    let v = r.first().ok_or("constraint measures nothing")?.v;
    Ok(match c.kind {
        // Residual is `measured - 0` (wrapped for angles).
        T::Distance | T::DistanceX | T::DistanceY | T::Radius | T::Diameter => v,
        T::Angle => {
            if v < 0.0 && c.second == GEO_UNDEF {
                v + 2.0 * PI
            } else {
                v
            }
        }
        _ => return Err("not a dimension".into()),
    })
}
