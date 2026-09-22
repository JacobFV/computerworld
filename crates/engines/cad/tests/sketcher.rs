//! The constraint solver on classic sketches: convergence, degrees of freedom,
//! redundant and conflicting constraints, dragging and editing dimensions.
use cw_cad::math::{radians, v2, V2};
use cw_cad::sketch::{
    tools, Constraint, ConstraintType as T, Geom, Pos, Sketch, SolveStatus, H_AXIS, V_AXIS,
};

fn c(kind: T, g: i32, p: Pos) -> Constraint {
    Constraint::new(kind, g, p)
}
fn close(a: V2, b: V2) -> bool {
    a.dist(b) < 1e-7
}

/// FreeCAD's rectangle tool output: 4 lines, 4 coincident, 2 horizontal, 2 vertical.
fn rectangle() -> (Sketch, Vec<i32>) {
    let mut s = Sketch::default();
    let ids = tools::rectangle(&mut s, v2(1.0, 1.0), v2(9.0, 5.0), false).unwrap();
    (s, ids)
}

#[test]
fn a_rectangle_is_under_constrained_until_placed_and_sized() {
    let (mut s, ids) = rectangle();
    let r = s.solve();
    assert_eq!(r.status, SolveStatus::UnderConstrained);
    assert_eq!(r.dof, 4);
    assert_eq!(r.message(), "Under constrained: 4 DoFs");
    // Lock the bottom-left corner at the origin (FreeCAD's Lock = DistanceX + DistanceY).
    s.add_constraint(c(T::Coincident, ids[0], Pos::Start).with_second(H_AXIS, Pos::Start))
        .unwrap();
    assert_eq!(s.solve().dof, 2);
    assert!(close(s.point(ids[0], Pos::Start).unwrap(), V2::ZERO));
    s.add_constraint(c(T::DistanceX, ids[0], Pos::None).with_value(30.0))
        .unwrap();
    let r = s.solve();
    assert_eq!(r.dof, 1);
    assert_eq!(r.message(), "Under constrained: 1 DoF");
    s.add_constraint(c(T::DistanceY, ids[1], Pos::None).with_value(20.0))
        .unwrap();
    let r = s.solve();
    assert_eq!(r.status, SolveStatus::FullyConstrained, "{}", r.message());
    assert_eq!(r.message(), "Fully constrained");
    assert_eq!(r.fully_constrained_geos, ids);
    assert!(close(s.point(ids[2], Pos::Start).unwrap(), v2(30.0, 20.0)));
    // Editing a dimension re-solves everything that depends on it.
    let width = s.constraints.len() - 2;
    s.constraints[width].value = 45.0;
    let r = s.solve();
    assert_eq!(r.status, SolveStatus::FullyConstrained);
    assert!(close(s.point(ids[1], Pos::End).unwrap(), v2(45.0, 20.0)));
    assert!(close(s.point(ids[2], Pos::End).unwrap(), v2(0.0, 20.0)));
}

#[test]
fn conflicting_constraints_are_named_and_leave_the_geometry_alone() {
    let (mut s, ids) = rectangle();
    s.add_constraint(c(T::DistanceX, ids[0], Pos::None).with_value(30.0))
        .unwrap();
    assert!(s.solve().solved());
    let before = s.geos.clone();
    // The top edge is equal to the bottom through the verticals; demanding otherwise
    // cannot be satisfied.
    s.add_constraint(c(T::DistanceX, ids[2], Pos::None).with_value(-10.0))
        .unwrap();
    let r = s.solve();
    assert_eq!(r.status, SolveStatus::Conflicting, "{}", r.message());
    assert_eq!(r.conflicting, vec![10], "{:?}", r.conflict_groups);
    assert!(r.conflict_groups.contains(&9) && r.conflict_groups.contains(&10));
    assert_eq!(r.message(), "Over-constrained: (10)");
    assert_eq!(s.geos, before, "a failed solve must not move anything");
}

#[test]
fn redundant_constraints_are_reported_but_the_sketch_still_solves() {
    let (mut s, ids) = rectangle();
    // Opposite sides of the rectangle are already parallel.
    s.add_constraint(c(T::Parallel, ids[0], Pos::None).with_second(ids[2], Pos::None))
        .unwrap();
    let r = s.solve();
    assert_eq!(r.status, SolveStatus::Redundant);
    assert_eq!(r.redundant, vec![9]);
    assert_eq!(r.message(), "Redundant constraints: (9)");
    assert!(r.solved());
}

#[test]
fn circles_tangents_and_radii() {
    let mut s = Sketch::default();
    let circle = tools::circle(&mut s, v2(0.5, 0.2), 3.0, false).unwrap();
    s.add_constraint(c(T::Coincident, circle, Pos::Center).with_second(H_AXIS, Pos::Start))
        .unwrap();
    s.add_constraint(c(T::Radius, circle, Pos::None).with_value(5.0))
        .unwrap();
    let r = s.solve();
    assert_eq!(r.status, SolveStatus::FullyConstrained, "{}", r.message());
    let line = tools::line(&mut s, v2(-10.0, 7.0), v2(10.0, 6.0), false, false).unwrap();
    s.add_constraint(c(T::Tangent, line, Pos::None).with_second(circle, Pos::None))
        .unwrap();
    s.add_constraint(c(T::Horizontal, line, Pos::None)).unwrap();
    let r = s.solve();
    assert!(r.solved(), "{}", r.message());
    let Some(Geom::Line { a, b }) = s.geom(line) else {
        panic!()
    };
    assert!((a.y - 5.0).abs() < 1e-9 && (b.y - 5.0).abs() < 1e-9);
    // 4 line parameters, 2 constraints: the line's two end x positions are free.
    assert_eq!(r.dof, 2);
    // A diameter on a second circle, equal to the first.
    let c2 = tools::circle(&mut s, v2(20.0, 0.0), 1.0, false).unwrap();
    s.add_constraint(c(T::Equal, circle, Pos::None).with_second(c2, Pos::None))
        .unwrap();
    s.solve();
    let Some(Geom::Circle { r, .. }) = s.geom(c2) else {
        panic!()
    };
    assert!((r - 5.0).abs() < 1e-9);
    s.add_constraint(c(T::Diameter, c2, Pos::None).with_value(10.0))
        .unwrap();
    assert_eq!(s.solve().status, SolveStatus::Redundant);
}

#[test]
fn a_slot_has_four_degrees_of_freedom_like_freecads() {
    let mut s = Sketch::default();
    tools::slot(&mut s, v2(0.0, 0.0), v2(20.0, 0.0), 4.0, false).unwrap();
    let r = s.solve();
    assert!(r.solved(), "{}", r.message());
    assert_eq!(r.dof, 4);
}

#[test]
fn symmetry_angles_perpendicularity_and_point_on_object() {
    let mut s = Sketch::default();
    let a = tools::point(&mut s, v2(-3.0, 1.0), false);
    let b = tools::point(&mut s, v2(4.0, 2.0), false);
    s.add_constraint(
        c(T::Symmetric, a, Pos::Start)
            .with_second(b, Pos::Start)
            .with_third(V_AXIS, Pos::None),
    )
    .unwrap();
    assert!(s.solve().solved());
    let (pa, pb) = (
        s.point(a, Pos::Start).unwrap(),
        s.point(b, Pos::Start).unwrap(),
    );
    assert!((pa.x + pb.x).abs() < 1e-9 && (pa.y - pb.y).abs() < 1e-9);

    let l1 = tools::line(&mut s, v2(0.0, 0.0), v2(10.0, 1.0), false, false).unwrap();
    s.add_constraint(c(T::Angle, l1, Pos::None).with_value(radians(30.0)))
        .unwrap();
    let l2 = tools::line(&mut s, v2(0.0, 0.0), v2(1.0, 10.0), false, false).unwrap();
    s.add_constraint(c(T::Perpendicular, l1, Pos::None).with_second(l2, Pos::None))
        .unwrap();
    let p = tools::point(&mut s, v2(3.0, 3.0), false);
    s.add_constraint(c(T::PointOnObject, p, Pos::Start).with_second(l1, Pos::None))
        .unwrap();
    let r = s.solve();
    assert!(r.solved(), "{}", r.message());
    let d1 = s.point(l1, Pos::End).unwrap() - s.point(l1, Pos::Start).unwrap();
    let d2 = s.point(l2, Pos::End).unwrap() - s.point(l2, Pos::Start).unwrap();
    assert!((d1.angle() - radians(30.0)).abs() < 1e-9);
    assert!(d1.dot(d2).abs() < 1e-9 * d1.len() * d2.len());
    let q = s.point(p, Pos::Start).unwrap();
    assert!(
        s.geom(l1).unwrap().distance(q) < 1e-7 || {
            let a0 = s.point(l1, Pos::Start).unwrap();
            (q - a0).cross(d1).abs() / d1.len() < 1e-9
        }
    );
}

#[test]
fn dragging_moves_what_is_free_and_nothing_that_is_fixed() {
    let (mut s, ids) = rectangle();
    let r = s.drag(ids[1], Pos::End, v2(12.0, 8.0));
    assert!(r.solved());
    // The corner went where it was dragged and the rectangle stayed a rectangle.
    assert!(close(s.point(ids[1], Pos::End).unwrap(), v2(12.0, 8.0)));
    assert!(close(s.point(ids[2], Pos::Start).unwrap(), v2(12.0, 8.0)));
    let (bl, br) = (
        s.point(ids[0], Pos::Start).unwrap(),
        s.point(ids[0], Pos::End).unwrap(),
    );
    assert!((bl.y - br.y).abs() < 1e-9);
    // The opposite corner did not move.
    assert!(close(bl, v2(1.0, 1.0)));
    // Fully constrain, then a drag changes nothing.
    s.add_constraint(c(T::Coincident, ids[0], Pos::Start).with_second(H_AXIS, Pos::Start))
        .unwrap();
    s.add_constraint(c(T::DistanceX, ids[0], Pos::None).with_value(10.0))
        .unwrap();
    s.add_constraint(c(T::DistanceY, ids[1], Pos::None).with_value(5.0))
        .unwrap();
    assert_eq!(s.solve().status, SolveStatus::FullyConstrained);
    let before = s.geos.clone();
    s.drag(ids[1], Pos::End, v2(40.0, 40.0));
    for (x, y) in before.iter().zip(&s.geos) {
        for (p, q) in x.geom.params().iter().zip(y.geom.params()) {
            assert!((p - q).abs() < 1e-9);
        }
    }
}

#[test]
fn solving_is_bit_for_bit_repeatable() {
    let build = || {
        let (mut s, ids) = rectangle();
        tools::fillet(&mut s, ids[2], Pos::End, 1.5).unwrap();
        s.add_constraint(c(T::DistanceX, ids[0], Pos::None).with_value(17.25))
            .unwrap();
        s.solve();
        serde_json::to_string(&s).unwrap()
    };
    assert_eq!(build(), build());
}

#[test]
fn constraints_that_do_not_fit_their_geometry_are_refused() {
    let mut s = Sketch::default();
    let circle = tools::circle(&mut s, V2::ZERO, 2.0, false).unwrap();
    assert!(s
        .add_constraint(c(T::Horizontal, circle, Pos::None))
        .is_err());
    assert!(s
        .add_constraint(c(T::Radius, circle, Pos::None).with_value(-1.0))
        .is_err());
    assert!(s
        .add_constraint(c(T::Coincident, circle, Pos::Start).with_second(H_AXIS, Pos::Start))
        .is_err());
}
