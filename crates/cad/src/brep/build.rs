//! Solids from profiles: extrusions (Pad, Pocket, prismatic tools), revolutions
//! (Revolution, Groove, Hole, round tools) and primitives, built as exact B-reps with
//! OpenCascade's face order (side faces in wire order, then the bottom and top caps).
use super::geom::{Curve, Surface};
use super::mass;
use super::topo::{Coedge, Face, Solid, TOL};
use crate::math::{self, v2, Frame, Xform, TAU, V2, V3};

/// A piece of a planar profile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg2 {
    Line(V2, V2),
    /// From angle `a0` through the signed `sweep` (±2π for a full circle).
    Arc {
        c: V2,
        r: f64,
        a0: f64,
        sweep: f64,
    },
}
impl Seg2 {
    pub fn start(&self) -> V2 {
        match *self {
            Seg2::Line(a, _) => a,
            Seg2::Arc { c, r, a0, .. } => c + V2::polar(a0, r),
        }
    }
    pub fn end(&self) -> V2 {
        match *self {
            Seg2::Line(_, b) => b,
            Seg2::Arc { c, r, a0, sweep } => c + V2::polar(a0 + sweep, r),
        }
    }
    pub fn full(&self) -> bool {
        matches!(self, Seg2::Arc { sweep, .. } if sweep.abs() >= TAU - 1e-12)
    }
    pub fn reversed(&self) -> Seg2 {
        match *self {
            Seg2::Line(a, b) => Seg2::Line(b, a),
            Seg2::Arc { c, r, a0, sweep } => Seg2::Arc {
                c,
                r,
                a0: a0 + sweep,
                sweep: -sweep,
            },
        }
    }
    pub fn point(&self, s: f64) -> V2 {
        match *self {
            Seg2::Line(a, b) => a.lerp(b, s),
            Seg2::Arc { c, r, a0, sweep } => c + V2::polar(a0 + sweep * s, r),
        }
    }
}

/// A closed wire, material on its left.
#[derive(Clone, Debug, PartialEq)]
pub struct Wire2 {
    pub segs: Vec<Seg2>,
}
impl Wire2 {
    pub fn reversed(&self) -> Wire2 {
        Wire2 {
            segs: self.segs.iter().rev().map(|s| s.reversed()).collect(),
        }
    }
    /// Signed area (positive counter-clockwise), exact for arcs.
    pub fn area(&self) -> f64 {
        let mut a = 0.0;
        for s in &self.segs {
            match *s {
                Seg2::Line(p, q) => a += p.cross(q) / 2.0,
                Seg2::Arc { c, r, a0, sweep } => {
                    // ∮ (x dy − y dx)/2 over the arc.
                    let (p, q) = (s.start(), s.end());
                    let _ = (p, q);
                    let (s0, c0) = math::sin_cos(a0);
                    let (s1, c1) = math::sin_cos(a0 + sweep);
                    a += (r * r * sweep + c.x * r * (s1 - s0) - c.y * r * (c1 - c0)) / 2.0;
                }
            }
        }
        a
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Region2 {
    pub outer: Wire2,
    pub holes: Vec<Wire2>,
}

/// A 3D curve and parameter range for a profile segment placed by `f` at height `z`.
fn seg_curve(f: &Frame, s: &Seg2, z: f64) -> (Curve, f64, f64, bool) {
    let lift = f.z * z;
    match *s {
        Seg2::Line(a, b) => {
            let (pa, pb) = (f.to_world(a) + lift, f.to_world(b) + lift);
            let len = pa.dist(pb);
            (
                Curve::Line {
                    o: pa,
                    d: (pb - pa) / len,
                },
                0.0,
                len,
                false,
            )
        }
        Seg2::Arc { c, r, a0, sweep } => {
            let cf = Frame {
                origin: f.to_world(c) + lift,
                x: f.x,
                y: f.y,
                z: f.z,
            };
            // Curves run counter-clockwise; a clockwise arc is the reversed coedge.
            if sweep >= 0.0 {
                (Curve::Circle { f: cf, r }, a0, a0 + sweep, false)
            } else {
                (Curve::Circle { f: cf, r }, a0 + sweep, a0, true)
            }
        }
    }
}

/// Extrude profile regions (in `frame`'s plane) from `z0` to `z1` along its normal.
pub fn extrude(regions: &[Region2], frame: &Frame, z0: f64, z1: f64) -> Solid {
    let mut s = Solid::default();
    if z1 - z0 <= TOL {
        return s;
    }
    let n = frame.z;
    let mut sides: Vec<Face> = Vec::new();
    let mut bottoms: Vec<Face> = Vec::new();
    let mut tops: Vec<Face> = Vec::new();
    for region in regions {
        let mut bottom_loops = Vec::new();
        let mut top_loops = Vec::new();
        for w in std::iter::once(&region.outer).chain(region.holes.iter()) {
            let segs: Vec<Seg2> = w
                .segs
                .iter()
                .copied()
                .filter(|g| g.full() || g.start().dist(g.end()) > TOL)
                .collect();
            if segs.is_empty() {
                continue;
            }
            let k = segs.len();
            // Vertices at each segment's start, bottom and top.
            let mut vb = Vec::with_capacity(k);
            let mut vt = Vec::with_capacity(k);
            for g in &segs {
                let p = frame.to_world(g.start());
                vb.push(s.add_vertex(p + n * z0));
                vt.push(s.add_vertex(p + n * z1));
            }
            // Vertical edges.
            let mut ve = Vec::with_capacity(k);
            for i in 0..k {
                let p = s.vertices[vb[i]].p;
                ve.push(s.add_edge(Curve::Line { o: p, d: n }, 0.0, z1 - z0, vb[i], vt[i]));
            }
            let mut bl = Vec::new();
            let mut tl = Vec::new();
            for i in 0..k {
                let j = (i + 1) % k;
                let g = &segs[i];
                let (cb, t0, t1, rev) = seg_curve(frame, g, z0);
                let (ct, _, _, _) = seg_curve(frame, g, z1);
                let (a0, a1) = if rev { (vb[j], vb[i]) } else { (vb[i], vb[j]) };
                let (b0, b1) = if rev { (vt[j], vt[i]) } else { (vt[i], vt[j]) };
                let eb = s.add_edge(cb, t0, t1, a0, a1);
                let et = s.add_edge(ct, t0, t1, b0, b1);
                let surface = match *g {
                    Seg2::Line(a, b) => {
                        let (pa, pb) = (frame.to_world(a), frame.to_world(b));
                        let d = (pb - pa).norm();
                        Surface::Plane {
                            f: Frame {
                                origin: pa + n * z0,
                                x: d,
                                y: n,
                                z: d.cross(n).norm(),
                            },
                        }
                    }
                    Seg2::Arc { c, r, .. } => Surface::Cylinder {
                        f: Frame {
                            origin: frame.to_world(c) + n * z0,
                            x: frame.x,
                            y: frame.y,
                            z: n,
                        },
                        r,
                    },
                };
                // Bottom a→b, up at b, top b→a, down at a.
                sides.push(Face {
                    surface,
                    loops: vec![vec![
                        Coedge { edge: eb, rev },
                        Coedge {
                            edge: ve[j],
                            rev: false,
                        },
                        Coedge {
                            edge: et,
                            rev: !rev,
                        },
                        Coedge {
                            edge: ve[i],
                            rev: true,
                        },
                    ]],
                });
                bl.push(Coedge { edge: eb, rev });
                tl.push(Coedge { edge: et, rev });
            }
            // The bottom is seen from below: its loop runs the other way.
            let mut bl_rev: Vec<Coedge> = bl
                .into_iter()
                .rev()
                .map(|c| Coedge {
                    edge: c.edge,
                    rev: !c.rev,
                })
                .collect();
            if bl_rev.is_empty() {
                continue;
            }
            bl_rev.rotate_right(0);
            bottom_loops.push(bl_rev);
            top_loops.push(tl);
        }
        if bottom_loops.is_empty() {
            continue;
        }
        bottoms.push(Face {
            surface: Surface::Plane {
                f: Frame {
                    origin: frame.origin + n * z0,
                    x: frame.x,
                    y: -frame.y,
                    z: -n,
                },
            },
            loops: bottom_loops,
        });
        tops.push(Face {
            surface: Surface::Plane {
                f: Frame {
                    origin: frame.origin + n * z1,
                    x: frame.x,
                    y: frame.y,
                    z: n,
                },
            },
            loops: top_loops,
        });
    }
    s.faces = sides;
    s.faces.extend(bottoms);
    s.faces.extend(tops);
    s.renumber();
    s
}

/// Revolve profile regions (in `frame`'s plane) about the axis through `axis_o` along
/// `axis_d`, from `start` through `angle` radians.
pub fn revolve(
    regions: &[Region2],
    frame: &Frame,
    axis_o: V3,
    axis_d: V3,
    start: f64,
    angle: f64,
) -> Result<Solid, String> {
    if angle.abs() < 1e-9 {
        return Err("The revolution angle must not be zero".into());
    }
    let k = axis_d.norm();
    let full = angle.abs() >= TAU - 1e-9;
    let (base, sweep) = if full {
        (start, TAU)
    } else if angle < 0.0 {
        (start + angle, -angle)
    } else {
        (start, angle)
    };
    // Which side of the axis the profile is on.
    let side_dir = k.cross(frame.z);
    let mut side = 0.0f64;
    let mut any = None;
    for r in regions {
        for w in std::iter::once(&r.outer).chain(r.holes.iter()) {
            for g in &w.segs {
                for i in 0..=8 {
                    let q = frame.to_world(g.point(i as f64 / 8.0));
                    let sd = (q - axis_o).dot(side_dir);
                    if sd.abs() > 1e-7 {
                        if side != 0.0 && sd.signum() != side {
                            return Err("The profile crosses the revolution axis".into());
                        }
                        side = sd.signum();
                        any = Some(q);
                    }
                }
            }
        }
    }
    if side == 0.0 || any.is_none() {
        return Err("The profile lies on the revolution axis".into());
    }
    // Axis frame: x towards the profile, turned to the start angle.
    let x0 = (side_dir * side).norm();
    let rot = Xform::rotate(axis_o, k, base);
    let fx = rot.dir(x0).norm();
    let fy = k.cross(fx).norm();
    let axis_frame = |h: f64| Frame {
        origin: axis_o + k * h,
        x: fx,
        y: fy,
        z: k,
    };
    let place = |p: V2| rot.point(frame.to_world(p));
    let at_end = Xform::rotate(axis_o, k, sweep);
    let coords = |p: V3| {
        let d = p - axis_o;
        let h = d.dot(k);
        ((d - k * h).len(), h)
    };
    let mut s = Solid::default();
    let mut laterals: Vec<Face> = Vec::new();
    let mut caps: Vec<Face> = Vec::new();
    for region in regions {
        let mut start_loops = Vec::new();
        let mut end_loops = Vec::new();
        for w in std::iter::once(&region.outer).chain(region.holes.iter()) {
            let segs: Vec<Seg2> = w
                .segs
                .iter()
                .copied()
                .filter(|g| g.full() || g.start().dist(g.end()) > TOL)
                .collect();
            let n = segs.len();
            if n == 0 {
                continue;
            }
            // Vertices at each segment start, at 0 and at the end angle.
            let mut v0 = Vec::with_capacity(n);
            let mut v1 = Vec::with_capacity(n);
            let mut on_axis = Vec::with_capacity(n);
            for g in &segs {
                let p = place(g.start());
                let (rho, _) = coords(p);
                let axis = rho < 1e-9 * (1.0 + p.len());
                on_axis.push(axis);
                let a = s.add_vertex(p);
                v0.push(a);
                v1.push(if full || axis {
                    a
                } else {
                    s.add_vertex(at_end.point(p))
                });
            }
            // Circles (or points) swept by each vertex.
            let mut circ = Vec::with_capacity(n);
            for i in 0..n {
                let p = s.vertices[v0[i]].p;
                let (rho, h) = coords(p);
                if on_axis[i] {
                    let e = s.add_degenerate(v0[i]);
                    s.edges[e].t1 = sweep;
                    circ.push(e);
                } else {
                    circ.push(s.add_edge(
                        Curve::Circle {
                            f: axis_frame(h),
                            r: rho,
                        },
                        0.0,
                        sweep,
                        v0[i],
                        v1[i],
                    ));
                }
            }
            let mut sl = Vec::new();
            let mut el = Vec::new();
            for i in 0..n {
                let j = (i + 1) % n;
                let g = &segs[i];
                let (pa, pb) = (place(g.start()), place(g.end()));
                let (ra, ha) = coords(pa);
                let (rb, hb) = coords(pb);
                let both_on_axis = on_axis[i] && on_axis[j] && matches!(g, Seg2::Line(..));
                // The profile edge at the start angle, and its copy at the end.
                let make = |s: &mut Solid, x: &Xform, va: usize, vb: usize| -> (usize, bool) {
                    match *g {
                        Seg2::Line(a, b) => {
                            let (p, q) = (x.point(place(a)), x.point(place(b)));
                            let len = p.dist(q);
                            (
                                s.add_edge(
                                    Curve::Line {
                                        o: p,
                                        d: (q - p) / len,
                                    },
                                    0.0,
                                    len,
                                    va,
                                    vb,
                                ),
                                false,
                            )
                        }
                        Seg2::Arc {
                            c,
                            r,
                            a0,
                            sweep: sw,
                        } => {
                            let cf = Frame {
                                origin: x.point(place(c)),
                                x: x.dir(rot.dir(frame.x)),
                                y: x.dir(rot.dir(frame.y)),
                                z: x.dir(rot.dir(frame.z)),
                            };
                            if sw >= 0.0 {
                                (
                                    s.add_edge(Curve::Circle { f: cf, r }, a0, a0 + sw, va, vb),
                                    false,
                                )
                            } else {
                                (
                                    s.add_edge(Curve::Circle { f: cf, r }, a0 + sw, a0, vb, va),
                                    true,
                                )
                            }
                        }
                    }
                };
                let (e0, rev0) = make(&mut s, &Xform::IDENTITY, v0[i], v0[j]);
                let (e1, rev1) = if full {
                    (e0, rev0)
                } else {
                    make(&mut s, &at_end, v1[i], v1[j])
                };
                sl.push(Coedge {
                    edge: e0,
                    rev: rev0,
                });
                el.push(Coedge {
                    edge: e1,
                    rev: rev1,
                });
                if both_on_axis {
                    continue;
                }
                let surface = match *g {
                    Seg2::Line(..) => {
                        if (ra - rb).abs() < 1e-9 * (1.0 + ra) {
                            Surface::Cylinder {
                                f: axis_frame(0.0),
                                r: ra,
                            }
                        } else if (ha - hb).abs() < 1e-9 * (1.0 + ha.abs()) {
                            Surface::Plane { f: axis_frame(ha) }
                        } else {
                            let t = (rb - ra) / (hb - ha);
                            Surface::Cone {
                                f: axis_frame(ha),
                                r: ra,
                                a: math::atan(t),
                            }
                        }
                    }
                    Seg2::Arc { c, r, .. } => {
                        let (rc, hc) = coords(place(c));
                        if rc < 1e-9 * (1.0 + r) {
                            Surface::Sphere {
                                f: axis_frame(hc),
                                r,
                            }
                        } else {
                            Surface::Torus {
                                f: axis_frame(hc),
                                major: rc,
                                minor: r,
                            }
                        }
                    }
                };
                if let Surface::Plane { .. } = surface {
                    // A disc, annulus or sector: no seam, and a centre on the axis is an
                    // ordinary point.
                    let mut loops = Vec::new();
                    if full {
                        if !on_axis[j] {
                            loops.push(vec![Coedge {
                                edge: circ[j],
                                rev: false,
                            }]);
                        }
                        if !on_axis[i] {
                            loops.push(vec![Coedge {
                                edge: circ[i],
                                rev: true,
                            }]);
                        }
                    } else {
                        let mut l = vec![Coedge {
                            edge: e0,
                            rev: rev0,
                        }];
                        if !on_axis[j] {
                            l.push(Coedge {
                                edge: circ[j],
                                rev: false,
                            });
                        }
                        l.push(Coedge {
                            edge: e1,
                            rev: !rev1,
                        });
                        if !on_axis[i] {
                            l.push(Coedge {
                                edge: circ[i],
                                rev: true,
                            });
                        }
                        loops.push(l);
                    }
                    laterals.push(Face { surface, loops });
                    continue;
                }
                // Profile at 0 (a→b), round at b, profile at the end (b→a), back at a.
                laterals.push(Face {
                    surface,
                    loops: vec![vec![
                        Coedge {
                            edge: e0,
                            rev: rev0,
                        },
                        Coedge {
                            edge: circ[j],
                            rev: false,
                        },
                        Coedge {
                            edge: e1,
                            rev: !rev1,
                        },
                        Coedge {
                            edge: circ[i],
                            rev: true,
                        },
                    ]],
                });
            }
            start_loops.push(sl);
            end_loops.push(el);
        }
        if !full && !start_loops.is_empty() {
            // Caps: the profile at the start (seen from the side it faces) and the end.
            let cap = |x: &Xform| {
                let px = x.dir(fx);
                Surface::Plane {
                    f: Frame {
                        origin: axis_o,
                        x: px,
                        y: k,
                        z: px.cross(k).norm(),
                    },
                }
            };
            caps.push(Face {
                surface: cap(&Xform::IDENTITY),
                loops: start_loops
                    .iter()
                    .map(|l| {
                        l.iter()
                            .rev()
                            .map(|c| Coedge {
                                edge: c.edge,
                                rev: !c.rev,
                            })
                            .collect()
                    })
                    .collect(),
            });
            caps.push(Face {
                surface: cap(&at_end),
                loops: end_loops,
            });
        }
    }
    // Drop edges whose loops vanished (profile segments on the axis are no face's).
    s.faces = laterals;
    s.faces.extend(caps);
    // Degenerate pole edges belong to their faces only; seams of segments lying on the
    // axis (both ends on it) must not appear in a loop.
    for f in &mut s.faces {
        for l in &mut f.loops {
            let edges = &s.edges;
            l.retain(|c| {
                let e = &edges[c.edge];
                !(e.degenerate && e.t1 - e.t0 == 0.0)
            });
        }
    }
    s.renumber();
    // Caps may use profile edges on the axis that no lateral face shares: drop those
    // from cap loops is not possible (a cap is a closed profile), so they stay; they
    // bound the cap along the axis.
    let m = mass::mass_props(&s);
    if m.volume < 0.0 {
        s = s.reversed();
    }
    Ok(s)
}

/// An axis-aligned box.
pub fn cuboid(min: V3, max: V3) -> Solid {
    let w = max - min;
    let r = Region2 {
        outer: Wire2 {
            segs: vec![
                Seg2::Line(v2(0.0, 0.0), v2(w.x, 0.0)),
                Seg2::Line(v2(w.x, 0.0), v2(w.x, w.y)),
                Seg2::Line(v2(w.x, w.y), v2(0.0, w.y)),
                Seg2::Line(v2(0.0, w.y), v2(0.0, 0.0)),
            ],
        },
        holes: vec![],
    };
    let f = Frame {
        origin: min,
        ..Frame::XY
    };
    extrude(&[r], &f, 0.0, w.z)
}

/// A circle profile.
pub fn circle_region(c: V2, r: f64) -> Region2 {
    Region2 {
        outer: Wire2 {
            segs: vec![Seg2::Arc {
                c,
                r,
                a0: 0.0,
                sweep: TAU,
            }],
        },
        holes: vec![],
    }
}

/// A closed polygon profile (counter-clockwise or not).
pub fn polygon_region(pts: &[V2]) -> Region2 {
    let n = pts.len();
    let w = Wire2 {
        segs: (0..n)
            .map(|i| Seg2::Line(pts[i], pts[(i + 1) % n]))
            .collect(),
    };
    let w = if w.area() < 0.0 { w.reversed() } else { w };
    Region2 {
        outer: w,
        holes: vec![],
    }
}

/// A cylinder of radius `r` round `axis` from `base`, `h` long.
pub fn cylinder(base: V3, axis: V3, r: f64, h: f64) -> Solid {
    let f = Frame::from_normal(base, axis, axis.any_perp());
    extrude(&[circle_region(V2::ZERO, r)], &f, 0.0, h)
}

/// A sphere: a half disc revolved.
pub fn sphere(c: V3, r: f64) -> Solid {
    let region = Region2 {
        outer: Wire2 {
            segs: vec![
                Seg2::Arc {
                    c: V2::ZERO,
                    r,
                    a0: -math::FRAC_PI_2,
                    sweep: math::PI,
                },
                Seg2::Line(v2(0.0, r), v2(0.0, -r)),
            ],
        },
        holes: vec![],
    };
    let f = Frame {
        origin: c,
        x: V3::X,
        y: V3::Z,
        z: -V3::Y,
    };
    revolve(&[region], &f, c, V3::Z, 0.0, TAU).expect("a half disc revolves")
}

/// A profile in the (radius, height) half-plane revolved a full turn about `axis`
/// through `base`: holes and round dress-up tools.
pub fn revolve_rz(base: V3, axis: V3, rz: &Region2) -> Result<Solid, String> {
    let y = axis.norm();
    let x = y.any_perp();
    let frame = Frame {
        origin: base,
        x,
        y,
        z: x.cross(y).norm(),
    };
    revolve(std::slice::from_ref(rz), &frame, base, y, 0.0, TAU)
}
