use super::build::*;
use super::mass::*;
use super::tess::tessellate;
use crate::math::{v2, v3, Frame, PI, TAU, V2, V3};

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

#[test]
fn a_box_is_exact() {
    let b = cuboid(v3(1.0, 2.0, 3.0), v3(3.0, 5.0, 7.0));
    b.check().unwrap();
    assert_eq!((b.faces.len(), b.edges.len(), b.vertices.len()), (6, 12, 8));
    let m = mass_props(&b);
    assert!(close(m.volume, 24.0, 1e-13), "{}", m.volume);
    assert!(close(m.area, 2.0 * (6.0 + 8.0 + 12.0), 1e-13), "{}", m.area);
    assert!(
        (m.center - v3(2.0, 3.5, 5.0)).len() < 1e-12,
        "{:?}",
        m.center
    );
    let (mesh, topo) = tessellate(&b);
    assert!(mesh.is_watertight(), "{}", mesh.open_edges());
    assert_eq!(topo.faces.len(), 6);
}

#[test]
fn a_cylinder_is_exact_with_a_seam() {
    let c = cylinder(v3(0.0, 0.0, 1.0), V3::Z, 5.0, 10.0);
    c.check().unwrap();
    // FreeCAD's: 3 faces, 3 edges (two circles and the seam), 2 vertices.
    assert_eq!((c.faces.len(), c.edges.len(), c.vertices.len()), (3, 3, 2));
    let m = mass_props(&c);
    assert!(close(m.volume, PI * 25.0 * 10.0, 1e-13), "{}", m.volume);
    assert!(close(m.area, 2.0 * PI * 25.0 + TAU * 5.0 * 10.0, 1e-13));
    assert!(
        (m.center - v3(0.0, 0.0, 6.0)).len() < 1e-11,
        "{:?}",
        m.center
    );
    let (mesh, _) = tessellate(&c);
    assert!(mesh.is_watertight(), "{}", mesh.open_edges());
}

#[test]
fn revolved_primitives_are_exact() {
    let s = sphere(v3(1.0, -2.0, 0.5), 3.0);
    s.check().unwrap();
    let m = mass_props(&s);
    assert!(
        close(m.volume, 4.0 / 3.0 * PI * 27.0, 1e-12),
        "{}",
        m.volume
    );
    assert!(close(m.area, 4.0 * PI * 9.0, 1e-12), "{}", m.area);
    assert!(
        (m.center - v3(1.0, -2.0, 0.5)).len() < 1e-10,
        "{:?}",
        m.center
    );
    let (mesh, _) = tessellate(&s);
    assert!(mesh.is_watertight(), "{}", mesh.open_edges());

    // A torus: a circle of radius 2 at distance 5 from the axis.
    let f = Frame::XZ;
    let torus = revolve(
        &[circle_region(v2(5.0, 1.0), 2.0)],
        &f,
        V3::ZERO,
        V3::Z,
        0.0,
        TAU,
    )
    .unwrap();
    torus.check().unwrap();
    let m = mass_props(&torus);
    assert!(
        close(m.volume, 2.0 * PI * PI * 5.0 * 4.0, 1e-12),
        "{}",
        m.volume
    );
    assert!(
        close(m.area, 4.0 * PI * PI * 5.0 * 2.0, 1e-12),
        "{}",
        m.area
    );
    let (mesh, _) = tessellate(&torus);
    assert!(mesh.is_watertight(), "{}", mesh.open_edges());

    // A cone: triangle (0,0) (3,0) (0,4) about the sketch's vertical axis.
    let cone = revolve(
        &[polygon_region(&[v2(0.0, 0.0), v2(3.0, 0.0), v2(0.0, 4.0)])],
        &f,
        V3::ZERO,
        V3::Z,
        0.0,
        TAU,
    )
    .unwrap();
    cone.check().unwrap();
    let m = mass_props(&cone);
    assert!(close(m.volume, PI * 9.0 * 4.0 / 3.0, 1e-12), "{}", m.volume);
    assert!(
        close(m.area, PI * 9.0 + PI * 3.0 * 5.0, 1e-12),
        "{}",
        m.area
    );
    assert!(
        (m.center - v3(0.0, 0.0, 1.0)).len() < 1e-11,
        "{:?}",
        m.center
    );
    let (mesh, _) = tessellate(&cone);
    assert!(mesh.is_watertight(), "{}", mesh.open_edges());

    // A quarter of a tube: Pappus.
    let tube = revolve(
        &[polygon_region(&[
            v2(5.0, 0.0),
            v2(15.0, 0.0),
            v2(15.0, 20.0),
            v2(5.0, 20.0),
        ])],
        &f,
        V3::ZERO,
        V3::Z,
        0.0,
        PI / 2.0,
    )
    .unwrap();
    tube.check().unwrap();
    let m = mass_props(&tube);
    assert!(
        close(m.volume, PI / 4.0 * (225.0 - 25.0) * 20.0, 1e-12),
        "{}",
        m.volume
    );
    let (mesh, _) = tessellate(&tube);
    assert!(mesh.is_watertight(), "{}", mesh.open_edges());
    let _ = V2::ZERO;
}

use super::boolean::{boolean, Op};

fn solid_ok(s: &super::Solid) {
    s.check().unwrap();
    let (mesh, _) = tessellate(s);
    assert!(
        mesh.is_watertight(),
        "{} open mesh edges",
        mesh.open_edges()
    );
}

#[test]
fn overlapping_boxes_and_coplanar_faces() {
    let a = cuboid(v3(0.0, 0.0, 0.0), v3(2.0, 2.0, 2.0));
    let b = cuboid(v3(1.0, 1.0, 1.0), v3(3.0, 3.0, 3.0));
    for (op, want) in [
        (Op::Union, 15.0),
        (Op::Difference, 7.0),
        (Op::Intersection, 1.0),
    ] {
        let r = boolean(&a, &b, op).unwrap();
        solid_ok(&r);
        let m = mass_props(&r);
        assert!(close(m.volume, want, 1e-12), "{op:?}: {}", m.volume);
    }
    // Intersection is a plain cube again.
    let i = boolean(&a, &b, Op::Intersection).unwrap();
    assert_eq!((i.faces.len(), i.edges.len(), i.vertices.len()), (6, 12, 8));
    // Coplanar faces: a box extended along x by another sharing four face planes
    // refines to one box.
    let c = cuboid(v3(1.0, 0.0, 0.0), v3(3.0, 2.0, 2.0));
    let u = boolean(&a, &c, Op::Union).unwrap();
    solid_ok(&u);
    assert!(close(mass_props(&u).volume, 12.0, 1e-12));
    assert_eq!((u.faces.len(), u.edges.len(), u.vertices.len()), (6, 12, 8));
    // A box on top of another, touching over a face: one solid.
    let top = cuboid(v3(0.5, 0.5, 2.0), v3(1.5, 1.5, 3.0));
    let u = boolean(&a, &top, Op::Union).unwrap();
    solid_ok(&u);
    assert!(close(mass_props(&u).volume, 9.0, 1e-12));
    assert_eq!(u.faces.len(), 11);
    // Cutting a slot flush with a face.
    let slot = cuboid(v3(0.5, -1.0, 1.0), v3(1.5, 3.0, 2.0));
    let d = boolean(&a, &slot, Op::Difference).unwrap();
    solid_ok(&d);
    assert!(close(mass_props(&d).volume, 8.0 - 2.0, 1e-12));
}

#[test]
fn a_through_hole_and_a_boss() {
    let plate = cuboid(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 2.0));
    let pin = cylinder(v3(5.0, 5.0, -1.0), V3::Z, 2.0, 4.0);
    let d = boolean(&plate, &pin, Op::Difference).unwrap();
    solid_ok(&d);
    let m = mass_props(&d);
    assert!(
        close(m.volume, 200.0 - PI * 4.0 * 2.0, 1e-12),
        "{}",
        m.volume
    );
    assert!(
        close(
            m.area,
            2.0 * (100.0 - 4.0 * PI) + 40.0 * 2.0 + TAU * 2.0 * 2.0,
            1e-12
        ),
        "{}",
        m.area
    );
    // Faces: 6 of the plate and the hole's wall.
    assert_eq!(d.faces.len(), 7);
    let u = boolean(&plate, &pin, Op::Union).unwrap();
    solid_ok(&u);
    assert!(close(mass_props(&u).volume, 200.0 + PI * 4.0 * 2.0, 1e-12));
    // A blind hole from the top.
    let blind = cylinder(v3(5.0, 5.0, 1.0), V3::Z, 2.0, 5.0);
    let d = boolean(&plate, &blind, Op::Difference).unwrap();
    solid_ok(&d);
    assert!(close(mass_props(&d).volume, 200.0 - PI * 4.0, 1e-12));
}

#[test]
fn sphere_and_box() {
    let s = sphere(V3::ZERO, 5.0);
    let half = cuboid(v3(0.0, -10.0, -10.0), v3(10.0, 10.0, 10.0));
    // The cutting plane contains the sphere's axis: the section runs through the poles.
    let h = boolean(&s, &half, Op::Intersection).unwrap();
    solid_ok(&h);
    let m = mass_props(&h);
    assert!(
        close(m.volume, 2.0 / 3.0 * PI * 125.0, 1e-12),
        "{}",
        m.volume
    );
    assert!(
        (m.center.x - 3.0 * 5.0 / 8.0).abs() < 1e-10,
        "{:?}",
        m.center
    );
    // A cap cut off by a plane across the axis.
    let slab = cuboid(v3(-10.0, -10.0, 3.0), v3(10.0, 10.0, 10.0));
    let cap = boolean(&s, &slab, Op::Intersection).unwrap();
    solid_ok(&cap);
    let hcap = 2.0;
    let want = PI * hcap * hcap * (3.0 * 5.0 - hcap) / 3.0;
    assert!(
        close(mass_props(&cap).volume, want, 1e-12),
        "{}",
        mass_props(&cap).volume
    );
    // A box with a spherical dent in a face.
    let block = cuboid(v3(-10.0, -10.0, -10.0), v3(10.0, 10.0, 3.0));
    let d = boolean(&block, &s, Op::Difference).unwrap();
    solid_ok(&d);
    assert!(close(
        mass_props(&d).volume,
        20.0 * 20.0 * 13.0 - (4.0 / 3.0 * PI * 125.0 - want),
        1e-12
    ));
}

#[test]
fn cylinders_crossing_and_touching() {
    // A T-junction: a vertical cylinder into a horizontal one, then through it.
    let main = cylinder(v3(-10.0, 0.0, 0.0), V3::X, 3.0, 20.0);
    let branch = cylinder(v3(0.0, 0.0, 0.0), V3::Z, 1.5, 10.0);
    let u = boolean(&main, &branch, Op::Union).unwrap();
    solid_ok(&u);
    // Volume of the branch outside the main cylinder: its full volume from the axis
    // minus the part inside, ∫∫ over the disc of sqrt(R² − y²).
    let (r, big) = (1.5f64, 3.0f64);
    // y = r sin θ keeps the integrand smooth.
    let [inside] = super::num::integrate_gl(-PI / 2.0, PI / 2.0, 8, |th| {
        let (s, c) = (th.sin(), th.cos());
        let y = r * s;
        [2.0 * r * c * (big * big - y * y).sqrt() * r * c]
    });
    let want = PI * 9.0 * 20.0 + PI * r * r * 10.0 - inside;
    let got = mass_props(&u).volume;
    assert!(close(got, want, 1e-9), "{got} vs {want}");
    // Straight through: the cross hole.
    let through = cylinder(v3(0.0, 0.0, -5.0), V3::Z, 1.5, 10.0);
    let d = boolean(&main, &through, Op::Difference).unwrap();
    solid_ok(&d);
    let got = mass_props(&d).volume;
    let want = PI * 9.0 * 20.0 - 2.0 * inside;
    assert!(close(got, want, 1e-9), "{got} vs {want}");
    // Two parallel cylinders touching along a line are tangent: a D-profile pad (box
    // with a cylinder tangent to two of its faces) unions cleanly.
    let bx = cuboid(v3(0.0, -5.0, 0.0), v3(10.0, 5.0, 4.0));
    let round = cylinder(v3(10.0, 0.0, 0.0), V3::Z, 5.0, 4.0);
    let u = boolean(&bx, &round, Op::Union).unwrap();
    solid_ok(&u);
    assert!(close(
        mass_props(&u).volume,
        400.0 + PI * 25.0 / 2.0 * 4.0,
        1e-12
    ));
    // Tangent cylinders side by side (touching along a line) give two solids touching.
    let c2 = cylinder(v3(20.0, 0.0, 0.0), V3::Z, 5.0, 4.0);
    let d = boolean(&round, &c2, Op::Difference).unwrap();
    solid_ok(&d);
    assert!(close(mass_props(&d).volume, PI * 25.0 * 4.0, 1e-12));
}

use super::blend::{dress, Dress};

/// The edge whose midpoint is `m`.
fn edge_at(s: &super::Solid, m: V3) -> usize {
    (0..s.edges.len())
        .find(|e| !s.edges[*e].degenerate && s.edges[*e].mid().dist(m) < 1e-6)
        .unwrap_or_else(|| panic!("no edge at {m:?}"))
}

#[test]
fn fillets_and_chamfers_on_straight_edges_are_exact() {
    let b = cuboid(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 5.0));
    let e = edge_at(&b, v3(10.0, 0.0, 5.0));
    let f = dress(&b, &[e], Dress::Fillet(2.0), "Fillet").unwrap();
    solid_ok(&f);
    let m = mass_props(&f);
    assert!(
        close(m.volume, 1000.0 - 4.0 * (1.0 - PI / 4.0) * 20.0, 1e-12),
        "{}",
        m.volume
    );
    assert_eq!(f.faces.len(), 7, "six faces and the round");
    assert!(f
        .faces
        .iter()
        .any(|x| matches!(x.surface, super::Surface::Cylinder { .. })));
    let c = dress(&b, &[e], Dress::Chamfer(2.0, 2.0), "Chamfer").unwrap();
    solid_ok(&c);
    assert!(close(mass_props(&c).volume, 1000.0 - 2.0 * 20.0, 1e-12));
    // Two distances.
    let c = dress(&b, &[e], Dress::Chamfer(1.0, 3.0), "Chamfer").unwrap();
    solid_ok(&c);
    assert!(close(mass_props(&c).volume, 1000.0 - 1.5 * 20.0, 1e-12));
    // Too big: FreeCAD refuses.
    let err = dress(&b, &[e], Dress::Fillet(6.0), "Fillet").unwrap_err();
    assert!(err.contains("too large"), "{err}");
}

#[test]
fn every_edge_of_a_cube_rounded_blends_the_corners_with_spheres() {
    let a = 10.0;
    let r = 2.0;
    let b = cuboid(V3::ZERO, v3(a, a, a));
    let all: Vec<usize> = (0..b.edges.len()).collect();
    let f = dress(&b, &all, Dress::Fillet(r), "Fillet").unwrap();
    solid_ok(&f);
    // The cube shrunk by r, grown back by a ball of radius r.
    let s = a - 2.0 * r;
    let want = s * s * s + 6.0 * s * s * r + 3.0 * PI * s * r * r + 4.0 / 3.0 * PI * r * r * r;
    let m = mass_props(&f);
    assert!(close(m.volume, want, 1e-11), "{} vs {want}", m.volume);
    let area = 6.0 * s * s + 3.0 * TAU * r * s + 4.0 * PI * r * r;
    assert!(close(m.area, area, 1e-11), "{} vs {area}", m.area);
    assert_eq!(f.faces.len(), 26);
    assert_eq!(
        f.faces
            .iter()
            .filter(|x| matches!(x.surface, super::Surface::Sphere { .. }))
            .count(),
        8
    );
    // Three edges at one corner only.
    let three: Vec<usize> = (0..b.edges.len())
        .filter(|e| {
            let ed = &b.edges[*e];
            ed.start().len() < 1e-9 || ed.end().len() < 1e-9
        })
        .collect();
    assert_eq!(three.len(), 3);
    let f = dress(&b, &three, Dress::Fillet(r), "Fillet").unwrap();
    solid_ok(&f);
    let removed = 3.0 * r * r * (1.0 - PI / 4.0) * a - 3.0 * r * r * (1.0 - PI / 4.0) * r
        + (r * r * r - PI / 6.0 * r * r * r);
    assert!(
        close(mass_props(&f).volume, a * a * a - removed, 1e-11),
        "{}",
        mass_props(&f).volume
    );
    // Chamfering every edge gives the triangular corner facets.
    let c = dress(&b, &all, Dress::Chamfer(1.0, 1.0), "Chamfer").unwrap();
    solid_ok(&c);
    assert_eq!(c.faces.len(), 26);
}

#[test]
fn round_edges_of_revolved_and_padded_shapes() {
    // The top rim of a cylinder: the round is a torus; Pappus gives the removed volume.
    let (big, r) = (5.0, 1.0);
    let c = cylinder(V3::ZERO, V3::Z, big, 10.0);
    let rim = (0..c.edges.len())
        .find(|e| c.edges[*e].closed() && c.edges[*e].start().z > 9.0)
        .unwrap();
    let f = dress(&c, &[rim], Dress::Fillet(r), "Fillet").unwrap();
    solid_ok(&f);
    let a = r * r * (1.0 - PI / 4.0);
    let xbar = (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI)) * r;
    let want = PI * big * big * 10.0 - TAU * (big - xbar) * a;
    assert!(
        close(mass_props(&f).volume, want, 1e-12),
        "{} vs {want}",
        mass_props(&f).volume
    );
    assert!(f
        .faces
        .iter()
        .any(|x| matches!(x.surface, super::Surface::Torus { .. })));
    let ch = dress(&c, &[rim], Dress::Chamfer(1.0, 1.0), "Chamfer").unwrap();
    solid_ok(&ch);
    let want = PI * big * big * 10.0 - TAU * (big - 1.0 / 3.0) * 0.5;
    assert!(close(mass_props(&ch).volume, want, 1e-12));
    assert!(ch
        .faces
        .iter()
        .any(|x| matches!(x.surface, super::Surface::Cone { .. })));
    // A vertical edge between a flat and a round side (a D profile).
    let bx = cuboid(v3(0.0, -5.0, 0.0), v3(10.0, 5.0, 4.0));
    let round = cylinder(v3(0.0, 0.0, 0.0), V3::Z, 7.0, 4.0);
    let d = boolean(&bx, &round, Op::Union).unwrap();
    solid_ok(&d);
    let v0 = mass_props(&d).volume;
    let e = (0..d.edges.len())
        .find(|e| {
            let m = d.edges[*e].mid();
            matches!(d.edges[*e].curve, super::Curve::Line { .. })
                && (m.z - 2.0).abs() < 1e-9
                && m.x > 4.0
                && m.y > 4.9
        })
        .unwrap();
    let f = dress(&d, &[e], Dress::Fillet(1.0), "Fillet").unwrap();
    solid_ok(&f);
    assert!(mass_props(&f).volume < v0 && mass_props(&f).volume > v0 - 4.0);
}

#[test]
fn concave_edges_gain_material() {
    let base = cuboid(V3::ZERO, v3(20.0, 10.0, 2.0));
    let wall = cuboid(v3(0.0, 0.0, 2.0), v3(3.0, 10.0, 12.0));
    let l = boolean(&base, &wall, Op::Union).unwrap();
    let e = edge_at(&l, v3(3.0, 5.0, 2.0));
    let f = dress(&l, &[e], Dress::Fillet(1.5), "Fillet").unwrap();
    solid_ok(&f);
    let want = mass_props(&l).volume + 1.5 * 1.5 * (1.0 - PI / 4.0) * 10.0;
    assert!(
        close(mass_props(&f).volume, want, 1e-12),
        "{} vs {want}",
        mass_props(&f).volume
    );
}

#[test]
fn a_fillet_between_two_cylinders_rolls_round_the_junction() {
    let main = cylinder(v3(-10.0, 0.0, 0.0), V3::X, 3.0, 20.0);
    let branch = cylinder(v3(0.0, 0.0, 0.0), V3::Z, 1.5, 10.0);
    let t = boolean(&main, &branch, Op::Union).unwrap();
    let v0 = mass_props(&t).volume;
    // The saddle curve where the branch meets the main tube.
    let e = (0..t.edges.len())
        .find(|e| matches!(t.edges[*e].curve, super::Curve::Traced(_)))
        .unwrap();
    let f = dress(&t, &[e], Dress::Fillet(0.5), "Fillet").unwrap();
    solid_ok(&f);
    let v1 = mass_props(&f).volume;
    assert!(v1 > v0, "a concave round adds material: {v1} vs {v0}");
    assert!(v1 - v0 < 1.0, "{}", v1 - v0);
    assert!(f
        .faces
        .iter()
        .any(|x| matches!(x.surface, super::Surface::Pipe { .. })));
}
