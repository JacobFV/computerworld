//! Part Design features against analytic volumes and areas, recompute after edits, and
//! watertight results.
use cw_cad::document::*;
use cw_cad::math::{v2, v3, PI, V3};
use cw_cad::sketch::{tools, Constraint, ConstraintType as T, Pos, Sketch};
use cw_cad::solid::EdgeKind;

/// A circle's area: the kernel is exact, so this is the real thing.
fn circle_area(r: f64) -> f64 {
    PI * r * r
}

fn sketch_on(doc: &mut Document, body: &str, plane: BasePlane, s: Sketch) -> String {
    doc.add_to_body(
        body,
        Feature::Sketch {
            sketch: s,
            support: Support::Plane { plane },
            offset: 0.0,
        },
    )
    .unwrap()
}
fn rect(w: f64, h: f64) -> Sketch {
    let mut s = Sketch::default();
    tools::rectangle(&mut s, v2(0.0, 0.0), v2(w, h), false).unwrap();
    s
}
fn pad(profile: &str, length: f64) -> Feature {
    Feature::Pad {
        profile: profile.into(),
        extent: Extent::Length,
        length,
        length2: 0.0,
        midplane: false,
        reversed: false,
    }
}
fn ok(model: &Model, names: &[&str]) {
    for n in names {
        assert_eq!(
            model.status.get(*n),
            Some(&Status::Ok),
            "{n}: {:?}",
            model.status.get(*n)
        );
    }
}
fn shape<'a>(model: &'a Model, name: &str) -> &'a Shape {
    model
        .shapes
        .get(name)
        .map(|s| &**s)
        .unwrap_or_else(|| panic!("{name} has no shape"))
}

fn padded_box() -> (Document, String, String) {
    let (mut doc, body) = with_body("Unnamed");
    let sk = sketch_on(&mut doc, &body, BasePlane::XY, rect(20.0, 10.0));
    let p = doc.add_to_body(&body, pad(&sk, 5.0)).unwrap();
    (doc, body, p)
}

#[test]
fn pad_then_pocket_through_all_and_edit_upstream() {
    let (mut doc, body, p) = padded_box();
    let model = recompute(&mut doc);
    ok(&model, &[&p, &body]);
    let m = shape(&model, &p);
    assert!(m.mesh.is_watertight());
    assert!((m.volume() - 1000.0).abs() < 1e-9);
    assert!((m.area() - 2.0 * (200.0 + 100.0 + 50.0)).abs() < 1e-9);

    let mut hole = Sketch::default();
    tools::circle(&mut hole, v2(10.0, 5.0), 3.0, false).unwrap();
    let sk2 = sketch_on(&mut doc, &body, BasePlane::XY, hole);
    let pocket = doc
        .add_to_body(
            &body,
            Feature::Pocket {
                profile: sk2.clone(),
                extent: Extent::ThroughAll,
                length: 0.0,
                length2: 0.0,
                midplane: false,
                reversed: true,
            },
        )
        .unwrap();
    let model = recompute(&mut doc);
    ok(&model, &[&pocket]);
    let m = &model.body_shape[&body];
    assert!(m.mesh.is_watertight(), "{} open edges", m.mesh.open_edges());
    let want = 1000.0 - circle_area(3.0) * 5.0;
    assert!((m.volume() - want).abs() < 1e-6, "{} vs {want}", m.volume());
    // Two circular edges and the box's twelve.
    let topo = &model.body_shape[&body].topo;
    assert_eq!(
        topo.edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Circle)
            .count(),
        2
    );

    // Make the pad taller: the pocket follows through the whole new height.
    if let Some(Object {
        feature: Feature::Pad { length, .. },
        ..
    }) = doc.get_mut(&p)
    {
        *length = 8.0;
    }
    let model = recompute(&mut doc);
    let m = &model.body_shape[&body];
    let want = 1600.0 - circle_area(3.0) * 8.0;
    assert!((m.volume() - want).abs() < 1e-6, "{} vs {want}", m.volume());
    assert!(m.mesh.is_watertight());
}

#[test]
fn a_sketch_on_a_face_follows_the_face_when_the_pad_changes() {
    let (mut doc, body, p) = padded_box();
    let model = recompute(&mut doc);
    let s = shape(&model, &p);
    let top = (0..s.topo.faces.len())
        .find(|i| face_normal(&s.mesh, &s.topo, *i).z > 0.9)
        .unwrap();
    let mut boss = Sketch::default();
    tools::circle(&mut boss, v2(10.0, 5.0), 2.0, false).unwrap();
    let sk = doc
        .add_to_body(
            &body,
            Feature::Sketch {
                sketch: boss,
                support: Support::Face {
                    feature: p.clone(),
                    face: SubRef::face(&s.topo, &s.mesh, top),
                },
                offset: 0.0,
            },
        )
        .unwrap();
    let p2 = doc.add_to_body(&body, pad(&sk, 3.0)).unwrap();
    let model = recompute(&mut doc);
    ok(&model, &[&sk, &p2]);
    assert!((model.frames[&sk].origin.z - 5.0).abs() < 1e-9);
    let want = 1000.0 + circle_area(2.0) * 3.0;
    assert!((model.body_shape[&body].volume() - want).abs() < 1e-6);
    let b = model.body_shape[&body].mesh.bounds().unwrap();
    assert!((b.max.z - 8.0).abs() < 1e-9);
    // The base pad grows; the sketch rides up with its face.
    if let Some(Object {
        feature: Feature::Pad { length, .. },
        ..
    }) = doc.get_mut(&p)
    {
        *length = 12.0;
    }
    let model = recompute(&mut doc);
    ok(&model, &[&sk, &p2]);
    let b = model.body_shape[&body].mesh.bounds().unwrap();
    assert!((b.max.z - 15.0).abs() < 1e-9, "top at {}", b.max.z);
}

#[test]
fn revolution_and_groove_match_their_exact_volumes() {
    let (mut doc, body) = with_body("Unnamed");
    // A 10 x 20 rectangle from radius 5 to 15, revolved about the sketch's V axis.
    let mut s = Sketch::default();
    tools::rectangle(&mut s, v2(5.0, 0.0), v2(15.0, 20.0), false).unwrap();
    let sk = sketch_on(&mut doc, &body, BasePlane::XZ, s);
    let rev = doc
        .add_to_body(
            &body,
            Feature::Revolution {
                profile: sk,
                axis: AxisRef::SketchV,
                angle: 360.0,
                midplane: false,
                reversed: false,
            },
        )
        .unwrap();
    let model = recompute(&mut doc);
    ok(&model, &[&rev]);
    let k = circle_area(1.0);
    let tube = k * (225.0 - 25.0) * 20.0;
    let m = &model.body_shape[&body];
    assert!(m.mesh.is_watertight());
    assert!((m.volume() - tube).abs() < 1e-6, "{} vs {tube}", m.volume());
    // Groove a 2 x 2 ring out of the outer wall at mid height.
    let mut g = Sketch::default();
    tools::rectangle(&mut g, v2(13.0, 9.0), v2(16.0, 11.0), false).unwrap();
    let gs = sketch_on(&mut doc, &body, BasePlane::XZ, g);
    let groove = doc
        .add_to_body(
            &body,
            Feature::Groove {
                profile: gs,
                axis: AxisRef::SketchV,
                angle: 360.0,
                midplane: false,
                reversed: false,
            },
        )
        .unwrap();
    let model = recompute(&mut doc);
    ok(&model, &[&groove]);
    let m = &model.body_shape[&body];
    assert!(m.mesh.is_watertight(), "{} open", m.mesh.open_edges());
    let want = tube - k * (225.0 - 169.0) * 2.0;
    assert!((m.volume() - want).abs() < 1e-6, "{} vs {want}", m.volume());
}

#[test]
fn fillet_and_chamfer_remove_exactly_their_sections() {
    for chamfer in [false, true] {
        let (mut doc, body, p) = padded_box();
        let model = recompute(&mut doc);
        let s = shape(&model, &p);
        // The top edge along x at y = 0: from (0,0,5) to (20,0,5).
        let e = (0..s.topo.edges.len())
            .find(|i| {
                let r = SubRef::edge(&s.topo, &s.mesh, *i);
                (r.center - v3(10.0, 0.0, 5.0)).len() < 1e-9
            })
            .unwrap();
        let edge = SubRef::edge(&s.topo, &s.mesh, e);
        let f = doc
            .add_to_body(
                &body,
                if chamfer {
                    Feature::Chamfer {
                        edges: vec![edge],
                        size: 2.0,
                        kind: ChamferType::Equal,
                        size2: 2.0,
                        angle: 45.0,
                        flip: false,
                        all_edges: false,
                    }
                } else {
                    Feature::Fillet {
                        edges: vec![edge],
                        radius: 2.0,
                        all_edges: false,
                    }
                },
            )
            .unwrap();
        let model = recompute(&mut doc);
        ok(&model, &[&f]);
        let m = &model.body_shape[&body];
        assert!(m.mesh.is_watertight(), "{} open", m.mesh.open_edges());
        let removed = if chamfer {
            2.0 * 2.0 / 2.0 * 20.0
        } else {
            // The square corner minus the quarter disc: exactly.
            (4.0 - circle_area(2.0) / 4.0) * 20.0
        };
        let got = 1000.0 - m.volume();
        assert!(
            (got - removed).abs() < 1e-9,
            "{chamfer}: removed {got}, want {removed}"
        );
        if !chamfer {
            let t = &model.body_shape[&body].topo;
            assert!(t.faces.iter().any(|f| matches!(
                m.mesh.surfaces[f.surface as usize],
                cw_cad::mesh::Surface::Cylinder { .. }
            )));
        }
    }
}

#[test]
fn chamfer_types_and_rounding_every_edge() {
    // Two distances and distance-and-angle take the sizes FreeCAD's properties name.
    for (kind, size, size2, angle, flip, want) in [
        (ChamferType::Equal, 2.0, 0.0, 0.0, false, 2.0 * 2.0 / 2.0),
        (ChamferType::TwoDistances, 1.0, 3.0, 0.0, false, 1.5),
        (ChamferType::TwoDistances, 1.0, 3.0, 0.0, true, 1.5),
        (
            ChamferType::DistanceAngle,
            2.0,
            0.0,
            30.0,
            false,
            2.0 * 2.0 * (PI / 6.0).tan() / 2.0,
        ),
    ] {
        let (mut doc, body, p) = padded_box();
        let model = recompute(&mut doc);
        let s = shape(&model, &p);
        let e = (0..s.topo.edges.len())
            .find(|i| (SubRef::edge(&s.topo, &s.mesh, *i).center - v3(10.0, 0.0, 5.0)).len() < 1e-9)
            .unwrap();
        let f = doc
            .add_to_body(
                &body,
                Feature::Chamfer {
                    edges: vec![SubRef::edge(&s.topo, &s.mesh, e)],
                    size,
                    kind,
                    size2,
                    angle,
                    flip,
                    all_edges: false,
                },
            )
            .unwrap();
        let model = recompute(&mut doc);
        ok(&model, &[&f]);
        let got = 1000.0 - model.body_shape[&body].volume();
        assert!(
            (got - want * 20.0).abs() < 1e-9,
            "{kind:?} flip {flip}: {got} vs {}",
            want * 20.0
        );
    }
    // Flipping the two distances mirrors the bevel: the same volume, a different shape.
    let two = |flip: bool| -> V3 {
        let (mut doc, body, p) = padded_box();
        let model = recompute(&mut doc);
        let s = shape(&model, &p);
        let e = (0..s.topo.edges.len())
            .find(|i| (SubRef::edge(&s.topo, &s.mesh, *i).center - v3(10.0, 0.0, 5.0)).len() < 1e-9)
            .unwrap();
        doc.add_to_body(
            &body,
            Feature::Chamfer {
                edges: vec![SubRef::edge(&s.topo, &s.mesh, e)],
                size: 1.0,
                kind: ChamferType::TwoDistances,
                size2: 3.0,
                angle: 45.0,
                flip,
                all_edges: false,
            },
        )
        .unwrap();
        let model = recompute(&mut doc);
        model.body_shape[&body].center_of_mass().unwrap()
    };
    let (a, b) = (two(false), two(true));
    assert!((a - b).len() > 0.01, "{a:?} vs {b:?}");
    // Rounding every edge needs no selection at all.
    let (mut doc, body, _) = padded_box();
    let f = doc
        .add_to_body(
            &body,
            Feature::Fillet {
                edges: vec![],
                radius: 1.0,
                all_edges: true,
            },
        )
        .unwrap();
    let model = recompute(&mut doc);
    ok(&model, &[&f]);
    let s = &model.body_shape[&body];
    // Six faces, twelve rounds, eight corners.
    assert_eq!(s.topo.faces.len(), 26);
    let (a, b, c) = (20.0 - 2.0, 10.0 - 2.0, 5.0 - 2.0);
    let r = 1.0;
    let want = a * b * c
        + 2.0 * r * (a * b + b * c + c * a)
        + PI * r * r * (a + b + c)
        + 4.0 / 3.0 * PI * r * r * r;
    assert!((s.volume() - want).abs() < 1e-9, "{} vs {want}", s.volume());
}

#[test]
fn a_blend_that_does_not_fit_is_refused_in_freecads_words() {
    let dress = |feature: Feature| -> String {
        let (mut doc, body, p) = padded_box();
        let model = recompute(&mut doc);
        let s = shape(&model, &p);
        let e = (0..s.topo.edges.len())
            .find(|i| (SubRef::edge(&s.topo, &s.mesh, *i).center - v3(10.0, 0.0, 5.0)).len() < 1e-9)
            .unwrap();
        let edges = vec![SubRef::edge(&s.topo, &s.mesh, e)];
        let feature = match feature {
            Feature::Fillet {
                radius, all_edges, ..
            } => Feature::Fillet {
                edges,
                radius,
                all_edges,
            },
            Feature::Chamfer {
                size,
                kind,
                size2,
                angle,
                flip,
                all_edges,
                ..
            } => Feature::Chamfer {
                edges,
                size,
                kind,
                size2,
                angle,
                flip,
                all_edges,
            },
            other => other,
        };
        let f = doc.add_to_body(&body, feature).unwrap();
        let model = recompute(&mut doc);
        model.error(&f).unwrap_or("").to_owned()
    };
    let big = dress(Feature::Fillet {
        edges: vec![],
        radius: 6.0,
        all_edges: false,
    });
    assert!(
        big.contains("Fillet not possible on selected shapes"),
        "{big}"
    );
    assert!(big.contains("too large"), "{big}");
    let zero = dress(Feature::Fillet {
        edges: vec![],
        radius: 0.0,
        all_edges: false,
    });
    assert!(
        zero.contains("Fillet radius must be greater than zero"),
        "{zero}"
    );
    let wide = dress(Feature::Chamfer {
        edges: vec![],
        size: 30.0,
        kind: ChamferType::Equal,
        size2: 30.0,
        angle: 45.0,
        flip: false,
        all_edges: false,
    });
    assert!(wide.contains("Failed to create chamfer"), "{wide}");
    let bad_angle = dress(Feature::Chamfer {
        edges: vec![],
        size: 1.0,
        kind: ChamferType::DistanceAngle,
        size2: 1.0,
        angle: 200.0,
        flip: false,
        all_edges: false,
    });
    assert!(
        bad_angle.contains("Angle must be greater than 0 and less than 180"),
        "{bad_angle}"
    );
}

#[test]
fn a_through_hole_with_a_counterbore() {
    let (mut doc, body, _) = padded_box();
    let mut s = Sketch::default();
    tools::circle(&mut s, v2(10.0, 5.0), 1.0, false).unwrap();
    let sk = doc
        .add_to_body(
            &body,
            Feature::Sketch {
                sketch: s,
                support: Support::Plane {
                    plane: BasePlane::XY,
                },
                offset: 5.0,
            },
        )
        .unwrap();
    let h = doc
        .add_to_body(
            &body,
            Feature::Hole {
                profile: sk,
                diameter: 4.0,
                depth: 0.0,
                through_all: true,
                cut: HoleCut::Counterbore {
                    diameter: 6.0,
                    depth: 2.0,
                },
                drill_point: None,
            },
        )
        .unwrap();
    let model = recompute(&mut doc);
    ok(&model, &[&h]);
    let m = &model.body_shape[&body];
    assert!(m.mesh.is_watertight(), "{} open", m.mesh.open_edges());
    let want = 1000.0 - circle_area(2.0) * 3.0 - circle_area(3.0) * 2.0;
    assert!((m.volume() - want).abs() < 1e-6, "{} vs {want}", m.volume());
}

#[test]
fn patterns_and_mirrors_repeat_the_original_tool() {
    let (mut doc, body, _) = padded_box();
    let mut s = Sketch::default();
    tools::circle(&mut s, v2(3.0, 5.0), 1.0, false).unwrap();
    let sk = sketch_on(&mut doc, &body, BasePlane::XY, s);
    let pocket = doc
        .add_to_body(
            &body,
            Feature::Pocket {
                profile: sk,
                extent: Extent::ThroughAll,
                length: 0.0,
                length2: 0.0,
                midplane: true,
                reversed: false,
            },
        )
        .unwrap();
    let lin = doc
        .add_to_body(
            &body,
            Feature::LinearPattern {
                originals: vec![pocket.clone()],
                direction: AxisRef::SketchH,
                length: 12.0,
                occurrences: 4,
                reversed: false,
            },
        )
        .unwrap();
    let model = recompute(&mut doc);
    ok(&model, &[&pocket, &lin]);
    let m = &model.body_shape[&body];
    assert!(m.mesh.is_watertight(), "{} open", m.mesh.open_edges());
    let hole = circle_area(1.0) * 5.0;
    assert!((m.volume() - (1000.0 - 4.0 * hole)).abs() < 1e-6);
    // Mirror the first hole across the plane x = 0 would leave the part; mirror
    // across the sketch's vertical axis through x = 0 instead of a centred one.
    let (mut doc, body, _) = padded_box();
    let mut s = Sketch::default();
    tools::rectangle(&mut s, v2(0.0, 0.0), v2(2.0, 10.0), false).unwrap();
    let sk = sketch_on(&mut doc, &body, BasePlane::XY, s);
    let boss = doc.add_to_body(&body, pad(&sk, 8.0)).unwrap();
    let mir = doc
        .add_to_body(
            &body,
            Feature::Mirrored {
                originals: vec![boss.clone()],
                plane: PlaneRef::Base(BasePlane::YZ),
            },
        )
        .unwrap();
    let model = recompute(&mut doc);
    ok(&model, &[&boss, &mir]);
    let m = &model.body_shape[&body];
    assert!(m.mesh.is_watertight());
    // Box, the boss's 3 mm above it, and the mirrored boss (all 8 mm) beside it.
    assert!(
        (m.volume() - (1000.0 + 2.0 * 10.0 * 3.0 + 2.0 * 10.0 * 8.0)).abs() < 1e-6,
        "{}",
        m.volume()
    );
    // A polar pattern of 6 bosses round the Z axis.
    let (mut doc, body) = with_body("Unnamed");
    let mut base = Sketch::default();
    tools::circle(&mut base, v2(0.0, 0.0), 10.0, false).unwrap();
    let bs = sketch_on(&mut doc, &body, BasePlane::XY, base);
    doc.add_to_body(&body, pad(&bs, 2.0)).unwrap();
    let mut lug = Sketch::default();
    tools::rectangle(&mut lug, v2(8.0, -1.0), v2(14.0, 1.0), false).unwrap();
    let ls = sketch_on(&mut doc, &body, BasePlane::XY, lug);
    let lp = doc.add_to_body(&body, pad(&ls, 2.0)).unwrap();
    let polar = doc
        .add_to_body(
            &body,
            Feature::PolarPattern {
                originals: vec![lp],
                axis: AxisRef::Z,
                angle: 360.0,
                occurrences: 6,
                reversed: false,
            },
        )
        .unwrap();
    let model = recompute(&mut doc);
    ok(&model, &[&polar]);
    let m = &model.body_shape[&body];
    assert!(m.mesh.is_watertight(), "{} open", m.mesh.open_edges());
    assert!(m.volume() > circle_area(10.0) * 2.0 + 6.0 * 4.0 * 2.0 * 2.0 - 1e-6);
    let _ = PI;
}

#[test]
fn a_pad_that_leaves_the_body_is_refused_as_two_solids() {
    let (mut doc, body, _) = padded_box();
    let mut s = Sketch::default();
    tools::rectangle(&mut s, v2(40.0, 0.0), v2(50.0, 10.0), false).unwrap();
    let sk = sketch_on(&mut doc, &body, BasePlane::XY, s);
    let far = doc.add_to_body(&body, pad(&sk, 5.0)).unwrap();
    let model = recompute(&mut doc);
    assert!(model.error(&far).unwrap().contains("multiple solids"));
    assert!(matches!(model.status[&body], Status::Error(_)));
}

#[test]
fn an_edge_reference_survives_an_upstream_change() {
    let (mut doc, body, p) = padded_box();
    let model = recompute(&mut doc);
    let s = shape(&model, &p);
    // The top edge along x at y = 0.
    let e = (0..s.topo.edges.len())
        .find(|i| (SubRef::edge(&s.topo, &s.mesh, *i).center - v3(10.0, 0.0, 5.0)).len() < 1e-9)
        .unwrap();
    let f = doc
        .add_to_body(
            &body,
            Feature::Fillet {
                edges: vec![SubRef::edge(&s.topo, &s.mesh, e)],
                radius: 2.0,
                all_edges: false,
            },
        )
        .unwrap();
    let model = recompute(&mut doc);
    ok(&model, &[&f]);
    let corner = 4.0 - PI;
    assert!((model.body_shape[&body].volume() - (1000.0 - corner * 20.0)).abs() < 1e-9);
    // Taller and wider: the rounded edge is still the top one at y = 0, and the
    // reference has been rewritten to where it is now.
    if let Some(Object {
        feature: Feature::Pad { length, .. },
        ..
    }) = doc.get_mut(&p)
    {
        *length = 9.0;
    }
    if let Some(Object {
        feature: Feature::Sketch { sketch, .. },
        ..
    }) = doc.get_mut("Sketch")
    {
        *sketch = rect(30.0, 10.0);
    }
    let model = recompute(&mut doc);
    ok(&model, &[&f]);
    assert!(
        (model.body_shape[&body].volume() - (30.0 * 10.0 * 9.0 - corner * 30.0)).abs() < 1e-9,
        "{}",
        model.body_shape[&body].volume()
    );
    let Some(Object {
        feature: Feature::Fillet { edges, .. },
        ..
    }) = doc.get(&f)
    else {
        panic!()
    };
    assert!(
        (edges[0].center - v3(15.0, 0.0, 9.0)).len() < 1e-9,
        "{:?}",
        edges[0]
    );
}

#[test]
fn an_imported_solid_round_trips_through_the_document() {
    let (mut doc, _, _) = padded_box();
    let model = recompute(&mut doc);
    let solid = model.shapes["Pad"].solid.clone().unwrap();
    let name = doc.add(Feature::Part {
        solid: solid.clone(),
    });
    let text = serde_json::to_string(&doc).unwrap();
    assert!(text.contains("\"TypeId\":\"Part::Feature\""));
    let back: Document = serde_json::from_str(&text).unwrap();
    let mut back = back;
    let model = recompute(&mut back);
    assert_eq!(model.status[&name], Status::Ok);
    assert!((model.shapes[&name].volume() - 1000.0).abs() < 1e-9);
}

#[test]
fn documents_round_trip_through_json() {
    let (mut doc, body, _) = padded_box();
    if let Some(Object {
        feature: Feature::Sketch { sketch, .. },
        ..
    }) = doc.get_mut("Sketch")
    {
        sketch
            .add_constraint(Constraint::new(T::Distance, 0, Pos::None).with_value(20.0))
            .unwrap();
    }
    let text = serde_json::to_string_pretty(&doc).unwrap();
    assert!(text.contains("\"TypeId\": \"PartDesign::Pad\""));
    let back: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(back, doc);
    let a = recompute(&mut doc);
    let mut back = back;
    let b = recompute(&mut back);
    assert_eq!(
        a.body_shape[&body].mesh, b.body_shape[&body].mesh,
        "recompute is deterministic"
    );
}
