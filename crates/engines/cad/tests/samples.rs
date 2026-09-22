//! The sample parts seeded into users' `Documents/Parts` folders are modelled by this
//! kernel, not by hand: `worlds/company-2026/home/all/Documents/Parts/*.FCStd.json`,
//! `home/macos/Documents/Parts/*` and the bracket's STEP must be exactly what these
//! builders write. Regenerate them with
//! `CW_UPDATE_SAMPLES=1 cargo test -p cw-cad --test samples`, then run
//! `scripts/build-content.sh` to seed them.
use cw_cad::document::*;
use cw_cad::math::{v2, v3, PI, V3};
use cw_cad::sketch::{tools, Constraint, ConstraintType as T, Pos, Sketch};
use std::path::PathBuf;

/// The world's first tick, as the STEP header records it.
const STAMP: &str = "2026-09-17T09:00:00";

fn home() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../worlds/company-2026/home")
}
fn on(plane: BasePlane) -> Support {
    Support::Plane { plane }
}
fn datum(name: &str) -> Support {
    Support::Datum {
        datum: name.to_owned(),
    }
}
fn sketch(support: Support, offset: f64, s: Sketch) -> Feature {
    Feature::Sketch {
        sketch: s,
        support,
        offset,
    }
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
fn pocket(profile: &str, length: f64, through: bool) -> Feature {
    Feature::Pocket {
        profile: profile.into(),
        extent: if through {
            Extent::ThroughAll
        } else {
            Extent::Length
        },
        length,
        length2: 0.0,
        midplane: false,
        reversed: false,
    }
}
fn hole(profile: &str, diameter: f64, depth: f64, through: bool, cut: HoleCut) -> Feature {
    Feature::Hole {
        profile: profile.into(),
        diameter,
        depth,
        through_all: through,
        cut,
        drill_point: None,
    }
}
fn datum_plane(support: Support, offset: f64, angle: f64) -> Feature {
    Feature::DatumPlane {
        support,
        offset,
        angle,
    }
}
fn circles(at: &[(f64, f64)], r: f64) -> Sketch {
    let mut s = Sketch::default();
    for (x, y) in at {
        tools::circle(&mut s, v2(*x, *y), r, false).unwrap();
    }
    s
}
/// A rectangle with its width and height as named, driving constraints.
fn named_rect(w: f64, h: f64, width: &str, height: &str) -> Sketch {
    let mut s = Sketch::default();
    let ids = tools::rectangle(&mut s, v2(0.0, 0.0), v2(w, h), false).unwrap();
    s.add_constraint(
        Constraint::new(T::DistanceX, ids[0], Pos::Start)
            .with_second(ids[0], Pos::End)
            .with_value(w)
            .named(width),
    )
    .unwrap();
    s.add_constraint(
        Constraint::new(T::DistanceY, ids[1], Pos::Start)
            .with_second(ids[1], Pos::End)
            .with_value(h)
            .named(height),
    )
    .unwrap();
    // Pinned to the origin, so the sketch is fully constrained.
    s.add_constraint(
        Constraint::new(T::Coincident, ids[0], Pos::Start).with_second(-1, Pos::Start),
    )
    .unwrap();
    s
}
fn bind(doc: &mut Document, object: &str, property: &str, expression: &str) {
    doc.get_mut(object)
        .unwrap()
        .expressions
        .insert(property.into(), expression.into());
}
fn relabel(doc: &mut Document, object: &str, label: &str) {
    doc.get_mut(object).unwrap().label = label.into();
}
/// Straight edges of the current shape whose midpoints satisfy `pick`.
fn edges_where(
    model: &Model,
    feature: &str,
    pick: impl Fn(V3, &cw_cad::solid::Edge) -> bool,
) -> Vec<SubRef> {
    let shape = &model.shapes[feature];
    (0..shape.topo.edges.len())
        .filter(|i| {
            let e = &shape.topo.edges[*i];
            pick(edge_mid(&shape.mesh, &shape.topo, *i), e)
        })
        .map(|i| SubRef::edge(&shape.topo, &shape.mesh, i))
        .collect()
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// A motor mount bracket: an 80 × 50 × 6 plate with four counterbored feet holes
/// (two drilled, then patterned), an upright wall on a datum plane whose height is
/// bound to the plate's width, a NEMA 17 pilot bore and screw pattern through the
/// wall, and rounded plate corners.
pub fn motor_mount_bracket() -> Document {
    let (mut doc, body) = with_body("motor-mount-bracket");
    let plate = doc
        .add_to_body(
            &body,
            sketch(
                on(BasePlane::XY),
                0.0,
                named_rect(80.0, 50.0, "width", "depth"),
            ),
        )
        .unwrap();
    relabel(&mut doc, &plate, "PlateSketch");
    let plate_pad = doc.add_to_body(&body, pad(&plate, 6.0)).unwrap();
    relabel(&mut doc, &plate_pad, "Plate");
    let top = doc
        .add_to_body(&body, datum_plane(on(BasePlane::XY), 6.0, 0.0))
        .unwrap();
    relabel(&mut doc, &top, "PlateTop");
    // The wall stands on the plate's back edge.
    let mut wall_sketch = Sketch::default();
    tools::rectangle(&mut wall_sketch, v2(0.0, 44.0), v2(80.0, 50.0), false).unwrap();
    let wall = doc
        .add_to_body(&body, sketch(datum(&top), 0.0, wall_sketch))
        .unwrap();
    relabel(&mut doc, &wall, "WallSketch");
    let wall_pad = doc.add_to_body(&body, pad(&wall, 40.0)).unwrap();
    relabel(&mut doc, &wall_pad, "Wall");
    bind(
        &mut doc,
        &wall_pad,
        "Length",
        "PlateSketch.Constraints.width / 2",
    );
    // Feet: two counterbored M6 holes drilled from the plate top, mirrored across
    // the plate by a linear pattern.
    let feet = doc
        .add_to_body(
            &body,
            sketch(
                datum(&top),
                0.0,
                circles(&[(10.0, 10.0), (70.0, 10.0)], 3.3),
            ),
        )
        .unwrap();
    relabel(&mut doc, &feet, "FeetSketch");
    let feet_hole = doc
        .add_to_body(
            &body,
            hole(
                &feet,
                6.6,
                6.0,
                true,
                HoleCut::Counterbore {
                    diameter: 11.0,
                    depth: 2.0,
                },
            ),
        )
        .unwrap();
    relabel(&mut doc, &feet_hole, "FeetHoles");
    let pattern = doc
        .add_to_body(
            &body,
            Feature::LinearPattern {
                originals: vec![feet_hole.clone()],
                direction: AxisRef::SketchV,
                length: 26.0,
                occurrences: 2,
                reversed: false,
            },
        )
        .unwrap();
    relabel(&mut doc, &pattern, "FeetPattern");
    // The motor face: a sketch on the wall's front (the XZ plane moved to y = 44;
    // its normal is -Y, so the pocket and holes cut back into the wall).
    let motor = doc
        .add_to_body(
            &body,
            sketch(on(BasePlane::XZ), -44.0, circles(&[(40.0, 28.0)], 11.0)),
        )
        .unwrap();
    relabel(&mut doc, &motor, "PilotSketch");
    let pilot = doc.add_to_body(&body, pocket(&motor, 6.0, true)).unwrap();
    relabel(&mut doc, &pilot, "PilotBore");
    let screws = doc
        .add_to_body(
            &body,
            sketch(
                on(BasePlane::XZ),
                -44.0,
                circles(
                    &[(24.5, 12.5), (55.5, 12.5), (24.5, 43.5), (55.5, 43.5)],
                    1.7,
                ),
            ),
        )
        .unwrap();
    relabel(&mut doc, &screws, "ScrewSketch");
    let screw_holes = doc
        .add_to_body(&body, hole(&screws, 3.4, 6.0, true, HoleCut::None))
        .unwrap();
    relabel(&mut doc, &screw_holes, "MotorScrews");
    // Round the plate's front corners.
    let model = recompute(&mut doc);
    let corners = edges_where(&model, &screw_holes, |m, e| {
        e.kind == cw_cad::solid::EdgeKind::Line
            && near(m.y, 0.0)
            && near(m.z, 3.0)
            && (near(m.x, 0.0) || near(m.x, 80.0))
    });
    assert_eq!(corners.len(), 2, "the plate's two front corner edges");
    let fillet = doc
        .add_to_body(
            &body,
            Feature::Fillet {
                edges: corners,
                radius: 8.0,
                all_edges: false,
            },
        )
        .unwrap();
    relabel(&mut doc, &fillet, "CornerFillet");
    recompute(&mut doc);
    doc
}

/// A bearing housing: a flanged sleeve revolved from its half section, a snap-ring
/// groove, chamfered bore edges and six flange bolt holes in a polar pattern.
pub fn bearing_housing() -> Document {
    let (mut doc, body) = with_body("bearing-housing");
    // The half section in the XZ plane (sketch x is the radius, sketch y the height),
    // revolved about the sketch's vertical axis, which is world Z.
    let mut section = Sketch::default();
    tools::polyline(
        &mut section,
        &[
            v2(12.0, 0.0),
            v2(30.0, 0.0),
            v2(30.0, 6.0),
            v2(20.0, 6.0),
            v2(20.0, 30.0),
            v2(12.0, 30.0),
        ],
        true,
        false,
    )
    .unwrap();
    let profile = doc
        .add_to_body(&body, sketch(on(BasePlane::XZ), 0.0, section))
        .unwrap();
    relabel(&mut doc, &profile, "Section");
    let rev = doc
        .add_to_body(
            &body,
            Feature::Revolution {
                profile: profile.clone(),
                axis: AxisRef::SketchV,
                angle: 360.0,
                midplane: false,
                reversed: false,
            },
        )
        .unwrap();
    relabel(&mut doc, &rev, "Sleeve");
    // A snap-ring groove 1.5 mm into the bore near the top.
    let mut groove_sketch = Sketch::default();
    tools::rectangle(&mut groove_sketch, v2(11.0, 24.0), v2(13.5, 26.0), false).unwrap();
    let groove_profile = doc
        .add_to_body(&body, sketch(on(BasePlane::XZ), 0.0, groove_sketch))
        .unwrap();
    relabel(&mut doc, &groove_profile, "GrooveSketch");
    let groove = doc
        .add_to_body(
            &body,
            Feature::Groove {
                profile: groove_profile,
                axis: AxisRef::SketchV,
                angle: 360.0,
                midplane: false,
                reversed: false,
            },
        )
        .unwrap();
    relabel(&mut doc, &groove, "SnapRingGroove");
    // Chamfer both ends of the bore.
    let model = recompute(&mut doc);
    let bore_ends = edges_where(
        &model,
        &groove,
        |_, e| matches!(e.circle, Some((c, r, _)) if near(r, 12.0) && (near(c.z, 0.0) || near(c.z, 30.0))),
    );
    assert_eq!(bore_ends.len(), 2, "the bore's two end circles");
    let chamfer = doc
        .add_to_body(
            &body,
            Feature::Chamfer {
                edges: bore_ends,
                size: 1.0,
                kind: ChamferType::Equal,
                size2: 1.0,
                angle: 45.0,
                flip: false,
                all_edges: false,
            },
        )
        .unwrap();
    relabel(&mut doc, &chamfer, "BoreChamfer");
    // Bolt holes drilled from the flange's top, then spread round the axis.
    let flange_top = doc
        .add_to_body(&body, datum_plane(on(BasePlane::XY), 6.0, 0.0))
        .unwrap();
    relabel(&mut doc, &flange_top, "FlangeTop");
    let bolt = doc
        .add_to_body(
            &body,
            sketch(datum(&flange_top), 0.0, circles(&[(25.0, 0.0)], 2.2)),
        )
        .unwrap();
    relabel(&mut doc, &bolt, "BoltSketch");
    let bolt_hole = doc
        .add_to_body(&body, hole(&bolt, 4.5, 6.0, true, HoleCut::None))
        .unwrap();
    relabel(&mut doc, &bolt_hole, "BoltHole");
    let polar = doc
        .add_to_body(
            &body,
            Feature::PolarPattern {
                originals: vec![bolt_hole],
                axis: AxisRef::SketchNormal,
                angle: 360.0,
                occurrences: 6,
                reversed: false,
            },
        )
        .unwrap();
    relabel(&mut doc, &polar, "BoltPattern");
    recompute(&mut doc);
    doc
}

/// An enclosure lid: an additive box hollowed by a pocket whose depth is bound to the
/// box's height, a grid of vent holes through the floor, and rounded corners.
pub fn enclosure_lid() -> Document {
    let (mut doc, body) = with_body("enclosure-lid");
    let block = doc
        .add_to_body(
            &body,
            Feature::Primitive {
                shape: Primitive::Box {
                    length: 120.0,
                    width: 80.0,
                    height: 8.0,
                },
                subtractive: false,
                support: on(BasePlane::XY),
                offset: 0.0,
            },
        )
        .unwrap();
    relabel(&mut doc, &block, "Blank");
    let rim = doc
        .add_to_body(&body, datum_plane(on(BasePlane::XY), 8.0, 0.0))
        .unwrap();
    relabel(&mut doc, &rim, "Rim");
    let mut cavity_sketch = Sketch::default();
    tools::rectangle(&mut cavity_sketch, v2(2.0, 2.0), v2(118.0, 78.0), false).unwrap();
    let cavity_profile = doc
        .add_to_body(&body, sketch(datum(&rim), 0.0, cavity_sketch))
        .unwrap();
    relabel(&mut doc, &cavity_profile, "CavitySketch");
    let cavity = doc
        .add_to_body(&body, pocket(&cavity_profile, 6.0, false))
        .unwrap();
    relabel(&mut doc, &cavity, "Cavity");
    bind(&mut doc, &cavity, "Length", "Blank.Height - 2");
    // Vents: a row of five holes through the 2 mm floor, repeated three times.
    let floor = doc
        .add_to_body(&body, datum_plane(on(BasePlane::XY), 2.0, 0.0))
        .unwrap();
    relabel(&mut doc, &floor, "Floor");
    let row: Vec<(f64, f64)> = (0..5).map(|i| (40.0 + 10.0 * i as f64, 25.0)).collect();
    let vent_sketch = doc
        .add_to_body(&body, sketch(datum(&floor), 0.0, circles(&row, 1.5)))
        .unwrap();
    relabel(&mut doc, &vent_sketch, "VentSketch");
    let vent = doc
        .add_to_body(&body, hole(&vent_sketch, 3.0, 2.0, true, HoleCut::None))
        .unwrap();
    relabel(&mut doc, &vent, "VentRow");
    let grid = doc
        .add_to_body(
            &body,
            Feature::LinearPattern {
                originals: vec![vent],
                direction: AxisRef::SketchV,
                length: 30.0,
                occurrences: 3,
                reversed: false,
            },
        )
        .unwrap();
    relabel(&mut doc, &grid, "VentGrid");
    // Round the outer vertical corners.
    let model = recompute(&mut doc);
    let corners = edges_where(&model, &grid, |m, e| {
        e.kind == cw_cad::solid::EdgeKind::Line
            && near(m.z, 4.0)
            && (near(m.x, 0.0) || near(m.x, 120.0))
            && (near(m.y, 0.0) || near(m.y, 80.0))
    });
    assert_eq!(corners.len(), 4, "the four outer corner edges");
    let fillet = doc
        .add_to_body(
            &body,
            Feature::Fillet {
                edges: corners,
                radius: 6.0,
                all_edges: false,
            },
        )
        .unwrap();
    relabel(&mut doc, &fillet, "CornerFillet");
    recompute(&mut doc);
    doc
}

fn check(path: &str, bytes: &[u8]) {
    let path = home().join(path);
    if std::env::var_os("CW_UPDATE_SAMPLES").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        return;
    }
    let current = std::fs::read(&path).unwrap_or_default();
    assert!(
        current == bytes,
        "{} is not what the kernel writes; regenerate with CW_UPDATE_SAMPLES=1",
        path.display()
    );
}
fn all_ok(doc: &mut Document) -> Model {
    let m = recompute(doc);
    for (name, s) in &m.status {
        assert_eq!(s, &Status::Ok, "{name}: {s:?}");
    }
    assert!(m.expression_errors.is_empty(), "{:?}", m.expression_errors);
    m
}
fn body_volume(m: &Model) -> f64 {
    m.body_shape["Body"].volume()
}

#[test]
fn the_seeded_bracket_is_what_the_kernel_models() {
    let mut doc = motor_mount_bracket();
    let m = all_ok(&mut doc);
    // Plate and wall, less the bore, screws and feet (both counterbores and shanks).
    let plate = 80.0 * 50.0 * 6.0;
    let wall = 80.0 * 6.0 * 40.0;
    let bore = PI * 11.0 * 11.0 * 6.0;
    let screws = 4.0 * PI * 1.7 * 1.7 * 6.0;
    let feet = 4.0 * (PI * 3.3 * 3.3 * 4.0 + PI * 5.5 * 5.5 * 2.0);
    let corner = 2.0 * (1.0 - PI / 4.0) * 64.0 * 6.0;
    let v = body_volume(&m);
    let expected = plate + wall - bore - screws - feet - corner;
    assert!((v - expected).abs() < 1e-3, "{v} vs {expected}");
    // The wall's height is the expression's doing.
    assert_eq!(
        doc.get("Pad001").unwrap().feature.number("Length"),
        Some(40.0)
    );
    assert_eq!(
        doc.get("Pad001").unwrap().expressions["Length"],
        "PlateSketch.Constraints.width / 2"
    );
    let b = m.body_shape["Body"].mesh.bounds().unwrap();
    assert!((b.max.z - 46.0).abs() < 1e-6 && (b.max - v3(80.0, 50.0, 46.0)).len() < 1e-6);
    // The saved document reads back and recomputes to the same solid.
    let text = cw_cad::io::save_native(&doc);
    let mut back = cw_cad::io::load_native(&text).unwrap();
    assert_eq!(back, doc);
    let m2 = recompute(&mut back);
    assert!((body_volume(&m2) - v).abs() < 1e-9);
    check(
        "all/Documents/Parts/motor-mount-bracket.FCStd.json",
        text.as_bytes(),
    );
    // Its STEP export, as File → Export writes it, reads back to the same volume.
    let solid = m.body_shape["Body"].solid.as_ref().unwrap();
    let step = cw_cad::step::write(
        solid,
        "motor-mount-bracket",
        cw_cad::step::Schema::Ap214,
        "/Users/alice/Documents/Parts/motor-mount-bracket.step",
        STAMP,
    );
    let read = cw_cad::step::read(&step).unwrap();
    assert_eq!(read.len(), 1);
    let sv = cw_cad::brep::mass::mass_props(&read[0].1).volume;
    assert!((sv - v).abs() < 1e-4 * v, "{sv} vs {v}");
    check(
        "macos/Documents/Parts/motor-mount-bracket.step",
        step.as_bytes(),
    );
}

#[test]
fn the_seeded_housing_is_what_the_kernel_models() {
    let mut doc = bearing_housing();
    let m = all_ok(&mut doc);
    let flange = PI * (30.0 * 30.0 - 12.0 * 12.0) * 6.0;
    let sleeve = PI * (20.0 * 20.0 - 12.0 * 12.0) * 24.0;
    let groove = PI * (13.5 * 13.5 - 12.0 * 12.0) * 2.0;
    // Each 45° chamfer removes a triangular ring of section 0.5 at radius 12 + 1/3.
    let chamfers = 2.0 * 2.0 * PI * (12.0 + 1.0 / 3.0) * 0.5;
    let bolts = 6.0 * PI * 2.25 * 2.25 * 6.0;
    let v = body_volume(&m);
    let expected = flange + sleeve - groove - chamfers - bolts;
    assert!((v - expected).abs() < 1e-3, "{v} vs {expected}");
    // The native file keeps a few less digits of an edge reference's hint than the
    // model carries, so the loaded document is compared by what it recomputes to.
    let text = cw_cad::io::save_native(&doc);
    let mut back = cw_cad::io::load_native(&text).unwrap();
    let m2 = all_ok(&mut back);
    assert!((body_volume(&m2) - v).abs() < 1e-9);
    check(
        "macos/Documents/Parts/bearing-housing.FCStd.json",
        text.as_bytes(),
    );
}

#[test]
fn the_seeded_lid_is_what_the_kernel_models() {
    let mut doc = enclosure_lid();
    let m = all_ok(&mut doc);
    let blank = 120.0 * 80.0 * 8.0;
    let cavity = 116.0 * 76.0 * 6.0;
    let vents = 15.0 * PI * 1.5 * 1.5 * 2.0;
    let corners = 4.0 * (1.0 - PI / 4.0) * 36.0 * 8.0;
    let v = body_volume(&m);
    let expected = blank - cavity - vents - corners;
    assert!((v - expected).abs() < 1e-3, "{v} vs {expected}");
    assert_eq!(
        doc.get("Pocket").unwrap().feature.number("Length"),
        Some(6.0)
    );
    assert_eq!(
        doc.get("Box").unwrap().feature.type_id(),
        "PartDesign::AdditiveBox"
    );
    let text = cw_cad::io::save_native(&doc);
    assert_eq!(cw_cad::io::load_native(&text).unwrap(), doc);
    check(
        "macos/Documents/Parts/enclosure-lid.FCStd.json",
        text.as_bytes(),
    );
}
