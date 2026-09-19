//! Bézier paths: GIMP's Paths tool, Paint's Curve and Pinta's Line/Curve. A path is a
//! chain of anchors with a handle on each side; it flattens to a polyline (in exact
//! integer arithmetic) that is stroked or filled as a [`Shape`], or turned into a
//! selection.
use crate::draw::{Shape, ShapeKind};
use crate::mask::{Mask, P16};
use crate::Rgba;
use serde::{Deserialize, Serialize};

/// One point of a path, with the handles that shape the curve into and out of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub point: P16,
    /// The control point of the segment arriving here.
    pub cin: P16,
    /// The control point of the segment leaving here.
    pub cout: P16,
}
impl Anchor {
    /// A corner: both handles on the point, so its segments are straight.
    pub fn corner(point: P16) -> Self {
        Self {
            point,
            cin: point,
            cout: point,
        }
    }
    /// A smooth point whose outgoing handle is at `handle`, the other mirrored.
    pub fn smooth(point: P16, handle: P16) -> Self {
        Self {
            point,
            cin: (2 * point.0 - handle.0, 2 * point.1 - handle.1),
            cout: handle,
        }
    }
}

/// Points along the cubic `p0 p1 p2 p3`, both ends included, about one every four
/// pixels of its control polygon (at least one segment, at most 64).
pub fn cubic(p0: P16, p1: P16, p2: P16, p3: P16) -> Vec<P16> {
    let dist = |a: P16, b: P16| (a.0 - b.0).abs() + (a.1 - b.1).abs();
    let n = ((dist(p0, p1) + dist(p1, p2) + dist(p2, p3)) / 64).clamp(1, 64);
    let n3 = n * n * n;
    (0..=n)
        .map(|k| {
            let (s, t) = (n - k, k);
            let w = [s * s * s, 3 * s * s * t, 3 * s * t * t, t * t * t];
            let axis = |f: fn(P16) -> i64| {
                let v = w[0] * f(p0) + w[1] * f(p1) + w[2] * f(p2) + w[3] * f(p3);
                (v + n3 / 2).div_euclid(n3)
            };
            (axis(|p| p.0), axis(|p| p.1))
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Path {
    pub anchors: Vec<Anchor>,
    pub closed: bool,
}
impl Path {
    /// A smooth curve through `points` (a Catmull-Rom spline as cubic segments), as
    /// Pinta's Line/Curve draws through its control points.
    pub fn through(points: &[P16], closed: bool) -> Self {
        let n = points.len();
        let at = |i: isize| -> P16 {
            if closed {
                points[i.rem_euclid(n as isize) as usize]
            } else {
                points[i.clamp(0, n as isize - 1) as usize]
            }
        };
        let anchors = (0..n as isize)
            .map(|i| {
                let (prev, p, next) = (at(i - 1), at(i), at(i + 1));
                // Tangent (next - prev) / 2; Bézier handles a third of it either side.
                let (mx, my) = ((next.0 - prev.0) / 6, (next.1 - prev.1) / 6);
                Anchor {
                    point: p,
                    cin: (p.0 - mx, p.1 - my),
                    cout: (p.0 + mx, p.1 + my),
                }
            })
            .collect();
        Self { anchors, closed }
    }
    /// The path as a polyline; a closed path returns to its first point.
    pub fn flatten(&self) -> Vec<P16> {
        let a = &self.anchors;
        let mut out: Vec<P16> = a.first().map(|f| vec![f.point]).unwrap_or_default();
        let count = if self.closed && a.len() > 1 {
            a.len()
        } else {
            a.len().saturating_sub(1)
        };
        for i in 0..count {
            let (s, e) = (a[i], a[(i + 1) % a.len()]);
            out.extend(cubic(s.point, s.cout, e.cin, e.point).into_iter().skip(1));
        }
        out
    }
    /// A stroke of the path in `color`, `width` pixels wide.
    pub fn stroke(&self, color: Rgba, width: u32, antialias: bool) -> Shape {
        Shape {
            kind: ShapeKind::Polyline,
            points: self.flatten(),
            outline: Some(color),
            fill: None,
            width: width.max(1),
            antialias,
        }
    }
    /// The area the path encloses, filled with `color` (an open path closes on itself).
    pub fn fill(&self, color: Rgba, antialias: bool) -> Shape {
        Shape {
            kind: ShapeKind::Polygon,
            points: self.flatten(),
            outline: None,
            fill: Some(color),
            width: 1,
            antialias,
        }
    }
    /// The enclosed area as a selection.
    pub fn selection(&self, width: u32, height: u32) -> Mask {
        Mask::polygon(width, height, &self.flatten())
    }
    /// The anchor or handle within `radius` (sub16) of `p`: `(anchor, 0 point, 1 in,
    /// 2 out)`, nearest first, points before handles.
    pub fn hit(&self, p: P16, radius: i64) -> Option<(usize, u8)> {
        let d = |q: P16| (q.0 - p.0).abs().max((q.1 - p.1).abs());
        let mut best: Option<(i64, usize, u8)> = None;
        for (i, a) in self.anchors.iter().enumerate() {
            for (which, q) in [(0u8, a.point), (1, a.cin), (2, a.cout)] {
                let dist = d(q);
                if dist <= radius && best.is_none_or(|b| (dist, which) < (b.0, b.2)) {
                    best = Some((dist, i, which));
                }
            }
        }
        best.map(|(_, i, w)| (i, w))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Canvas, IRect, BLACK, WHITE};
    fn px(x: i64, y: i64) -> P16 {
        (x * 16 + 8, y * 16 + 8)
    }
    #[test]
    fn cubics_pass_through_their_ends_and_bend_toward_their_handles() {
        let c = cubic(px(0, 0), px(0, 20), px(20, 20), px(20, 0));
        assert_eq!(c.first(), Some(&px(0, 0)));
        assert_eq!(c.last(), Some(&px(20, 0)));
        // This symmetric arch peaks at 3/4 of its handles' height, half-way across.
        let peak = c.iter().map(|p| p.1).max().unwrap();
        assert!(peak <= px(0, 15).1 && peak > px(0, 14).1, "{peak}");
        let even = cubic((0, 0), (0, 640), (640, 640), (640, 0));
        assert_eq!(even.len() % 2, 1);
        assert_eq!(even[even.len() / 2], (320, 480));
        // A straight cubic stays on its line.
        for p in cubic(px(0, 5), px(0, 5), px(30, 5), px(30, 5)) {
            assert_eq!(p.1, px(0, 5).1);
        }
    }
    #[test]
    fn paths_stroke_fill_and_select() {
        let path = Path {
            anchors: vec![
                Anchor::corner(px(2, 2)),
                Anchor::corner(px(17, 2)),
                Anchor::corner(px(17, 12)),
                Anchor::corner(px(2, 12)),
            ],
            closed: true,
        };
        let flat = path.flatten();
        assert_eq!(
            flat.first(),
            flat.last(),
            "closed paths return to the start"
        );
        let mut c = Canvas::filled(20, 15, WHITE);
        path.stroke(BLACK, 1, false).draw(&mut c, None).unwrap();
        assert_eq!(c.get(10, 2), BLACK);
        assert_eq!(c.get(2, 7), BLACK);
        assert_eq!(c.get(10, 7), WHITE);
        let mut c = Canvas::filled(20, 15, WHITE);
        path.fill(BLACK, false).draw(&mut c, None).unwrap();
        assert_eq!(c.get(10, 7), BLACK);
        assert_eq!(c.get(1, 1), WHITE);
        let sel = path.selection(20, 15);
        // Edge pixels the outline half covers are partly selected.
        assert_eq!(sel.bounds(), Some(IRect::new(2, 2, 16, 11)));
        assert_eq!(sel.get(10, 7), 255);
        assert_eq!(path.hit(px(17, 3), 32), Some((1, 0)));
        assert_eq!(path.hit(px(9, 9), 16), None);
    }
    #[test]
    fn splines_run_through_every_control_point() {
        let pts = [px(0, 10), px(10, 0), px(20, 10), px(30, 0)];
        let path = Path::through(&pts, false);
        let flat = path.flatten();
        for p in pts {
            assert!(flat.contains(&p), "{p:?}");
        }
        let smooth = Anchor::smooth(px(5, 5), px(8, 5));
        assert_eq!(smooth.cin, px(2, 5));
    }
}
