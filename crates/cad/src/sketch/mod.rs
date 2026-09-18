//! Sketcher: planar geometry, geometric and dimensional constraints, and the solver
//! that keeps them consistent. The data model follows FreeCAD's `Sketcher::SketchObject`
//! closely — geometry is addressed by a `GeoId` (non-negative for the sketch's own
//! geometry, `-1` for the horizontal axis, `-2` for the vertical axis) and a point on it
//! by a `Pos` (start, end or centre); a constraint names up to three of these plus a
//! value — so that a user who knows FreeCAD finds the same concepts here.
use crate::math::{v2, wrap_positive, PI, TAU, V2};
use serde::{Deserialize, Serialize};

pub mod profile;
pub mod solver;
pub mod tools;

pub use solver::{SolveReport, SolveStatus};

/// The sketch's horizontal axis (FreeCAD `H_Axis`); its start point is the origin.
pub const H_AXIS: i32 = -1;
/// The sketch's vertical axis (FreeCAD `V_Axis`).
pub const V_AXIS: i32 = -2;
/// No geometry (FreeCAD `GeoUndef`).
pub const GEO_UNDEF: i32 = -2000;

/// A point on a piece of geometry: FreeCAD's `PointPos`.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Pos {
    /// The edge itself rather than one of its points.
    #[default]
    None,
    Start,
    End,
    Center,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Geom {
    Point {
        p: V2,
    },
    Line {
        a: V2,
        b: V2,
    },
    Circle {
        c: V2,
        r: f64,
    },
    /// Counter-clockwise from `start` to `end` (radians), `end` in `(start, start + 2π]`.
    Arc {
        c: V2,
        r: f64,
        start: f64,
        end: f64,
    },
}
impl Geom {
    /// Solver parameters, in the order `solver` reads them.
    pub fn params(&self) -> Vec<f64> {
        match *self {
            Geom::Point { p } => vec![p.x, p.y],
            Geom::Line { a, b } => vec![a.x, a.y, b.x, b.y],
            Geom::Circle { c, r } => vec![c.x, c.y, r],
            Geom::Arc { c, r, start, end } => vec![c.x, c.y, r, start, end],
        }
    }
    pub fn param_count(&self) -> usize {
        match self {
            Geom::Point { .. } => 2,
            Geom::Line { .. } => 4,
            Geom::Circle { .. } => 3,
            Geom::Arc { .. } => 5,
        }
    }
    /// Rebuild from solver parameters, normalising radius sign and arc angles.
    pub fn with_params(&self, p: &[f64]) -> Geom {
        match self {
            Geom::Point { .. } => Geom::Point { p: v2(p[0], p[1]) },
            Geom::Line { .. } => Geom::Line {
                a: v2(p[0], p[1]),
                b: v2(p[2], p[3]),
            },
            Geom::Circle { .. } => Geom::Circle {
                c: v2(p[0], p[1]),
                r: p[2].abs(),
            },
            Geom::Arc { .. } => {
                let (mut r, mut start, mut end) = (p[2], p[3], p[4]);
                if r < 0.0 {
                    r = -r;
                    start += PI;
                    end += PI;
                }
                let s = wrap_positive(start);
                let mut sweep = end - start;
                if sweep <= 0.0 {
                    sweep = wrap_positive(sweep);
                    if sweep == 0.0 {
                        sweep = TAU;
                    }
                } else if sweep > TAU {
                    sweep = TAU;
                }
                Geom::Arc {
                    c: v2(p[0], p[1]),
                    r,
                    start: s,
                    end: s + sweep,
                }
            }
        }
    }
    pub fn point(&self, pos: Pos) -> Option<V2> {
        match (self, pos) {
            (Geom::Point { p }, Pos::Start) => Some(*p),
            (Geom::Line { a, .. }, Pos::Start) => Some(*a),
            (Geom::Line { b, .. }, Pos::End) => Some(*b),
            (Geom::Circle { c, .. }, Pos::Center) => Some(*c),
            (Geom::Arc { c, .. }, Pos::Center) => Some(*c),
            (Geom::Arc { c, r, start, .. }, Pos::Start) => Some(*c + V2::polar(*start, *r)),
            (Geom::Arc { c, r, end, .. }, Pos::End) => Some(*c + V2::polar(*end, *r)),
            _ => None,
        }
    }
    /// The points a user can pick on this geometry.
    pub fn points(&self) -> Vec<Pos> {
        match self {
            Geom::Point { .. } => vec![Pos::Start],
            Geom::Line { .. } => vec![Pos::Start, Pos::End],
            Geom::Circle { .. } => vec![Pos::Center],
            Geom::Arc { .. } => vec![Pos::Start, Pos::End, Pos::Center],
        }
    }
    pub fn is_edge(&self) -> bool {
        !matches!(self, Geom::Point { .. })
    }
    pub fn is_curve(&self) -> bool {
        matches!(self, Geom::Circle { .. } | Geom::Arc { .. })
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Geom::Point { .. } => "Point",
            Geom::Line { .. } => "Line segment",
            Geom::Circle { .. } => "Circle",
            Geom::Arc { .. } => "Arc of circle",
        }
    }
    /// Closest distance from `q` to this geometry.
    pub fn distance(&self, q: V2) -> f64 {
        match *self {
            Geom::Point { p } => p.dist(q),
            Geom::Line { a, b } => segment_distance(a, b, q),
            Geom::Circle { c, r } => (q.dist(c) - r).abs(),
            Geom::Arc { c, r, start, end } => {
                let ang = (q - c).angle();
                if angle_in_arc(ang, start, end) {
                    (q.dist(c) - r).abs()
                } else {
                    let s = c + V2::polar(start, r);
                    let e = c + V2::polar(end, r);
                    q.dist(s).min(q.dist(e))
                }
            }
        }
    }
    /// Length of the edge (circumference for a circle).
    pub fn length(&self) -> f64 {
        match *self {
            Geom::Point { .. } => 0.0,
            Geom::Line { a, b } => a.dist(b),
            Geom::Circle { r, .. } => TAU * r,
            Geom::Arc { r, start, end, .. } => r * (end - start),
        }
    }
    /// A point in the middle of the edge, for labels and picking.
    pub fn midpoint(&self) -> V2 {
        match *self {
            Geom::Point { p } => p,
            Geom::Line { a, b } => a.lerp(b, 0.5),
            Geom::Circle { c, r } => c + V2::polar(PI / 4.0, r),
            Geom::Arc { c, r, start, end } => c + V2::polar((start + end) / 2.0, r),
        }
    }
    /// Polyline approximation with at most `max_step` radians per chord on curves.
    pub fn polyline(&self, max_step: f64) -> Vec<V2> {
        match *self {
            Geom::Point { p } => vec![p],
            Geom::Line { a, b } => vec![a, b],
            Geom::Circle { c, r } => {
                let n = ((TAU / max_step).ceil() as usize).max(8);
                (0..=n)
                    .map(|i| c + V2::polar(TAU * i as f64 / n as f64, r))
                    .collect()
            }
            Geom::Arc { c, r, start, end } => {
                let n = (((end - start) / max_step).ceil() as usize).max(2);
                (0..=n)
                    .map(|i| c + V2::polar(start + (end - start) * i as f64 / n as f64, r))
                    .collect()
            }
        }
    }
    pub fn translated(&self, d: V2) -> Geom {
        match *self {
            Geom::Point { p } => Geom::Point { p: p + d },
            Geom::Line { a, b } => Geom::Line { a: a + d, b: b + d },
            Geom::Circle { c, r } => Geom::Circle { c: c + d, r },
            Geom::Arc { c, r, start, end } => Geom::Arc {
                c: c + d,
                r,
                start,
                end,
            },
        }
    }
    pub fn is_finite(&self) -> bool {
        self.params().iter().all(|v| v.is_finite())
    }
}
pub fn segment_distance(a: V2, b: V2, q: V2) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    if l2 == 0.0 {
        return q.dist(a);
    }
    let t = ((q - a).dot(d) / l2).clamp(0.0, 1.0);
    q.dist(a + d * t)
}
/// Whether `angle` lies on the counter-clockwise sweep from `start` to `end`.
pub fn angle_in_arc(angle: f64, start: f64, end: f64) -> bool {
    let sweep = end - start;
    let rel = wrap_positive(angle - start);
    rel <= sweep + 1e-12
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Geo {
    #[serde(flatten)]
    pub geom: Geom,
    #[serde(default)]
    pub construction: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConstraintType {
    Coincident,
    PointOnObject,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Tangent,
    Equal,
    Symmetric,
    Block,
    Distance,
    DistanceX,
    DistanceY,
    Radius,
    Diameter,
    Angle,
}
impl ConstraintType {
    pub fn is_dimensional(self) -> bool {
        matches!(
            self,
            Self::Distance
                | Self::DistanceX
                | Self::DistanceY
                | Self::Radius
                | Self::Diameter
                | Self::Angle
        )
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Coincident => "Coincident",
            Self::PointOnObject => "Point on object",
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
            Self::Parallel => "Parallel",
            Self::Perpendicular => "Perpendicular",
            Self::Tangent => "Tangent",
            Self::Equal => "Equal",
            Self::Symmetric => "Symmetric",
            Self::Block => "Block",
            Self::Distance => "Distance",
            Self::DistanceX => "Horizontal distance",
            Self::DistanceY => "Vertical distance",
            Self::Radius => "Radius",
            Self::Diameter => "Diameter",
            Self::Angle => "Angle",
        }
    }
}

/// One constraint, addressed the way FreeCAD's `Sketcher::Constraint` is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Constraint {
    #[serde(rename = "Type")]
    pub kind: ConstraintType,
    #[serde(rename = "First")]
    pub first: i32,
    #[serde(rename = "FirstPos", default)]
    pub first_pos: Pos,
    #[serde(rename = "Second", default = "undef")]
    pub second: i32,
    #[serde(rename = "SecondPos", default)]
    pub second_pos: Pos,
    #[serde(rename = "Third", default = "undef")]
    pub third: i32,
    #[serde(rename = "ThirdPos", default)]
    pub third_pos: Pos,
    /// Millimetres, or radians for an angle. Unused by geometric constraints.
    #[serde(rename = "Value", default)]
    pub value: f64,
    /// A reference (non-driving) dimension measures without constraining.
    #[serde(rename = "Driving", default = "yes")]
    pub driving: bool,
    #[serde(rename = "Name", default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Tangency between two circles: inside rather than outside each other.
    #[serde(rename = "Internal", default, skip_serializing_if = "is_false")]
    pub internal: bool,
    /// Parameters a Block constraint holds fixed.
    #[serde(rename = "Frozen", default, skip_serializing_if = "Vec::is_empty")]
    pub frozen: Vec<f64>,
}
fn undef() -> i32 {
    GEO_UNDEF
}
fn yes() -> bool {
    true
}
fn is_false(v: &bool) -> bool {
    !*v
}
impl Constraint {
    pub fn new(kind: ConstraintType, first: i32, first_pos: Pos) -> Constraint {
        Constraint {
            kind,
            first,
            first_pos,
            second: GEO_UNDEF,
            second_pos: Pos::None,
            third: GEO_UNDEF,
            third_pos: Pos::None,
            value: 0.0,
            driving: true,
            name: String::new(),
            internal: false,
            frozen: vec![],
        }
    }
    pub fn with_second(mut self, geo: i32, pos: Pos) -> Self {
        self.second = geo;
        self.second_pos = pos;
        self
    }
    pub fn with_third(mut self, geo: i32, pos: Pos) -> Self {
        self.third = geo;
        self.third_pos = pos;
        self
    }
    pub fn with_value(mut self, value: f64) -> Self {
        self.value = value;
        self
    }
    /// Geometry ids this constraint references (sketch geometry only).
    pub fn geos(&self) -> Vec<i32> {
        [self.first, self.second, self.third]
            .into_iter()
            .filter(|g| *g >= 0)
            .collect()
    }
    pub fn references(&self, geo: i32) -> bool {
        self.first == geo || self.second == geo || self.third == geo
    }
    /// The value as the user reads it: degrees for angles, millimetres otherwise.
    pub fn display_value(&self) -> String {
        match self.kind {
            ConstraintType::Angle => format!(
                "{} °",
                crate::math::fmt_num(crate::math::degrees(self.value), 2)
            ),
            ConstraintType::Radius => format!("R{} mm", crate::math::fmt_num(self.value, 2)),
            ConstraintType::Diameter => format!("⌀{} mm", crate::math::fmt_num(self.value, 2)),
            _ => format!("{} mm", crate::math::fmt_num(self.value, 2)),
        }
    }
}

/// A sketch: its geometry, its constraints and the last solver verdict.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    #[serde(rename = "Geometry")]
    pub geos: Vec<Geo>,
    #[serde(rename = "Constraints")]
    pub constraints: Vec<Constraint>,
}

impl Sketch {
    pub fn geo(&self, id: i32) -> Option<&Geo> {
        usize::try_from(id).ok().and_then(|i| self.geos.get(i))
    }
    /// Geometry by id, including the two axes.
    pub fn geom(&self, id: i32) -> Option<Geom> {
        match id {
            H_AXIS => Some(Geom::Line {
                a: V2::ZERO,
                b: v2(1.0, 0.0),
            }),
            V_AXIS => Some(Geom::Line {
                a: V2::ZERO,
                b: v2(0.0, 1.0),
            }),
            _ => self.geo(id).map(|g| g.geom.clone()),
        }
    }
    pub fn point(&self, id: i32, pos: Pos) -> Option<V2> {
        self.geom(id)?.point(pos)
    }
    pub fn add_geo(&mut self, geom: Geom, construction: bool) -> i32 {
        self.geos.push(Geo { geom, construction });
        self.geos.len() as i32 - 1
    }
    /// Add a constraint after checking it names geometry of the right kinds. Returns
    /// its index. A Block captures the geometry's current parameters.
    pub fn add_constraint(&mut self, mut c: Constraint) -> Result<usize, String> {
        solver::validate(self, &c)?;
        if c.kind == ConstraintType::Block {
            c.frozen = self.geo(c.first).ok_or("no such geometry")?.geom.params();
        }
        self.constraints.push(c);
        Ok(self.constraints.len() - 1)
    }
    /// Delete geometry, dropping every constraint that referenced it and renumbering
    /// the rest, as FreeCAD does.
    pub fn delete_geos(&mut self, ids: &[i32]) {
        let mut ids: Vec<i32> = ids.iter().copied().filter(|i| *i >= 0).collect();
        ids.sort_unstable();
        ids.dedup();
        self.constraints
            .retain(|c| !ids.iter().any(|id| c.references(*id)));
        for id in ids.iter().rev() {
            if (*id as usize) < self.geos.len() {
                self.geos.remove(*id as usize);
            }
        }
        let shift = |g: i32| -> i32 {
            if g < 0 {
                g
            } else {
                g - ids.iter().filter(|d| **d < g).count() as i32
            }
        };
        for c in &mut self.constraints {
            c.first = shift(c.first);
            c.second = shift(c.second);
            c.third = shift(c.third);
        }
    }
    pub fn delete_constraints(&mut self, indices: &[usize]) {
        let mut idx: Vec<usize> = indices.to_vec();
        idx.sort_unstable();
        idx.dedup();
        for i in idx.into_iter().rev() {
            if i < self.constraints.len() {
                self.constraints.remove(i);
            }
        }
    }
    /// Constraints that touch a geometry, for its tree/list entry.
    pub fn constraints_on(&self, geo: i32) -> Vec<usize> {
        (0..self.constraints.len())
            .filter(|i| self.constraints[*i].references(geo))
            .collect()
    }
    pub fn solve(&mut self) -> SolveReport {
        solver::solve(self, None)
    }
    /// Move one point (or a whole edge when `pos` is `None`) towards `target`, letting
    /// the constraints decide how the rest follows. Nothing moves when the drag would
    /// break a constraint.
    pub fn drag(&mut self, geo: i32, pos: Pos, target: V2) -> SolveReport {
        solver::solve(self, Some(solver::Drag { geo, pos, target }))
    }
    /// Axis-aligned bounds of the sketch's geometry, `None` when empty.
    pub fn bounds(&self) -> Option<(V2, V2)> {
        let mut lo = v2(f64::INFINITY, f64::INFINITY);
        let mut hi = v2(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for g in &self.geos {
            for p in g.geom.polyline(PI / 16.0) {
                lo = v2(lo.x.min(p.x), lo.y.min(p.y));
                hi = v2(hi.x.max(p.x), hi.y.max(p.y));
            }
        }
        lo.x.is_finite().then_some((lo, hi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deleting_geometry_renumbers_constraints() {
        let mut s = Sketch::default();
        let a = s.add_geo(
            Geom::Line {
                a: v2(0.0, 0.0),
                b: v2(1.0, 0.0),
            },
            false,
        );
        let b = s.add_geo(
            Geom::Line {
                a: v2(1.0, 0.0),
                b: v2(1.0, 1.0),
            },
            false,
        );
        let c = s.add_geo(
            Geom::Line {
                a: v2(1.0, 1.0),
                b: v2(0.0, 1.0),
            },
            false,
        );
        s.add_constraint(
            Constraint::new(ConstraintType::Coincident, a, Pos::End).with_second(b, Pos::Start),
        )
        .unwrap();
        s.add_constraint(
            Constraint::new(ConstraintType::Coincident, b, Pos::End).with_second(c, Pos::Start),
        )
        .unwrap();
        s.add_constraint(Constraint::new(ConstraintType::Horizontal, c, Pos::None))
            .unwrap();
        s.delete_geos(&[a]);
        assert_eq!(s.geos.len(), 2);
        assert_eq!(s.constraints.len(), 2);
        assert_eq!((s.constraints[0].first, s.constraints[0].second), (0, 1));
        assert_eq!(s.constraints[1].first, 1);
    }
    #[test]
    fn arcs_normalise_their_sweep() {
        let g = Geom::Arc {
            c: V2::ZERO,
            r: 1.0,
            start: 0.0,
            end: 1.0,
        };
        let n = g.with_params(&[0.0, 0.0, -2.0, 0.0, 1.0]);
        let Geom::Arc { r, start, end, .. } = n else {
            panic!()
        };
        assert!((r - 2.0).abs() < 1e-12);
        assert!((start - PI).abs() < 1e-12 && (end - start - 1.0).abs() < 1e-12);
    }
}
