//! Closed profiles from a sketch: its (non-construction) edges chained into wires, the
//! wires nested into faces with holes, as Part Design's Pad and Pocket consume them.
use super::{Geom, Sketch};
use crate::math::{TAU, V2};

/// Segments per full turn when a curve becomes a polygon. Volumes of revolved and
/// extruded curves are therefore those of the inscribed polygon.
pub const SEGMENTS: usize = 64;

/// One closed wire as a polygon. `edge_geo[i]` is the sketch geometry the segment from
/// `pts[i]` to `pts[i + 1]` came from, so faces built on it know their surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    pub pts: Vec<V2>,
    pub edge_geo: Vec<i32>,
}
impl Wire {
    pub fn area(&self) -> f64 {
        signed_area(&self.pts)
    }
    pub(crate) fn reverse(&mut self) {
        // Segment k of the reversed wire joins old points n-1-k and n-2-k, which was old
        // segment n-2-k; the closing segment joins old points 0 and n-1 either way.
        let n = self.pts.len();
        let old = self.edge_geo.clone();
        self.pts.reverse();
        self.edge_geo = (0..n)
            .map(|k| {
                if k + 1 < n {
                    old[n - 2 - k]
                } else {
                    old[n - 1]
                }
            })
            .collect();
    }
}
/// A face to extrude: a counter-clockwise outer wire and clockwise holes.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub outer: Wire,
    pub holes: Vec<Wire>,
}
impl Region {
    pub fn area(&self) -> f64 {
        self.outer.area() + self.holes.iter().map(Wire::area).sum::<f64>()
    }
}

pub fn signed_area(pts: &[V2]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i].cross(pts[(i + 1) % n])).sum::<f64>() / 2.0
}
pub fn point_in_polygon(p: V2, pts: &[V2]) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (pts[i], pts[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}
fn segments_cross(a: V2, b: V2, c: V2, d: V2) -> bool {
    let d1 = (b - a).cross(c - a);
    let d2 = (b - a).cross(d - a);
    let d3 = (d - c).cross(a - c);
    let d4 = (d - c).cross(b - c);
    let eps = 1e-9 * (1.0 + (b - a).len() * (d - c).len());
    ((d1 > eps && d2 < -eps) || (d1 < -eps && d2 > eps))
        && ((d3 > eps && d4 < -eps) || (d3 < -eps && d4 > eps))
}

/// Points along an edge from one end to the other (excluding the last point).
fn edge_points(g: &Geom, forward: bool) -> Vec<V2> {
    let mut pts = match *g {
        Geom::Line { a, b } => vec![a, b],
        Geom::Arc { c, r, start, end } => {
            let n = (((end - start) / TAU * SEGMENTS as f64).ceil() as usize).max(2);
            (0..=n)
                .map(|i| c + V2::polar(start + (end - start) * i as f64 / n as f64, r))
                .collect()
        }
        _ => vec![],
    };
    if !forward {
        pts.reverse();
    }
    pts.pop();
    pts
}

/// The sketch's closed wires nested into faces. Errors name the problem the way
/// Part Design reports it.
pub fn regions(s: &Sketch) -> Result<Vec<Region>, String> {
    let mut wires: Vec<Wire> = Vec::new();
    // Every open edge, by id, with its two end points.
    let mut open: Vec<(i32, V2, V2)> = Vec::new();
    for (i, g) in s.geos.iter().enumerate() {
        if g.construction {
            continue;
        }
        match g.geom {
            Geom::Circle { c, r } => {
                let pts: Vec<V2> = (0..SEGMENTS)
                    .map(|k| c + V2::polar(TAU * k as f64 / SEGMENTS as f64, r))
                    .collect();
                wires.push(Wire {
                    edge_geo: vec![i as i32; pts.len()],
                    pts,
                });
            }
            Geom::Line { a, b } => open.push((i as i32, a, b)),
            Geom::Arc { .. } => {
                let (a, b) = (
                    g.geom.point(super::Pos::Start).unwrap(),
                    g.geom.point(super::Pos::End).unwrap(),
                );
                open.push((i as i32, a, b));
            }
            Geom::Point { .. } => {}
        }
    }
    let scale = s
        .bounds()
        .map(|(lo, hi)| (hi - lo).len())
        .unwrap_or(1.0)
        .max(1.0);
    let tol = 1e-7 * scale;
    let mut used = vec![false; open.len()];
    for start in 0..open.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (id0, a0, b0) = open[start];
        let mut pts = edge_points(&s.geos[id0 as usize].geom, true);
        let mut edge_geo = vec![id0; pts.len()];
        let mut end = b0;
        loop {
            if end.dist(a0) <= tol {
                break;
            }
            let mut next = None;
            for (k, (id, a, b)) in open.iter().enumerate() {
                if used[k] {
                    continue;
                }
                if a.dist(end) <= tol {
                    if next.is_some() {
                        return Err(
                            "The sketch has a wire that branches: three edges meet at one point"
                                .into(),
                        );
                    }
                    next = Some((k, *id, true, *b));
                } else if b.dist(end) <= tol {
                    if next.is_some() {
                        return Err(
                            "The sketch has a wire that branches: three edges meet at one point"
                                .into(),
                        );
                    }
                    next = Some((k, *id, false, *a));
                }
            }
            let Some((k, id, forward, far)) = next else {
                return Err("Wire is not closed: the sketch has an open end".into());
            };
            used[k] = true;
            let more = edge_points(&s.geos[id as usize].geom, forward);
            edge_geo.extend(std::iter::repeat_n(id, more.len()));
            pts.extend(more);
            end = far;
        }
        if pts.len() < 3 {
            return Err("A wire in the sketch encloses no area".into());
        }
        wires.push(Wire { pts, edge_geo });
    }
    if wires.is_empty() {
        return Err("The sketch has no closed profile".into());
    }
    // No wire may cross itself or another.
    for (wi, w) in wires.iter().enumerate() {
        if signed_area(&w.pts).abs() < 1e-12 * scale * scale {
            return Err("A wire in the sketch encloses no area".into());
        }
        for (vi, v) in wires.iter().enumerate().skip(wi) {
            let (n, m) = (w.pts.len(), v.pts.len());
            for i in 0..n {
                let (a, b) = (w.pts[i], w.pts[(i + 1) % n]);
                for j in 0..m {
                    if wi == vi && (i == j || (i + 1) % n == j || (j + 1) % n == i) {
                        continue;
                    }
                    if segments_cross(a, b, v.pts[j], v.pts[(j + 1) % m]) {
                        return Err(if wi == vi {
                            "A wire in the sketch intersects itself".into()
                        } else {
                            "Wires in the sketch intersect each other".into()
                        });
                    }
                }
            }
        }
    }
    // Nesting depth: how many other wires contain each one.
    let depth: Vec<usize> = wires
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let probe = inner_point(w);
            wires
                .iter()
                .enumerate()
                .filter(|(j, v)| *j != i && point_in_polygon(probe, &v.pts))
                .count()
        })
        .collect();
    let mut regions: Vec<(usize, Region)> = Vec::new();
    for (i, w) in wires.iter().enumerate() {
        if depth[i].is_multiple_of(2) {
            let mut outer = w.clone();
            if outer.area() < 0.0 {
                outer.reverse();
            }
            regions.push((
                i,
                Region {
                    outer,
                    holes: vec![],
                },
            ));
        }
    }
    for (i, w) in wires.iter().enumerate() {
        if depth[i] % 2 == 1 {
            // The hole belongs to the containing outer wire one level up.
            let probe = inner_point(w);
            let parent = regions
                .iter_mut()
                .find(|(j, r)| depth[*j] + 1 == depth[i] && point_in_polygon(probe, &r.outer.pts))
                .ok_or("A hole in the sketch has no outer wire")?;
            let mut hole = w.clone();
            if hole.area() > 0.0 {
                hole.reverse();
            }
            parent.1.holes.push(hole);
        }
    }
    Ok(regions.into_iter().map(|(_, r)| r).collect())
}

/// The sketch's profile as exact wires (lines and arcs) for the solid kernel: the same
/// faces [`regions`] finds, each polygon run of one geometry turned back into that
/// geometry, traversed the wire's way.
pub fn exact_regions(s: &Sketch) -> Result<Vec<crate::brep::build::Region2>, String> {
    use crate::brep::build::{Region2, Wire2};
    let exact = |w: &Wire| -> Wire2 {
        let n = w.pts.len();
        let start = (0..n).find(|&i| w.edge_geo[(i + n - 1) % n] != w.edge_geo[i]);
        let Some(start) = start else {
            // One geometry all round: a circle.
            let g = &s.geos[w.edge_geo[0] as usize].geom;
            let (c, r) = match *g {
                Geom::Circle { c, r } => (c, r),
                Geom::Arc { c, r, .. } => (c, r),
                _ => (w.pts[0], 0.0),
            };
            let sweep = if w.area() >= 0.0 { TAU } else { -TAU };
            return Wire2 {
                segs: vec![crate::brep::build::Seg2::Arc {
                    c,
                    r,
                    a0: 0.0,
                    sweep,
                }],
            };
        };
        let mut segs = Vec::new();
        let mut i = 0;
        while i < n {
            let k = (start + i) % n;
            let id = w.edge_geo[k];
            let mut len = 1;
            while i + len < n && w.edge_geo[(start + i + len) % n] == id {
                len += 1;
            }
            let from = w.pts[k];
            let to = w.pts[(start + i + len) % n];
            let g = &s.geos[id as usize].geom;
            use crate::brep::build::Seg2;
            match *g {
                Geom::Line { a, b } => {
                    if from.dist(a) <= from.dist(b) {
                        segs.push(Seg2::Line(a, b));
                    } else {
                        segs.push(Seg2::Line(b, a));
                    }
                }
                Geom::Arc { c, r, start: a0, end: a1 } => {
                    let ps = c + V2::polar(a0, r);
                    if from.dist(ps) <= to.dist(ps) {
                        segs.push(Seg2::Arc {
                            c,
                            r,
                            a0,
                            sweep: a1 - a0,
                        });
                    } else {
                        segs.push(Seg2::Arc {
                            c,
                            r,
                            a0: a1,
                            sweep: a0 - a1,
                        });
                    }
                }
                _ => segs.push(Seg2::Line(from, to)),
            }
            i += len;
        }
        Wire2 { segs }
    };
    Ok(regions(s)?
        .iter()
        .map(|r| Region2 {
            outer: exact(&r.outer),
            holes: r.holes.iter().map(exact).collect(),
        })
        .collect())
}

/// A point strictly on the wire's boundary, nudged nowhere: containment of wires that
/// do not cross is decided by any of their vertices.
fn inner_point(w: &Wire) -> V2 {
    // Midpoint of the first segment avoids coinciding with another wire's vertex.
    w.pts[0].lerp(w.pts[1], 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v2;
    use crate::sketch::tools;
    #[test]
    fn a_rectangle_with_a_circle_inside_is_one_face_with_a_hole() {
        let mut s = Sketch::default();
        tools::rectangle(&mut s, v2(0.0, 0.0), v2(40.0, 20.0), false).unwrap();
        tools::circle(&mut s, v2(20.0, 10.0), 5.0, false).unwrap();
        let r = regions(&s).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].holes.len(), 1);
        assert!(r[0].outer.area() > 0.0 && r[0].holes[0].area() < 0.0);
        let poly_circle = 0.5 * SEGMENTS as f64 * 25.0 * (TAU / SEGMENTS as f64).sin();
        assert!((r[0].area() - (800.0 - poly_circle)).abs() < 1e-9);
    }
    #[test]
    fn open_and_crossing_wires_are_refused() {
        let mut s = Sketch::default();
        tools::polyline(
            &mut s,
            &[v2(0.0, 0.0), v2(10.0, 0.0), v2(10.0, 10.0)],
            false,
            false,
        )
        .unwrap();
        assert!(regions(&s).unwrap_err().contains("not closed"));
        let mut s = Sketch::default();
        tools::rectangle(&mut s, v2(0.0, 0.0), v2(10.0, 10.0), false).unwrap();
        tools::rectangle(&mut s, v2(5.0, 5.0), v2(15.0, 15.0), false).unwrap();
        assert!(regions(&s).unwrap_err().contains("intersect"));
    }
    #[test]
    fn reversing_keeps_each_segment_on_its_edge() {
        let mut w = Wire {
            pts: vec![v2(0.0, 0.0), v2(1.0, 0.0), v2(1.0, 1.0), v2(0.0, 1.0)],
            edge_geo: vec![10, 11, 12, 13],
        };
        w.reverse();
        // New segments: (0,1)->(1,1) was old 12, (1,1)->(1,0) old 11, (1,0)->(0,0) old 10,
        // closing (0,0)->(0,1) old 13.
        assert_eq!(w.pts[0], v2(0.0, 1.0));
        assert_eq!(w.edge_geo, vec![12, 11, 10, 13]);
    }
    #[test]
    fn slots_close_through_their_tangent_arcs() {
        let mut s = Sketch::default();
        tools::slot(&mut s, v2(0.0, 0.0), v2(20.0, 0.0), 4.0, false).unwrap();
        let r = regions(&s).unwrap();
        assert_eq!(r.len(), 1);
        let exact = 20.0 * 8.0 + std::f64::consts::PI * 16.0;
        assert!((r[0].area() - exact).abs() / exact < 2e-3);
    }
}
