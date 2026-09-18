//! Sketcher tools: the geometry-creation commands (each adds the constraints FreeCAD's
//! own tool adds), and the editing tools trim, extend and fillet.
use super::{angle_in_arc, Constraint, ConstraintType as T, Geom, Pos, Sketch, GEO_UNDEF};
use crate::math::{v2, wrap_positive, PI, TAU, V2};

fn coincident(a: i32, pa: Pos, b: i32, pb: Pos) -> Constraint {
    Constraint::new(T::Coincident, a, pa).with_second(b, pb)
}
fn push(s: &mut Sketch, c: Constraint) {
    // The tools only build constraints over geometry they just made, so these hold.
    s.add_constraint(c).expect("tool constraint is well formed");
}

/// Lines within this many radians of an axis get FreeCAD's automatic H/V constraint.
const AUTO_AXIS: f64 = 0.035;

fn auto_axis(s: &mut Sketch, id: i32) {
    if let Some(Geom::Line { a, b }) = s.geom(id) {
        let d = b - a;
        if d.len() == 0.0 {
            return;
        }
        let ang = wrap_positive(d.angle()) % PI;
        if ang < AUTO_AXIS || PI - ang < AUTO_AXIS {
            push(s, Constraint::new(T::Horizontal, id, Pos::None));
        } else if (ang - PI / 2.0).abs() < AUTO_AXIS {
            push(s, Constraint::new(T::Vertical, id, Pos::None));
        }
    }
}

pub fn point(s: &mut Sketch, p: V2, construction: bool) -> i32 {
    s.add_geo(Geom::Point { p }, construction)
}

pub fn line(s: &mut Sketch, a: V2, b: V2, construction: bool, auto: bool) -> Result<i32, String> {
    if a.dist(b) < 1e-9 {
        return Err("a line needs two different points".into());
    }
    let id = s.add_geo(Geom::Line { a, b }, construction);
    if auto {
        auto_axis(s, id);
    }
    Ok(id)
}

/// Connected line segments, each joined to the next by a coincident constraint, and
/// the last to the first when `closed`.
pub fn polyline(
    s: &mut Sketch,
    pts: &[V2],
    closed: bool,
    construction: bool,
) -> Result<Vec<i32>, String> {
    let pts: Vec<V2> = pts.iter().copied().fold(Vec::<V2>::new(), |mut acc, p| {
        if acc.last().is_none_or(|q| q.dist(p) > 1e-9) {
            acc.push(p);
        }
        acc
    });
    if pts.len() < 2 {
        return Err("a polyline needs at least two points".into());
    }
    let mut ids = Vec::new();
    for w in pts.windows(2) {
        ids.push(line(s, w[0], w[1], construction, true)?);
    }
    if closed && pts.len() > 2 && pts[0].dist(*pts.last().unwrap()) > 1e-9 {
        ids.push(line(s, *pts.last().unwrap(), pts[0], construction, true)?);
    }
    for w in ids.windows(2) {
        push(s, coincident(w[0], Pos::End, w[1], Pos::Start));
    }
    if closed && ids.len() > 2 {
        push(
            s,
            coincident(*ids.last().unwrap(), Pos::End, ids[0], Pos::Start),
        );
    }
    Ok(ids)
}

/// FreeCAD's rectangle: four lines, four coincidences, two horizontals, two verticals.
pub fn rectangle(s: &mut Sketch, p1: V2, p2: V2, construction: bool) -> Result<Vec<i32>, String> {
    let (lo, hi) = (
        v2(p1.x.min(p2.x), p1.y.min(p2.y)),
        v2(p1.x.max(p2.x), p1.y.max(p2.y)),
    );
    if hi.x - lo.x < 1e-9 || hi.y - lo.y < 1e-9 {
        return Err("a rectangle needs two opposite corners".into());
    }
    let corners = [lo, v2(hi.x, lo.y), hi, v2(lo.x, hi.y)];
    let mut ids = Vec::new();
    for i in 0..4 {
        ids.push(s.add_geo(
            Geom::Line {
                a: corners[i],
                b: corners[(i + 1) % 4],
            },
            construction,
        ));
    }
    for i in 0..4 {
        push(
            s,
            coincident(ids[i], Pos::End, ids[(i + 1) % 4], Pos::Start),
        );
    }
    push(s, Constraint::new(T::Horizontal, ids[0], Pos::None));
    push(s, Constraint::new(T::Horizontal, ids[2], Pos::None));
    push(s, Constraint::new(T::Vertical, ids[1], Pos::None));
    push(s, Constraint::new(T::Vertical, ids[3], Pos::None));
    Ok(ids)
}

pub fn circle(s: &mut Sketch, c: V2, r: f64, construction: bool) -> Result<i32, String> {
    if r.is_nan() || r <= 1e-9 {
        return Err("a circle needs a radius".into());
    }
    Ok(s.add_geo(Geom::Circle { c, r }, construction))
}

/// Circle through three points.
pub fn circle_3pt(s: &mut Sketch, a: V2, b: V2, c: V2, construction: bool) -> Result<i32, String> {
    let (center, r) = circumcircle(a, b, c).ok_or("the three points lie on a line")?;
    circle(s, center, r, construction)
}

/// Arc by centre, a start point and an end point, counter-clockwise.
pub fn arc_center(
    s: &mut Sketch,
    c: V2,
    start: V2,
    end: V2,
    construction: bool,
) -> Result<i32, String> {
    let r = c.dist(start);
    if r < 1e-9 || c.dist(end) < 1e-9 {
        return Err("an arc needs a radius".into());
    }
    let a0 = wrap_positive((start - c).angle());
    let mut a1 = (end - c).angle();
    while a1 <= a0 {
        a1 += TAU;
    }
    Ok(s.add_geo(
        Geom::Arc {
            c,
            r,
            start: a0,
            end: a1,
        },
        construction,
    ))
}

/// Arc through a start point, a point on the arc and an end point.
pub fn arc_3pt(s: &mut Sketch, a: V2, on: V2, b: V2, construction: bool) -> Result<i32, String> {
    let (c, r) = circumcircle(a, on, b).ok_or("the three points lie on a line")?;
    let (aa, ab, ao) = ((a - c).angle(), (b - c).angle(), (on - c).angle());
    // Sweep counter-clockwise from whichever end lets the middle point lie on the arc.
    let (start, end) = if angle_in_arc(
        ao,
        wrap_positive(aa),
        wrap_positive(aa) + wrap_positive(ab - aa),
    ) {
        (aa, ab)
    } else {
        (ab, aa)
    };
    let s0 = wrap_positive(start);
    let sweep = wrap_positive(end - start);
    Ok(s.add_geo(
        Geom::Arc {
            c,
            r,
            start: s0,
            end: s0 + if sweep == 0.0 { TAU } else { sweep },
        },
        construction,
    ))
}

/// FreeCAD's slot: two semicircles joined by two lines, all tangent at their ends, the
/// arcs equal, and the slot's axis horizontal or vertical when drawn so.
pub fn slot(
    s: &mut Sketch,
    c1: V2,
    c2: V2,
    r: f64,
    construction: bool,
) -> Result<Vec<i32>, String> {
    let axis = c2 - c1;
    if axis.len() < 1e-9 || (r.is_nan() || r <= 1e-9) {
        return Err("a slot needs two centres and a radius".into());
    }
    let ang = axis.angle();
    let n = axis.norm().perp() * r;
    // Arc around c1 on the far side, arc around c2 on the near side.
    let a1 = s.add_geo(
        Geom::Arc {
            c: c1,
            r,
            start: wrap_positive(ang + PI / 2.0),
            end: wrap_positive(ang + PI / 2.0) + PI,
        },
        construction,
    );
    let a2 = s.add_geo(
        Geom::Arc {
            c: c2,
            r,
            start: wrap_positive(ang - PI / 2.0),
            end: wrap_positive(ang - PI / 2.0) + PI,
        },
        construction,
    );
    // Arc 1 runs from c1+n to c1-n; arc 2 from c2-n to c2+n.
    let l1 = s.add_geo(
        Geom::Line {
            a: c1 - n,
            b: c2 - n,
        },
        construction,
    );
    let l2 = s.add_geo(
        Geom::Line {
            a: c2 + n,
            b: c1 + n,
        },
        construction,
    );
    let tangent =
        |a: i32, pa: Pos, b: i32, pb: Pos| Constraint::new(T::Tangent, a, pa).with_second(b, pb);
    push(s, tangent(a1, Pos::End, l1, Pos::Start));
    push(s, tangent(l1, Pos::End, a2, Pos::Start));
    push(s, tangent(a2, Pos::End, l2, Pos::Start));
    push(s, tangent(l2, Pos::End, a1, Pos::Start));
    push(
        s,
        Constraint::new(T::Equal, a1, Pos::None).with_second(a2, Pos::None),
    );
    let flat = wrap_positive(ang) % PI;
    if flat < AUTO_AXIS || PI - flat < AUTO_AXIS {
        push(s, Constraint::new(T::Horizontal, l1, Pos::None));
    } else if (flat - PI / 2.0).abs() < AUTO_AXIS {
        push(s, Constraint::new(T::Vertical, l1, Pos::None));
    }
    Ok(vec![a1, a2, l1, l2])
}

pub fn circumcircle(a: V2, b: V2, c: V2) -> Option<(V2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.dot(a), b.dot(b), c.dot(c));
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let center = v2(ux, uy);
    Some((center, center.dist(a)))
}

/// Where two pieces of geometry cross, each as a point. Curves are treated as their
/// full circles and lines as segments; callers filter by their own parameter ranges.
fn intersect_full(g: &Geom, h: &Geom) -> Vec<V2> {
    match (g, h) {
        (Geom::Line { a, b }, Geom::Line { a: c, b: d }) => {
            let (r, s) = (*b - *a, *d - *c);
            let den = r.cross(s);
            if den.abs() < 1e-12 {
                return vec![];
            }
            let t = (*c - *a).cross(s) / den;
            vec![*a + r * t]
        }
        (Geom::Line { a, b }, Geom::Circle { c, r } | Geom::Arc { c, r, .. })
        | (Geom::Circle { c, r } | Geom::Arc { c, r, .. }, Geom::Line { a, b }) => {
            let d = (*b - *a).norm();
            let t0 = (*c - *a).dot(d);
            let foot = *a + d * t0;
            let fd = foot.dist(*c);
            let h2 = r * r - fd * fd;
            if h2 < -1e-12 {
                return vec![];
            }
            let h = h2.max(0.0).sqrt();
            if h < 1e-12 {
                vec![foot]
            } else {
                vec![foot - d * h, foot + d * h]
            }
        }
        (
            Geom::Circle { c: c1, r: r1 } | Geom::Arc { c: c1, r: r1, .. },
            Geom::Circle { c: c2, r: r2 } | Geom::Arc { c: c2, r: r2, .. },
        ) => {
            let dist = c1.dist(*c2);
            if dist < 1e-12 || dist > r1 + r2 + 1e-12 || dist < (r1 - r2).abs() - 1e-12 {
                return vec![];
            }
            let a = (r1 * r1 - r2 * r2 + dist * dist) / (2.0 * dist);
            let h = (r1 * r1 - a * a).max(0.0).sqrt();
            let u = (*c2 - *c1) / dist;
            let base = *c1 + u * a;
            if h < 1e-12 {
                vec![base]
            } else {
                vec![base + u.perp() * h, base - u.perp() * h]
            }
        }
        _ => vec![],
    }
}

/// Parameter of point `p` along geometry `g`: 0..1 on a line, an angle on a curve
/// (relative to the arc's start).
fn param(g: &Geom, p: V2) -> f64 {
    match *g {
        Geom::Line { a, b } => {
            let d = b - a;
            (p - a).dot(d) / d.dot(d)
        }
        Geom::Circle { c, .. } => wrap_positive((p - c).angle()),
        Geom::Arc { c, start, .. } => wrap_positive((p - c).angle() - start),
        Geom::Point { .. } => 0.0,
    }
}
fn on_edge(g: &Geom, p: V2) -> bool {
    match *g {
        Geom::Line { .. } => {
            let t = param(g, p);
            (-1e-9..=1.0 + 1e-9).contains(&t)
        }
        Geom::Circle { .. } => true,
        Geom::Arc { start, end, .. } => param(g, p) <= end - start + 1e-9,
        Geom::Point { .. } => false,
    }
}

/// Crossings of geometry `id` with every other edge, as (parameter on `id`, point,
/// other geometry).
fn crossings(s: &Sketch, id: i32) -> Vec<(f64, V2, i32)> {
    let Some(g) = s.geom(id) else { return vec![] };
    let mut out = Vec::new();
    for (other, h) in s.geos.iter().enumerate() {
        let other = other as i32;
        if other == id || !h.geom.is_edge() {
            continue;
        }
        for p in intersect_full(&g, &h.geom) {
            if on_edge(&g, p) && on_edge(&h.geom, p) {
                out.push((param(&g, p), p, other));
            }
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// How a new endpoint sits on the geometry that cut it: coincident with one of its
/// points when it lands there, on the edge otherwise.
fn attach(s: &mut Sketch, geo: i32, pos: Pos, other: i32, at: V2) {
    let Some(h) = s.geom(other) else { return };
    for p in h.points() {
        if p != Pos::Center && h.point(p).is_some_and(|q| q.dist(at) < 1e-7) {
            push(s, coincident(geo, pos, other, p));
            return;
        }
    }
    push(
        s,
        Constraint::new(T::PointOnObject, geo, pos).with_second(other, Pos::None),
    );
}

/// Drop constraints that pin a point of `geo` that has moved.
fn release(s: &mut Sketch, geo: i32, pos: Pos) {
    s.constraints.retain(|c| {
        !((c.first == geo && c.first_pos == pos)
            || (c.second == geo && c.second_pos == pos)
            || (c.third == geo && c.third_pos == pos))
    });
}

/// Trim the piece of edge `id` around `at` back to its nearest crossings, the way
/// FreeCAD's trim tool does: no crossing deletes the edge, one shortens it, two either
/// shorten it or split it in two. Returns a description of what happened.
pub fn trim(s: &mut Sketch, id: i32, at: V2) -> Result<String, String> {
    let g = s
        .geom(id)
        .filter(|g| g.is_edge())
        .ok_or("pick an edge to trim")?;
    let cuts = crossings(s, id);
    let t = param(&g, at);
    match g {
        Geom::Line { a, b } => {
            let inner: Vec<_> = cuts
                .iter()
                .filter(|c| c.0 > 1e-7 && c.0 < 1.0 - 1e-7)
                .collect();
            let before = inner.iter().rev().find(|c| c.0 < t).copied().copied();
            let after = inner.iter().find(|c| c.0 > t).copied().copied();
            match (before, after) {
                (None, None) => {
                    s.delete_geos(&[id]);
                    Ok("Deleted the edge: nothing crosses it".into())
                }
                (None, Some((_, p, other))) => {
                    set_geom(s, id, Geom::Line { a: p, b });
                    release(s, id, Pos::Start);
                    attach(s, id, Pos::Start, other, p);
                    Ok("Trimmed the start of the line".into())
                }
                (Some((_, p, other)), None) => {
                    set_geom(s, id, Geom::Line { a, b: p });
                    release(s, id, Pos::End);
                    attach(s, id, Pos::End, other, p);
                    Ok("Trimmed the end of the line".into())
                }
                (Some((_, p, o1)), Some((_, q, o2))) => {
                    let construction = s.geo(id).is_some_and(|g| g.construction);
                    set_geom(s, id, Geom::Line { a, b: p });
                    let new = s.add_geo(Geom::Line { a: q, b }, construction);
                    move_end(s, id, Pos::End, new, Pos::End);
                    attach(s, id, Pos::End, o1, p);
                    attach(s, new, Pos::Start, o2, q);
                    Ok("Split the line at its crossings".into())
                }
            }
        }
        Geom::Circle { c, r } => {
            if cuts.len() < 2 {
                s.delete_geos(&[id]);
                return Ok("Deleted the circle: it needs two crossings to trim".into());
            }
            let before = cuts
                .iter()
                .rev()
                .find(|x| x.0 < t)
                .or(cuts.last())
                .copied()
                .unwrap();
            let after = cuts
                .iter()
                .find(|x| x.0 > t)
                .or(cuts.first())
                .copied()
                .unwrap();
            // Keep the arc from `after` round to `before`.
            let start = after.0;
            let mut end = before.0;
            while end <= start {
                end += TAU;
            }
            set_geom(s, id, Geom::Arc { c, r, start, end });
            attach(s, id, Pos::Start, after.2, after.1);
            attach(s, id, Pos::End, before.2, before.1);
            Ok("Trimmed the circle to an arc".into())
        }
        Geom::Arc { c, r, start, end } => {
            let sweep = end - start;
            let inner: Vec<_> = cuts
                .iter()
                .filter(|x| x.0 > 1e-7 && x.0 < sweep - 1e-7)
                .collect();
            let before = inner.iter().rev().find(|x| x.0 < t).copied().copied();
            let after = inner.iter().find(|x| x.0 > t).copied().copied();
            match (before, after) {
                (None, None) => {
                    s.delete_geos(&[id]);
                    Ok("Deleted the arc: nothing crosses it".into())
                }
                (None, Some((pt, p, other))) => {
                    set_geom(
                        s,
                        id,
                        Geom::Arc {
                            c,
                            r,
                            start: start + pt,
                            end,
                        },
                    );
                    release(s, id, Pos::Start);
                    attach(s, id, Pos::Start, other, p);
                    Ok("Trimmed the start of the arc".into())
                }
                (Some((pt, p, other)), None) => {
                    set_geom(
                        s,
                        id,
                        Geom::Arc {
                            c,
                            r,
                            start,
                            end: start + pt,
                        },
                    );
                    release(s, id, Pos::End);
                    attach(s, id, Pos::End, other, p);
                    Ok("Trimmed the end of the arc".into())
                }
                (Some((p0, p, o1)), Some((p1, q, o2))) => {
                    let construction = s.geo(id).is_some_and(|g| g.construction);
                    set_geom(
                        s,
                        id,
                        Geom::Arc {
                            c,
                            r,
                            start,
                            end: start + p0,
                        },
                    );
                    let new = s.add_geo(
                        Geom::Arc {
                            c,
                            r,
                            start: start + p1,
                            end,
                        },
                        construction,
                    );
                    move_end(s, id, Pos::End, new, Pos::End);
                    attach(s, id, Pos::End, o1, p);
                    attach(s, new, Pos::Start, o2, q);
                    push(s, coincident(new, Pos::Center, id, Pos::Center));
                    push(
                        s,
                        Constraint::new(T::Equal, id, Pos::None).with_second(new, Pos::None),
                    );
                    Ok("Split the arc at its crossings".into())
                }
            }
        }
        Geom::Point { .. } => Err("pick an edge to trim".into()),
    }
}

fn set_geom(s: &mut Sketch, id: i32, g: Geom) {
    if let Some(geo) = s.geos.get_mut(id as usize) {
        geo.geom = g;
    }
}
/// Re-point constraints on one endpoint to another geometry's endpoint.
fn move_end(s: &mut Sketch, from: i32, from_pos: Pos, to: i32, to_pos: Pos) {
    for c in &mut s.constraints {
        if c.first == from && c.first_pos == from_pos {
            c.first = to;
            c.first_pos = to_pos;
        }
        if c.second == from && c.second_pos == from_pos {
            c.second = to;
            c.second_pos = to_pos;
        }
        if c.third == from && c.third_pos == from_pos {
            c.third = to;
            c.third_pos = to_pos;
        }
    }
}

/// Extend (or shorten) the end of `id` nearest `near` until it meets `boundary`.
pub fn extend(s: &mut Sketch, id: i32, near: V2, boundary: i32) -> Result<String, String> {
    let g = s.geom(id).ok_or("pick an edge to extend")?;
    let h = s
        .geom(boundary)
        .filter(|h| h.is_edge())
        .ok_or("pick the edge to extend to")?;
    if id == boundary {
        return Err("an edge cannot be extended to itself".into());
    }
    match g {
        Geom::Line { a, b } => {
            let end = if near.dist(a) < near.dist(b) {
                Pos::Start
            } else {
                Pos::End
            };
            let (fixed, moving) = if end == Pos::Start { (b, a) } else { (a, b) };
            let dir = (moving - fixed).norm();
            let far = Geom::Line {
                a: fixed,
                b: fixed + dir,
            };
            let hit = intersect_full(&far, &h)
                .into_iter()
                .filter(|p| on_edge(&h, *p) && (*p - fixed).dot(dir) > 1e-9)
                .min_by(|p, q| p.dist(moving).total_cmp(&q.dist(moving)))
                .ok_or("the line never meets that edge")?;
            let new = if end == Pos::Start {
                Geom::Line { a: hit, b }
            } else {
                Geom::Line { a, b: hit }
            };
            set_geom(s, id, new);
            release(s, id, end);
            attach(s, id, end, boundary, hit);
            Ok("Extended the line".into())
        }
        Geom::Arc { c, r, start, end } => {
            let ps = c + V2::polar(start, r);
            let pe = c + V2::polar(end, r);
            let which = if near.dist(ps) < near.dist(pe) {
                Pos::Start
            } else {
                Pos::End
            };
            let full = Geom::Circle { c, r };
            let hits: Vec<V2> = intersect_full(&full, &h)
                .into_iter()
                .filter(|p| on_edge(&h, *p))
                .collect();
            // Grow the arc the shortest way round to a crossing outside it.
            let mut best: Option<(f64, V2)> = None;
            for p in hits {
                let ang = (p - c).angle();
                let grow = if which == Pos::End {
                    wrap_positive(ang - end)
                } else {
                    wrap_positive(start - ang)
                };
                if grow > 1e-9 && grow < TAU - (end - start) && best.is_none_or(|b| grow < b.0) {
                    best = Some((grow, p));
                }
            }
            let (grow, p) = best.ok_or("the arc never meets that edge")?;
            let new = if which == Pos::End {
                Geom::Arc {
                    c,
                    r,
                    start,
                    end: end + grow,
                }
            } else {
                Geom::Arc {
                    c,
                    r,
                    start: start - grow,
                    end,
                }
            };
            set_geom(s, id, new.with_params(&new.params()));
            release(s, id, which);
            attach(s, id, which, boundary, p);
            Ok("Extended the arc".into())
        }
        _ => Err("only lines and arcs can be extended".into()),
    }
}

/// Round the corner where two lines meet (at `geo`'s `pos`) with an arc of radius `r`,
/// tangent to both, trimming the lines back to it.
pub fn fillet(s: &mut Sketch, geo: i32, pos: Pos, r: f64) -> Result<i32, String> {
    if r.is_nan() || r <= 0.0 {
        return Err("a fillet needs a positive radius".into());
    }
    let corner = s.point(geo, pos).ok_or("pick the corner of two lines")?;
    // The other line meeting here, by a coincident constraint or by position.
    let partner = s
        .constraints
        .iter()
        .find_map(|c| {
            if c.kind != T::Coincident {
                return None;
            }
            if c.first == geo && c.first_pos == pos {
                Some((c.second, c.second_pos))
            } else if c.second == geo && c.second_pos == pos {
                Some((c.first, c.first_pos))
            } else {
                None
            }
        })
        .or_else(|| {
            s.geos.iter().enumerate().find_map(|(i, g)| {
                let i = i as i32;
                if i == geo {
                    return None;
                }
                [Pos::Start, Pos::End]
                    .into_iter()
                    .find(|p| g.geom.point(*p).is_some_and(|q| q.dist(corner) < 1e-7))
                    .map(|p| (i, p))
            })
        })
        .ok_or("no second line meets this corner")?;
    let (g2, p2) = partner;
    let (Some(Geom::Line { a: a1, b: b1 }), Some(Geom::Line { a: a2, b: b2 })) =
        (s.geom(geo), s.geom(g2))
    else {
        return Err("a sketch fillet joins two lines".into());
    };
    let far1 = if pos == Pos::Start { b1 } else { a1 };
    let far2 = if p2 == Pos::Start { b2 } else { a2 };
    let (u1, u2) = ((far1 - corner).norm(), (far2 - corner).norm());
    let cos = u1.dot(u2).clamp(-1.0, 1.0);
    let theta = crate::math::acos(cos);
    if theta < 1e-6 || PI - theta < 1e-6 {
        return Err("the lines are parallel".into());
    }
    let t = r / crate::math::tan(theta / 2.0);
    if t >= corner.dist(far1) || t >= corner.dist(far2) {
        return Err("the radius is too large for these lines".into());
    }
    let (t1, t2) = (corner + u1 * t, corner + u2 * t);
    let bis = (u1 + u2).norm();
    let center = corner + bis * (r / crate::math::sin(theta / 2.0));
    let (ang1, ang2) = ((t1 - center).angle(), (t2 - center).angle());
    // Counter-clockwise the short way: from whichever tangent point leads.
    let (start_ang, end_ang, first_is_1) = if wrap_positive(ang2 - ang1) < PI {
        (ang1, ang2, true)
    } else {
        (ang2, ang1, false)
    };
    let s0 = wrap_positive(start_ang);
    let arc = Geom::Arc {
        c: center,
        r,
        start: s0,
        end: s0 + wrap_positive(end_ang - start_ang),
    };
    // Move the corner ends to the tangent points; the old coincidence goes.
    s.constraints.retain(|c| {
        !(c.kind == T::Coincident
            && ((c.first == geo && c.first_pos == pos && c.second == g2 && c.second_pos == p2)
                || (c.first == g2 && c.first_pos == p2 && c.second == geo && c.second_pos == pos)))
    });
    release(s, geo, pos);
    release(s, g2, p2);
    let with_end = |g: Geom, p: Pos, q: V2| match g {
        Geom::Line { a: _, b } if p == Pos::Start => Geom::Line { a: q, b },
        Geom::Line { a, .. } => Geom::Line { a, b: q },
        other => other,
    };
    let l1 = s.geom(geo).unwrap();
    let l2 = s.geom(g2).unwrap();
    set_geom(s, geo, with_end(l1, pos, t1));
    set_geom(s, g2, with_end(l2, p2, t2));
    let construction = s.geo(geo).is_some_and(|g| g.construction);
    let id = s.add_geo(arc, construction);
    let (on1, on2) = if first_is_1 {
        (Pos::Start, Pos::End)
    } else {
        (Pos::End, Pos::Start)
    };
    push(
        s,
        Constraint::new(T::Tangent, geo, pos).with_second(id, on1),
    );
    push(s, Constraint::new(T::Tangent, g2, p2).with_second(id, on2));
    Ok(id)
}

/// Nearest pickable thing to `q` within `tol`: a point first, then an edge.
pub fn pick(s: &Sketch, q: V2, tol: f64) -> Option<(i32, Pos)> {
    let mut best: Option<(f64, i32, Pos)> = None;
    for (i, g) in s.geos.iter().enumerate() {
        for pos in g.geom.points() {
            if let Some(p) = g.geom.point(pos) {
                let d = p.dist(q);
                if d <= tol && best.is_none_or(|b| d < b.0) {
                    best = Some((d, i as i32, pos));
                }
            }
        }
    }
    // The root point.
    if q.len() <= tol && best.is_none_or(|b| q.len() < b.0) {
        best = Some((q.len(), super::H_AXIS, Pos::Start));
    }
    if let Some((_, g, p)) = best {
        return Some((g, p));
    }
    let mut edge: Option<(f64, i32)> = None;
    for (i, g) in s.geos.iter().enumerate() {
        if !g.geom.is_edge() {
            continue;
        }
        let d = g.geom.distance(q);
        if d <= tol && edge.is_none_or(|b| d < b.0) {
            edge = Some((d, i as i32));
        }
    }
    edge.map(|(_, g)| (g, Pos::None)).or_else(|| {
        if q.y.abs() <= tol {
            Some((super::H_AXIS, Pos::None))
        } else if q.x.abs() <= tol {
            Some((super::V_AXIS, Pos::None))
        } else {
            None
        }
    })
}

/// A dimension for whatever is selected, seeded from the geometry as it stands:
/// FreeCAD's Constrain Distance / Horizontal / Vertical distance commands.
pub fn dimension(s: &Sketch, kind: T, picks: &[(i32, Pos)]) -> Result<Constraint, String> {
    let mut c = match (kind, picks) {
        (T::Distance, [(g, Pos::None)]) => Constraint::new(T::Distance, *g, Pos::None),
        (T::Distance, [(g1, p1), (g2, p2)]) if *p1 != Pos::None && *p2 != Pos::None => {
            Constraint::new(T::Distance, *g1, *p1).with_second(*g2, *p2)
        }
        (T::Distance, [(g1, p1), (g2, Pos::None)]) if *p1 != Pos::None => {
            Constraint::new(T::Distance, *g1, *p1).with_second(*g2, Pos::None)
        }
        (T::Distance, [(g2, Pos::None), (g1, p1)]) if *p1 != Pos::None => {
            Constraint::new(T::Distance, *g1, *p1).with_second(*g2, Pos::None)
        }
        (T::DistanceX | T::DistanceY, [(g, Pos::None)]) => Constraint::new(kind, *g, Pos::None),
        (T::DistanceX | T::DistanceY, [(g, p)]) => Constraint::new(kind, *g, *p),
        (T::DistanceX | T::DistanceY, [(g1, p1), (g2, p2)])
            if *p1 != Pos::None && *p2 != Pos::None =>
        {
            Constraint::new(kind, *g1, *p1).with_second(*g2, *p2)
        }
        (T::Radius | T::Diameter, [(g, Pos::None)]) => Constraint::new(kind, *g, Pos::None),
        (T::Angle, [(g, Pos::None)]) => Constraint::new(T::Angle, *g, Pos::None),
        (T::Angle, [(g1, Pos::None), (g2, Pos::None)]) => {
            Constraint::new(T::Angle, *g1, Pos::None).with_second(*g2, Pos::None)
        }
        _ => {
            return Err(format!(
                "select suitable geometry for a {} constraint",
                kind.label().to_lowercase()
            ))
        }
    };
    if c.second == GEO_UNDEF
        && matches!(kind, T::DistanceX | T::DistanceY)
        && c.first_pos != Pos::None
        && c.first < 0
    {
        return Err("the origin is always at zero".into());
    }
    let mut v = super::solver::measure(s, &c)?;
    // Horizontal and vertical distances read left-to-right / bottom-to-top, as in
    // FreeCAD: swap the points (or the line's ends) rather than store a negative.
    if matches!(kind, T::DistanceX | T::DistanceY) && v < 0.0 {
        if c.second != GEO_UNDEF {
            std::mem::swap(&mut c.first, &mut c.second);
            std::mem::swap(&mut c.first_pos, &mut c.second_pos);
            v = -v;
        } else if c.first_pos == Pos::None {
            // A line measured from its start: keep the sign, the solver honours it.
        }
    }
    c.value = v;
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::SolveStatus;
    #[test]
    fn trimming_a_line_between_two_crossings_splits_it() {
        let mut s = Sketch::default();
        let l = line(&mut s, v2(0.0, 0.0), v2(10.0, 0.0), false, false).unwrap();
        line(&mut s, v2(3.0, -1.0), v2(3.0, 1.0), false, false).unwrap();
        line(&mut s, v2(7.0, -1.0), v2(7.0, 1.0), false, false).unwrap();
        trim(&mut s, l, v2(5.0, 0.0)).unwrap();
        assert_eq!(s.geos.len(), 4);
        assert_eq!(
            s.geom(l),
            Some(Geom::Line {
                a: v2(0.0, 0.0),
                b: v2(3.0, 0.0)
            })
        );
        assert_eq!(
            s.geom(3),
            Some(Geom::Line {
                a: v2(7.0, 0.0),
                b: v2(10.0, 0.0)
            })
        );
        assert!(s.solve().solved());
    }
    #[test]
    fn trimming_a_circle_leaves_an_arc_and_extend_reaches_a_boundary() {
        let mut s = Sketch::default();
        let c = circle(&mut s, V2::ZERO, 5.0, false).unwrap();
        line(&mut s, v2(-10.0, 0.0), v2(10.0, 0.0), false, false).unwrap();
        trim(&mut s, c, v2(0.0, -5.0)).unwrap();
        let Some(Geom::Arc { start, end, .. }) = s.geom(c) else {
            panic!("not an arc")
        };
        assert!((end - start - PI).abs() < 1e-9, "kept the upper half");
        let l = line(&mut s, v2(20.0, 1.0), v2(20.0, 2.0), false, false).unwrap();
        let b = line(&mut s, v2(15.0, 10.0), v2(25.0, 10.0), false, false).unwrap();
        extend(&mut s, l, v2(20.0, 2.0), b).unwrap();
        assert_eq!(s.point(l, Pos::End), Some(v2(20.0, 10.0)));
    }
    #[test]
    fn a_fillet_is_tangent_to_both_lines() {
        let mut s = Sketch::default();
        let ids = rectangle(&mut s, v2(0.0, 0.0), v2(20.0, 10.0), false).unwrap();
        let arc = fillet(&mut s, ids[0], Pos::End, 2.0).unwrap();
        let report = s.solve();
        assert!(report.solved(), "{}", report.message());
        let Some(Geom::Arc { c, r, .. }) = s.geom(arc) else {
            panic!()
        };
        assert!((r - 2.0).abs() < 1e-9);
        assert!((c - v2(18.0, 2.0)).len() < 1e-9);
        assert_eq!(
            s.point(ids[0], Pos::End)
                .map(|p| (p - v2(18.0, 0.0)).len() < 1e-9),
            Some(true)
        );
        assert_ne!(report.status, SolveStatus::Conflicting);
    }
    #[test]
    fn three_point_arc_passes_through_its_points() {
        let mut s = Sketch::default();
        let id = arc_3pt(&mut s, v2(1.0, 0.0), v2(0.0, 1.0), v2(-1.0, 0.0), false).unwrap();
        let g = s.geom(id).unwrap();
        assert!(g.distance(v2(0.0, 1.0)) < 1e-12);
        assert!(g.distance(v2(0.0, -1.0)) > 0.5, "went the wrong way round");
        let id = arc_3pt(&mut s, v2(1.0, 0.0), v2(0.0, -1.0), v2(-1.0, 0.0), false).unwrap();
        assert!(s.geom(id).unwrap().distance(v2(0.0, -1.0)) < 1e-12);
    }
}
