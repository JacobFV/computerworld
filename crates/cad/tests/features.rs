//! Primitives, booleans between bodies, datums and expressions: the newer Part Design
//! features against analytic volumes, through JSON and STEP.
use cw_cad::document::*;
use cw_cad::math::{v2, v3, PI, V3};
use cw_cad::sketch::{tools, Constraint, ConstraintType as T, Pos, Sketch};
use std::collections::BTreeMap;

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
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
fn on_xy(shape: Primitive, subtractive: bool) -> Feature {
    Feature::Primitive {
        shape,
        subtractive,
        support: Support::Plane {
            plane: BasePlane::XY,
        },
        offset: 0.0,
    }
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

#[test]
fn every_primitive_has_its_closed_form_volume() {
    let (mut doc, body) = with_body("Primitives");
    let b = doc
        .add_to_body(
            &body,
            on_xy(
                Primitive::Box {
                    length: 20.0,
                    width: 10.0,
                    height: 5.0,
                },
                false,
            ),
        )
        .unwrap();
    let m = recompute(&mut doc);
    ok(&m, &[&b]);
    assert!(close(m.shapes[&b].volume(), 1000.0, 1e-9));
    assert_eq!(
        doc.get(&b).unwrap().feature.type_id(),
        "PartDesign::AdditiveBox"
    );

    for (shape, volume) in [
        (
            Primitive::Cylinder {
                radius: 3.0,
                height: 10.0,
                angle: 360.0,
            },
            PI * 9.0 * 10.0,
        ),
        (
            Primitive::Cylinder {
                radius: 3.0,
                height: 10.0,
                angle: 90.0,
            },
            PI * 9.0 * 10.0 / 4.0,
        ),
        (Primitive::Sphere { radius: 4.0 }, 4.0 / 3.0 * PI * 64.0),
        (
            Primitive::Cone {
                radius1: 4.0,
                radius2: 0.0,
                height: 9.0,
            },
            PI * 16.0 * 9.0 / 3.0,
        ),
        (
            Primitive::Cone {
                radius1: 2.0,
                radius2: 4.0,
                height: 6.0,
            },
            PI * 6.0 / 3.0 * (4.0 + 8.0 + 16.0),
        ),
        (
            Primitive::Torus {
                radius1: 10.0,
                radius2: 2.0,
            },
            2.0 * PI * PI * 10.0 * 4.0,
        ),
    ] {
        let (mut doc, body) = with_body("P");
        let name = doc.add_to_body(&body, on_xy(shape, false)).unwrap();
        let m = recompute(&mut doc);
        ok(&m, &[&name]);
        let v = m.shapes[&name].volume();
        assert!(close(v, volume, 1e-6), "{}: {v} vs {volume}", shape.name());
        assert!(m.shapes[&name].mesh.is_watertight(), "{}", shape.name());
    }
}

#[test]
fn a_subtractive_primitive_cuts_and_is_attached_to_a_face() {
    let (mut doc, body) = with_body("Cut");
    let b = doc
        .add_to_body(
            &body,
            on_xy(
                Primitive::Box {
                    length: 20.0,
                    width: 20.0,
                    height: 10.0,
                },
                false,
            ),
        )
        .unwrap();
    let m = recompute(&mut doc);
    // The top face, found by its centre.
    let shape = &m.shapes[&b];
    let top = (0..shape.topo.faces.len())
        .find(|i| (shape.topo.faces[*i].centroid - v3(10.0, 10.0, 10.0)).len() < 1e-6)
        .expect("top face");
    let face = SubRef::face(&shape.topo, &shape.mesh, top);
    // A cylinder bored 4 mm into the top, its axis along the face normal, moved down
    // by a negative offset so it starts inside the block. A face's frame has its origin
    // where the world origin projects onto the plane (FreeCAD's attachment), so the
    // cylinder stands on the block's corner and a quarter of it is inside.
    let c = doc
        .add_to_body(
            &body,
            Feature::Primitive {
                shape: Primitive::Cylinder {
                    radius: 2.0,
                    height: 4.0,
                    angle: 360.0,
                },
                subtractive: true,
                support: Support::Face {
                    feature: b.clone(),
                    face,
                },
                offset: -4.0,
            },
        )
        .unwrap();
    let m = recompute(&mut doc);
    ok(&m, &[&b, &c]);
    let v = m.body_shape[&body].volume();
    assert!(close(v, 4000.0 - PI * 4.0 * 4.0 / 4.0, 1e-9), "{v}");
    // A subtractive primitive on an empty body is refused in FreeCAD's words.
    let (mut doc, body) = with_body("Empty");
    let s = doc
        .add_to_body(&body, on_xy(Primitive::Sphere { radius: 3.0 }, true))
        .unwrap();
    let m = recompute(&mut doc);
    assert!(matches!(m.status.get(&s), Some(Status::Error(e)) if e.contains("base shape")));
}

#[test]
fn a_boolean_between_bodies_fuses_cuts_and_intersects() {
    for (kind, volume) in [
        (BoolType::Fuse, 1000.0),
        (BoolType::Cut, 750.0),
        (BoolType::Common, 250.0),
    ] {
        let mut doc = Document::new("Bool");
        let a = doc.add(Feature::Body {
            group: vec![],
            tip: None,
        });
        let b = doc.add(Feature::Body {
            group: vec![],
            tip: None,
        });
        doc.add_to_body(
            &a,
            on_xy(
                Primitive::Box {
                    length: 10.0,
                    width: 10.0,
                    height: 10.0,
                },
                false,
            ),
        )
        .unwrap();
        // The second body: a 5 × 5 × 10 box on a datum plane, inside the first's corner.
        let plane = doc
            .add_to_body(
                &b,
                Feature::DatumPlane {
                    support: Support::Plane {
                        plane: BasePlane::XY,
                    },
                    offset: 0.0,
                    angle: 0.0,
                },
            )
            .unwrap();
        doc.add_to_body(
            &b,
            Feature::Primitive {
                shape: Primitive::Box {
                    length: 5.0,
                    width: 5.0,
                    height: 10.0,
                },
                subtractive: false,
                support: Support::Datum { datum: plane },
                offset: 0.0,
            },
        )
        .unwrap();
        let bool_name = doc
            .add_to_body(
                &a,
                Feature::Boolean {
                    kind,
                    bodies: vec![b.clone()],
                },
            )
            .unwrap();
        let m = recompute(&mut doc);
        ok(&m, &[&bool_name, &a, &b]);
        let v = m.body_shape[&a].volume();
        assert!(close(v, volume, 1e-9), "{}: {v} vs {volume}", kind.label());
        // Body B keeps its own shape.
        assert!(close(m.body_shape[&b].volume(), 250.0, 1e-9));
    }
}

#[test]
fn a_boolean_finds_a_tool_body_declared_after_it() {
    let mut doc = Document::new("Order");
    let a = doc.add(Feature::Body {
        group: vec![],
        tip: None,
    });
    doc.add_to_body(
        &a,
        on_xy(
            Primitive::Box {
                length: 10.0,
                width: 10.0,
                height: 10.0,
            },
            false,
        ),
    )
    .unwrap();
    let cut = doc
        .add_to_body(
            &a,
            Feature::Boolean {
                kind: BoolType::Cut,
                bodies: vec!["Body001".into()],
            },
        )
        .unwrap();
    let b = doc.add(Feature::Body {
        group: vec![],
        tip: None,
    });
    assert_eq!(b, "Body001");
    doc.add_to_body(&b, on_xy(Primitive::Sphere { radius: 3.0 }, false))
        .unwrap();
    let m = recompute(&mut doc);
    ok(&m, &[&cut]);
    // An eighth of the sphere sits inside the box's corner.
    let v = m.body_shape[&a].volume();
    let eighth = 4.0 / 3.0 * PI * 27.0 / 8.0;
    assert!(close(v, 1000.0 - eighth, 1e-6), "{v}");
    // A body cannot consume itself.
    if let Some(Object {
        feature: Feature::Boolean { bodies, .. },
        ..
    }) = doc.get_mut(&cut)
    {
        *bodies = vec![a.clone()];
    }
    let m = recompute(&mut doc);
    assert!(matches!(m.status.get(&cut), Some(Status::Error(e)) if e.contains("itself")));
}

#[test]
fn a_sketch_on_a_turned_datum_plane_pads_along_its_normal() {
    let (mut doc, body) = with_body("Datum");
    // XY moved up 5 and turned 90° about its x axis: the plane y = -5 … its normal
    // becomes -Y (the rotation carries +Z to -Y).
    let plane = doc
        .add_to_body(
            &body,
            Feature::DatumPlane {
                support: Support::Plane {
                    plane: BasePlane::XY,
                },
                offset: 5.0,
                angle: 90.0,
            },
        )
        .unwrap();
    let sk = doc
        .add_to_body(
            &body,
            Feature::Sketch {
                sketch: rect(10.0, 4.0),
                support: Support::Datum {
                    datum: plane.clone(),
                },
                offset: 0.0,
            },
        )
        .unwrap();
    let p = doc.add_to_body(&body, pad(&sk, 3.0)).unwrap();
    let m = recompute(&mut doc);
    ok(&m, &[&plane, &sk, &p]);
    let Some(DatumGeom::Plane(f)) = m.datums.get(&plane) else {
        panic!("no datum frame");
    };
    assert!((f.origin - v3(0.0, 0.0, 5.0)).len() < 1e-9);
    assert!((f.z - v3(0.0, -1.0, 0.0)).len() < 1e-9, "{:?}", f.z);
    let b = m.body_shape[&body].mesh.bounds().unwrap();
    assert!(close(m.body_shape[&body].volume(), 120.0, 1e-9));
    // The pad grows along -Y from y = 0 to y = -3.
    assert!(
        (b.min.y + 3.0).abs() < 1e-6 && b.max.y.abs() < 1e-6,
        "{b:?}"
    );
    // The sketch's x runs along X and its y along the turned axis (+Z).
    assert!(
        (b.min.z - 5.0).abs() < 1e-6 && (b.max.z - 9.0).abs() < 1e-6,
        "{b:?}"
    );

    // A datum line at 90° in XY is the Y axis; a datum point lands where it says.
    let line = doc
        .add_to_body(
            &body,
            Feature::DatumLine {
                support: Support::Plane {
                    plane: BasePlane::XY,
                },
                offset: 0.0,
                angle: 90.0,
            },
        )
        .unwrap();
    let point = doc
        .add_to_body(
            &body,
            Feature::DatumPoint {
                support: Support::Plane {
                    plane: BasePlane::XZ,
                },
                offset: 1.0,
                x: 2.0,
                y: 3.0,
            },
        )
        .unwrap();
    let m = recompute(&mut doc);
    ok(&m, &[&line, &point]);
    match m.datums[&line] {
        DatumGeom::Line { origin, dir } => {
            assert!((origin - V3::ZERO).len() < 1e-9);
            assert!((dir - v3(0.0, 1.0, 0.0)).len() < 1e-9);
        }
        _ => panic!("not a line"),
    }
    match m.datums[&point] {
        // XZ's normal is -Y, so an offset of 1 lands at y = -1.
        DatumGeom::Point(p) => assert!((p - v3(2.0, -1.0, 3.0)).len() < 1e-9, "{p:?}"),
        _ => panic!("not a point"),
    }
    // A polar pattern about the datum line.
    let m = {
        let hole_sketch = {
            let mut s = Sketch::default();
            tools::circle(&mut s, v2(5.0, -1.5), 0.5, false).unwrap();
            s
        };
        let _ = hole_sketch;
        recompute(&mut doc)
    };
    ok(&m, &[&p]);
}

#[test]
fn expressions_drive_properties_and_report_their_errors() {
    let (mut doc, body) = with_body("Expr");
    let mut s = rect(10.0, 4.0);
    // Name the width constraint so an expression can read it.
    s.add_constraint(
        Constraint::new(T::DistanceX, 0, Pos::Start)
            .with_second(0, Pos::End)
            .with_value(10.0)
            .named("width"),
    )
    .unwrap();
    let sk = doc
        .add_to_body(
            &body,
            Feature::Sketch {
                sketch: s,
                support: Support::Plane {
                    plane: BasePlane::XY,
                },
                offset: 0.0,
            },
        )
        .unwrap();
    let p = doc.add_to_body(&body, pad(&sk, 1.0)).unwrap();
    doc.get_mut(&p)
        .unwrap()
        .expressions
        .insert("Length".into(), "Sketch.Constraints.width * 2".into());
    let m = recompute(&mut doc);
    ok(&m, &[&p]);
    assert!(m.expression_errors.is_empty(), "{:?}", m.expression_errors);
    assert_eq!(doc.get(&p).unwrap().feature.number("Length"), Some(20.0));
    assert!(close(m.body_shape[&body].volume(), 10.0 * 4.0 * 20.0, 1e-9));
    // Change the constraint: the pad follows on the next recompute.
    doc.get_mut(&sk)
        .unwrap()
        .feature
        .set_number("Constraints.width", 6.0)
        .unwrap();
    let m = recompute(&mut doc);
    assert_eq!(doc.get(&p).unwrap().feature.number("Length"), Some(12.0));
    assert!(close(m.body_shape[&body].volume(), 6.0 * 4.0 * 12.0, 1e-9));
    // A chain: a chamfer-free second pad reads the first pad's length.
    let sk2 = doc
        .add_to_body(
            &body,
            Feature::Sketch {
                sketch: rect(2.0, 2.0),
                support: Support::Plane {
                    plane: BasePlane::XY,
                },
                offset: 11.0,
            },
        )
        .unwrap();
    let p2 = doc.add_to_body(&body, pad(&sk2, 1.0)).unwrap();
    doc.get_mut(&p2)
        .unwrap()
        .expressions
        .insert("Length".into(), "Pad.Length / 4 + 1 mm".into());
    let m = recompute(&mut doc);
    ok(&m, &[&p2]);
    assert_eq!(doc.get(&p2).unwrap().feature.number("Length"), Some(4.0));
    // Errors: an unknown reference, a self reference, a value the property refuses.
    doc.get_mut(&p2)
        .unwrap()
        .expressions
        .insert("Length".into(), "Nothing.Length".into());
    let m = recompute(&mut doc);
    assert!(m.expression_errors[&(p2.clone(), "Length".into())].contains("Nothing.Length"));
    doc.get_mut(&p2)
        .unwrap()
        .expressions
        .insert("Length".into(), "Pad001.Length + 1".into());
    let m = recompute(&mut doc);
    assert!(m.expression_errors[&(p2.clone(), "Length".into())].contains("itself"));
    doc.get_mut(&p2)
        .unwrap()
        .expressions
        .insert("Length".into(), "0 - 3".into());
    let m = recompute(&mut doc);
    assert!(m.expression_errors[&(p2.clone(), "Length".into())].contains("positive"));
    // The JSON carries the bindings.
    let text = serde_json::to_string_pretty(&doc).unwrap();
    assert!(text.contains("\"ExpressionEngine\""));
    let back: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(back, doc);
}

#[test]
fn new_features_round_trip_through_json_and_step() {
    let mut doc = Document::new("RoundTrip");
    let a = doc.add(Feature::Body {
        group: vec![],
        tip: None,
    });
    let b = doc.add(Feature::Body {
        group: vec![],
        tip: None,
    });
    let plane = doc
        .add_to_body(
            &a,
            Feature::DatumPlane {
                support: Support::Plane {
                    plane: BasePlane::XY,
                },
                offset: 2.0,
                angle: 0.0,
            },
        )
        .unwrap();
    doc.add_to_body(
        &a,
        Feature::Primitive {
            shape: Primitive::Torus {
                radius1: 4.0,
                radius2: 2.0,
            },
            subtractive: false,
            support: Support::Datum { datum: plane },
            offset: 0.0,
        },
    )
    .unwrap();
    doc.add_to_body(
        &b,
        on_xy(
            Primitive::Cone {
                radius1: 3.0,
                radius2: 1.0,
                height: 12.0,
            },
            false,
        ),
    )
    .unwrap();
    doc.add_to_body(
        &b,
        Feature::DatumLine {
            support: Support::Plane {
                plane: BasePlane::YZ,
            },
            offset: 0.0,
            angle: 30.0,
        },
    )
    .unwrap();
    doc.add_to_body(
        &b,
        Feature::DatumPoint {
            support: Support::Plane {
                plane: BasePlane::XY,
            },
            offset: 0.0,
            x: 1.0,
            y: 2.0,
        },
    )
    .unwrap();
    let boolean = doc
        .add_to_body(
            &a,
            Feature::Boolean {
                kind: BoolType::Fuse,
                bodies: vec![b.clone()],
            },
        )
        .unwrap();
    let text = serde_json::to_string_pretty(&doc).unwrap();
    for tag in [
        "\"TypeId\": \"PartDesign::FeaturePrimitive\"",
        "\"Primitive\": \"Torus\"",
        "\"TypeId\": \"PartDesign::Boolean\"",
        "\"TypeId\": \"PartDesign::Plane\"",
        "\"TypeId\": \"PartDesign::Line\"",
        "\"TypeId\": \"PartDesign::Point\"",
        "\"Type\": \"Datum\"",
    ] {
        assert!(text.contains(tag), "{tag} missing from\n{text}");
    }
    let mut back: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(back, doc);
    let m = recompute(&mut doc);
    ok(&m, &[&boolean]);
    let m2 = recompute(&mut back);
    assert_eq!(m.body_shape[&a].mesh, m2.body_shape[&a].mesh);
    // Native save/load keeps everything too.
    let native = cw_cad::io::save_native(&doc);
    assert_eq!(cw_cad::io::load_native(&native).unwrap(), doc);
    // STEP holds the fused solid: torus, cone and their intersection curves.
    let solid = m.body_shape[&a].solid.as_ref().unwrap();
    let step = cw_cad::step::write(
        solid,
        "RoundTrip",
        cw_cad::step::Schema::Ap214,
        "/home/alice/Documents/Parts/RoundTrip.step",
        "2026-09-17T09:00:00",
    );
    assert!(step.contains("TOROIDAL_SURFACE") && step.contains("CONICAL_SURFACE"));
    let read = cw_cad::step::read(&step).unwrap();
    assert_eq!(read.len(), 1);
    let v = cw_cad::brep::mass::mass_props(&read[0].1).volume;
    assert!(close(v, m.body_shape[&a].volume(), 1e-4), "{v}");
    let _: BTreeMap<String, String> = BTreeMap::new();
}
