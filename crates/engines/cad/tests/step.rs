//! STEP: the files we write are the ones other CAD systems expect, and everything we
//! write we read back to the same solid.
use cw_cad::brep::blend::{dress, Dress};
use cw_cad::brep::boolean::{boolean, Op};
use cw_cad::brep::build::{circle_region, cuboid, cylinder, revolve, sphere};
use cw_cad::brep::mass::mass_props;
use cw_cad::brep::Solid;
use cw_cad::math::{v2, v3, Frame, PI, TAU, V3};
use cw_cad::step::{self, Schema};

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

fn round_trip(s: &Solid, schema: Schema) -> Solid {
    let text = step::write(
        s,
        "Part",
        schema,
        "/home/user/Part.step",
        "2026-09-18T10:00:00",
    );
    let mut read = step::read(&text).unwrap();
    assert_eq!(read.len(), 1, "one solid");
    let (name, back) = read.remove(0);
    assert_eq!(name, "Part");
    back
}

fn same_solid(a: &Solid, b: &Solid, rel: f64) {
    let (ma, mb) = (mass_props(a), mass_props(b));
    assert!(
        close(mb.volume, ma.volume, rel),
        "{} vs {}",
        mb.volume,
        ma.volume
    );
    assert!(close(mb.area, ma.area, rel), "{} vs {}", mb.area, ma.area);
    assert!(
        (mb.center - ma.center).len() <= rel * (1.0 + ma.center.len()) * 10.0,
        "{:?} vs {:?}",
        mb.center,
        ma.center
    );
    assert_eq!(a.faces.len(), b.faces.len(), "faces");
    assert_eq!(
        a.edges.iter().filter(|e| !e.degenerate).count(),
        b.edges.iter().filter(|e| !e.degenerate).count(),
        "edges"
    );
    assert_eq!(a.vertices.len(), b.vertices.len(), "vertices");
}

#[test]
fn a_box_writes_the_entities_a_step_reader_expects() {
    let b = cuboid(V3::ZERO, v3(20.0, 10.0, 5.0));
    let text = step::write(
        &b,
        "Box",
        Schema::Ap214,
        "/home/carol/Box.step",
        "2026-09-18T10:00:00",
    );
    assert!(text.starts_with("ISO-10303-21;\nHEADER;\n"));
    assert!(text.ends_with("ENDSEC;\nEND-ISO-10303-21;\n"));
    for want in [
        "FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));",
        "FILE_NAME('/home/carol/Box.step','2026-09-18T10:00:00'",
        "MANIFOLD_SOLID_BREP('Box'",
        "CLOSED_SHELL",
        "ADVANCED_FACE",
        "FACE_OUTER_BOUND",
        "EDGE_LOOP",
        "ORIENTED_EDGE",
        "EDGE_CURVE",
        "VERTEX_POINT",
        "CARTESIAN_POINT",
        "AXIS2_PLACEMENT_3D",
        "PLANE('",
        "( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );",
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "SHAPE_DEFINITION_REPRESENTATION",
        "PRODUCT('Box','Box'",
        "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000",
    ] {
        assert!(text.contains(want), "missing {want}");
    }
    // Six faces, twelve edges, eight vertices, and nothing dangling.
    assert_eq!(text.matches("ADVANCED_FACE").count(), 6);
    assert_eq!(text.matches("EDGE_CURVE('',").count(), 12);
    assert_eq!(text.matches("VERTEX_POINT").count(), 8);
    assert_eq!(text.matches("ORIENTED_EDGE").count(), 24);
    let ids = text.matches("\n#").count();
    for i in 1..=ids {
        assert!(
            text.contains(&format!("\n#{i} = ")),
            "entity #{i} is missing"
        );
    }
    // AP242 says so in its header.
    let text = step::write(&b, "Box", Schema::Ap242, "Box.step", "2026-09-18T10:00:00");
    assert!(text.contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"));
    assert!(text.contains("'managed model based 3d engineering',2020"));
}

#[test]
fn a_hand_written_reference_file_reads_as_the_box_it_describes() {
    let text = include_str!("data/box_inches_ap214.step");
    let mut solids = step::read(text).unwrap();
    assert_eq!(solids.len(), 1);
    let (name, s) = solids.remove(0);
    assert_eq!(name, "Box");
    assert_eq!((s.faces.len(), s.edges.len(), s.vertices.len()), (6, 12, 8));
    s.check().unwrap();
    let m = mass_props(&s);
    // 2 x 3 x 4 inches, in millimetres.
    let i3 = 25.4 * 25.4 * 25.4;
    assert!(close(m.volume, 24.0 * i3, 1e-12), "{}", m.volume);
    assert!(close(m.area, 52.0 * 25.4 * 25.4, 1e-12), "{}", m.area);
    assert!(
        (m.center - v3(1.0, 1.5, 2.0) * 25.4).len() < 1e-9,
        "{:?}",
        m.center
    );
    // It round trips through our own writer unchanged.
    same_solid(&s, &round_trip(&s, Schema::Ap242), 1e-12);
}

#[test]
fn analytic_solids_round_trip_exactly() {
    let plate = cuboid(V3::ZERO, v3(20.0, 12.0, 4.0));
    let hole = cylinder(v3(6.0, 6.0, -1.0), V3::Z, 2.5, 6.0);
    let part = boolean(&plate, &hole, Op::Difference).unwrap();
    let ball = sphere(v3(16.0, 6.0, 4.0), 3.0);
    let part = boolean(&part, &ball, Op::Difference).unwrap();
    // A cone and a torus too.
    let cone = revolve(
        &[cw_cad::brep::build::polygon_region(&[
            v2(0.0, 0.0),
            v2(3.0, 0.0),
            v2(0.0, 5.0),
        ])],
        &Frame::XZ,
        V3::ZERO,
        V3::Z,
        0.0,
        TAU,
    )
    .unwrap();
    let torus = revolve(
        &[circle_region(v2(8.0, 0.0), 1.5)],
        &Frame::XZ,
        V3::ZERO,
        V3::Z,
        0.0,
        TAU,
    )
    .unwrap();
    for (what, s) in [("part", &part), ("cone", &cone), ("torus", &torus)] {
        for schema in [Schema::Ap214, Schema::Ap242] {
            let back = round_trip(s, schema);
            back.check().unwrap_or_else(|e| panic!("{what}: {e}"));
            same_solid(s, &back, 1e-11);
        }
    }
    // The surfaces are the exact ones, not approximations.
    let text = step::write(
        &part,
        "Part",
        Schema::Ap214,
        "p.step",
        "2026-09-18T10:00:00",
    );
    assert!(text.contains("CYLINDRICAL_SURFACE"));
    assert!(text.contains("SPHERICAL_SURFACE"));
    let text = step::write(
        &cone,
        "Cone",
        Schema::Ap214,
        "c.step",
        "2026-09-18T10:00:00",
    );
    assert!(text.contains("CONICAL_SURFACE"));
    let text = step::write(
        &torus,
        "Torus",
        Schema::Ap214,
        "t.step",
        "2026-09-18T10:00:00",
    );
    assert!(text.contains("TOROIDAL_SURFACE"));
}

#[test]
fn an_oblique_cut_writes_an_ellipse_and_reads_back() {
    let cyl = cylinder(V3::ZERO, V3::Z, 4.0, 10.0);
    // A wedge whose face crosses the cylinder at 45°.
    let knife = cuboid(v3(-10.0, -10.0, 0.0), v3(10.0, 10.0, 10.0)).transformed(
        &cw_cad::math::Xform::rotate(v3(0.0, 0.0, 6.0), V3::X, PI / 4.0),
    );
    let cut = boolean(&cyl, &knife, Op::Difference).unwrap();
    let text = step::write(
        &cut,
        "Cut",
        Schema::Ap214,
        "cut.step",
        "2026-09-18T10:00:00",
    );
    assert!(
        text.contains("ELLIPSE('"),
        "an oblique cut of a cylinder is an ellipse"
    );
    let back = round_trip(&cut, Schema::Ap214);
    same_solid(&cut, &back, 1e-11);
}

#[test]
fn a_blend_writes_b_splines_close_to_the_exact_surface() {
    let main = cylinder(v3(-10.0, 0.0, 0.0), V3::X, 3.0, 20.0);
    let branch = cylinder(v3(0.0, 0.0, 0.0), V3::Z, 1.5, 10.0);
    let t = boolean(&main, &branch, Op::Union).unwrap();
    let e = (0..t.edges.len())
        .find(|e| matches!(t.edges[*e].curve, cw_cad::brep::Curve::Traced(_)))
        .unwrap();
    let f = dress(&t, &[e], Dress::Fillet(0.5), "Fillet").unwrap();
    let text = step::write(&f, "Tee", Schema::Ap242, "tee.step", "2026-09-18T10:00:00");
    assert!(text.contains("B_SPLINE_SURFACE_WITH_KNOTS"));
    assert!(text.contains("B_SPLINE_CURVE_WITH_KNOTS"));
    let back = round_trip(&f, Schema::Ap242);
    back.check().unwrap();
    // The fit is not exact, but it is a great deal better than a millimetre.
    same_solid(&f, &back, 1e-6);
}

#[test]
fn nonsense_is_refused() {
    assert!(step::read("hello").is_err());
    assert!(step::read("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\n").is_err());
    let bad = "ISO-10303-21;\nDATA;\n#1 = CLOSED_SHELL('',(#2));\nENDSEC;\n";
    assert!(step::read(bad).is_err());
}
