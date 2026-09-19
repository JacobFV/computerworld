//! FreeCAD application tests: commands against the model, pointer work in the 3D view,
//! every painted control in many states, files in and out, and snapshot round trips.
use super::*;
use crate::desktop_scene::app_content_with;
use crate::{AppState, NativeApp, PointerPhase};
use cw_cad::document::Shape;
use cw_cad::math::{v2, v3};
use cw_cad::sketch::{tools, ConstraintType as T, Geom, SolveStatus};

const W: u64 = 1;
const VIEW: (u32, u32) = (800, 600);

fn cad() -> Cad {
    let (app, _) = Freecad::launch("", W, 0);
    let mut c = *app.0;
    c.home = "/home/carol".into();
    c.view_size = VIEW;
    c
}
fn sketch_of(c: &mut Cad) -> &mut cw_cad::sketch::Sketch {
    let name = c.sketch_edit().unwrap().name.clone();
    match &mut c.doc.get_mut(&name).unwrap().feature {
        Feature::Sketch { sketch, .. } => sketch,
        _ => unreachable!(),
    }
}
/// Pixel in the 3D view where sketch point `p` of the open sketch is drawn.
fn px(c: &Cad, p: V2) -> (i32, i32) {
    let f = c.sketch_frame().unwrap();
    let (x, y, _) = c.camera.project(f.to_world(p), VIEW.0, VIEW.1).unwrap();
    (x.round() as i32, y.round() as i32)
}
fn click(c: &mut Cad, (x, y): (i32, i32)) {
    c.pointer(W, PointerPhase::Down, x, y).unwrap();
    c.pointer(W, PointerPhase::Up, x, y).unwrap();
}
/// Click sketch point `p` in the 3D view.
fn tap(c: &mut Cad, p: V2) {
    let at = px(c, p);
    click(c, at);
}
/// Pixel where world point `p` is drawn.
fn wpx(c: &Cad, p: V3) -> (i32, i32) {
    let (x, y, _) = c.camera.project(p, VIEW.0, VIEW.1).unwrap();
    (x.round() as i32, y.round() as i32)
}
fn body_shape(c: &Cad) -> Arc<Shape> {
    let b = c.body().unwrap();
    c.model().body_shape[&b].clone()
}

/// A body with a 40 x 20 x 10 pad from a sketch on XY.
fn padded() -> Cad {
    let mut c = cad();
    c.run(W, "PartDesign_NewSketch").unwrap();
    c.task_command(W, "ok").unwrap();
    tools::rectangle(sketch_of(&mut c), v2(0.0, 0.0), v2(40.0, 20.0), false).unwrap();
    c.leave_sketch();
    c.run(W, "PartDesign_Pad").unwrap();
    c.task_command(W, "ok").unwrap();
    c
}

#[test]
fn a_sketch_on_a_picked_face_is_placed_on_it() {
    let mut c = padded();
    c.set_view(StdView::Isometric);
    c.fit_all();
    let at = wpx(&c, v3(20.0, 10.0, 10.0));
    click(&mut c, at);
    assert!(c.selection[0].sub.starts_with("Face"), "{:?}", c.selection);
    c.run(W, "PartDesign_NewSketch").unwrap();
    let e = c
        .sketch_edit()
        .expect("editing the new sketch")
        .name
        .clone();
    let frame = c.model().frames[&e];
    assert!((frame.origin.z - 10.0).abs() < 1e-9);
    assert!(c.sketch_point(400.0, 300.0).is_some());
}

#[test]
fn sketch_tools_draw_what_is_clicked_with_freecads_constraints() {
    let mut c = cad();
    c.run(W, "PartDesign_NewSketch").unwrap();
    c.task_command(W, "ok").unwrap();
    c.camera.half_height = 60.0;
    c.camera.target = V3::ZERO;
    // A line clicked from the root point: it starts coincident with the origin and,
    // drawn level, is constrained horizontal.
    c.run(W, "Sketcher_CreateLine").unwrap();
    tap(&mut c, V2::ZERO);
    tap(&mut c, v2(30.0, 0.2));
    let s = sketch_of(&mut c).clone();
    assert!(matches!(s.geom(0), Some(Geom::Line { .. })));
    assert!(s.constraints.iter().any(|k| k.kind == T::Horizontal));
    assert!(s
        .constraints
        .iter()
        .any(|k| k.kind == T::Coincident && k.second == cw_cad::sketch::H_AXIS));
    // The tool is still active (continuous mode); a second line from the first's end
    // is joined to it.
    tap(&mut c, v2(30.0, 0.0));
    tap(&mut c, v2(30.0, 20.0));
    let s = sketch_of(&mut c).clone();
    assert!(s
        .constraints
        .iter()
        .any(|k| k.kind == T::Coincident && k.first == 1 && k.second == 0));
    assert!(s
        .constraints
        .iter()
        .any(|k| k.kind == T::Vertical && k.first == 1));
    c.key(W, "Escape").unwrap();
    // Circle, arcs, polyline, slot.
    c.run(W, "Sketcher_CreateCircle").unwrap();
    tap(&mut c, v2(-20.0, 10.0));
    tap(&mut c, v2(-15.0, 10.0));
    c.run(W, "Sketcher_Create3PointArc").unwrap();
    for p in [v2(-30.0, -10.0), v2(-10.0, -10.0), v2(-20.0, -4.0)] {
        tap(&mut c, p);
    }
    c.run(W, "Sketcher_CreatePolyline").unwrap();
    for p in [v2(10.0, 30.0), v2(20.0, 35.0), v2(15.0, 40.0)] {
        tap(&mut c, p);
    }
    c.key(W, "Enter").unwrap();
    c.run(W, "Sketcher_CreateSlot").unwrap();
    for p in [v2(-30.0, 30.0), v2(-10.0, 30.0), v2(-20.0, 33.0)] {
        tap(&mut c, p);
    }
    c.key(W, "Escape").unwrap();
    let s = sketch_of(&mut c).clone();
    let kinds: Vec<&str> = s.geos.iter().map(|g| g.geom.type_name()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "Circle").count(), 1);
    assert!(
        kinds.iter().filter(|k| **k == "Arc of circle").count() >= 3,
        "{kinds:?}"
    );
    let Some(Geom::Circle { r, .. }) = s
        .geos
        .iter()
        .map(|g| g.geom.clone())
        .find(|g| matches!(g, Geom::Circle { .. }))
    else {
        panic!()
    };
    assert!((r - 5.0).abs() < 0.2, "the rim click sets the radius: {r}");
    assert!(c.sketch_edit().unwrap().report.solved());
}

#[test]
fn constraining_a_rectangle_fully_by_selection_and_dimension_dialogs() {
    let mut c = cad();
    c.run(W, "PartDesign_NewSketch").unwrap();
    c.task_command(W, "ok").unwrap();
    c.run(W, "Sketcher_CreateRectangle").unwrap();
    tap(&mut c, V2::ZERO);
    tap(&mut c, v2(25.0, 12.0));
    c.key(W, "Escape").unwrap();
    assert_eq!(c.sketch_edit().unwrap().report.dof, 2);
    // Select the bottom edge by clicking it in the view, then a horizontal distance.
    tap(&mut c, v2(12.0, 0.0));
    assert_eq!(c.sketch_edit().unwrap().picked, vec![(0, Pos::None)]);
    c.run(W, "Sketcher_ConstrainDistanceX").unwrap();
    assert!(matches!(c.dialog, Some(Dialog::Dimension { .. })));
    c.type_text("50").unwrap();
    c.key(W, "Enter").unwrap();
    assert!(c.dialog.is_none());
    // A constraint command with nothing selected waits for its picks.
    c.run(W, "Sketcher_ConstrainDistanceY").unwrap();
    assert_eq!(
        c.sketch_edit().unwrap().pending.as_deref(),
        Some("Sketcher_ConstrainDistanceY")
    );
    tap(&mut c, v2(50.0, 6.0));
    c.type_text("30").unwrap();
    c.key(W, "Enter").unwrap();
    let e = c.sketch_edit().unwrap();
    assert_eq!(
        e.report.status,
        SolveStatus::FullyConstrained,
        "{}",
        e.report.message()
    );
    let s = sketch_of(&mut c).clone();
    assert!(s.point(2, Pos::Start).unwrap().dist(v2(50.0, 30.0)) < 1e-9);
    // Double-clicking a dimension edits it; the sketch re-solves.
    let dim = s
        .constraints
        .iter()
        .position(|k| k.kind == T::DistanceX)
        .unwrap();
    c.double_click(W, &format!("sk:dim:{dim}")).unwrap();
    c.type_text("60 mm").unwrap();
    c.key(W, "Enter").unwrap();
    assert!(
        sketch_of(&mut c)
            .point(1, Pos::End)
            .unwrap()
            .dist(v2(60.0, 30.0))
            < 1e-9
    );
    // A conflicting constraint is reported by number and moves nothing.
    tap(&mut c, v2(30.0, 30.0));
    c.run(W, "Sketcher_ConstrainDistanceX").unwrap();
    c.type_text("10").unwrap();
    let _ = c.key(W, "Enter");
    let r = &c.sketch_edit().unwrap().report;
    assert_eq!(r.status, SolveStatus::Conflicting, "{}", r.message());
    assert!(r.message().starts_with("Over-constrained"));
    // Undo takes the conflicting constraint back out.
    c.key(W, "Ctrl+z").unwrap();
    c.key(W, "Ctrl+z").unwrap();
    assert_eq!(
        c.sketch_edit().unwrap().report.status,
        SolveStatus::FullyConstrained
    );
}

#[test]
fn dragging_sketch_geometry_goes_through_the_solver() {
    let mut c = cad();
    c.run(W, "PartDesign_NewSketch").unwrap();
    c.task_command(W, "ok").unwrap();
    tools::rectangle(sketch_of(&mut c), v2(0.0, 0.0), v2(20.0, 10.0), false).unwrap();
    c.refresh_sketch_report();
    let (x0, y0) = px(&c, v2(20.0, 10.0));
    let (x1, y1) = px(&c, v2(30.0, 25.0));
    c.pointer(W, PointerPhase::Down, x0, y0).unwrap();
    for k in 1..=5 {
        c.pointer(
            W,
            PointerPhase::Move,
            x0 + (x1 - x0) * k / 5,
            y0 + (y1 - y0) * k / 5,
        )
        .unwrap();
    }
    c.pointer(W, PointerPhase::Up, x1, y1).unwrap();
    let s = sketch_of(&mut c).clone();
    let corner = s.point(1, Pos::End).unwrap();
    assert!(
        corner.dist(v2(30.0, 25.0)) < 0.5,
        "the corner followed the pointer: {corner:?}"
    );
    // Still a rectangle: the neighbours followed through their constraints.
    assert!((s.point(2, Pos::Start).unwrap() - corner).len() < 1e-9);
    assert!((s.point(0, Pos::End).unwrap().x - corner.x).abs() < 1e-9);
    // One undo step for the whole drag.
    c.key(W, "Ctrl+z").unwrap();
    assert_eq!(sketch_of(&mut c).point(1, Pos::End), Some(v2(20.0, 10.0)));
}

#[test]
fn trim_extend_and_fillet_by_clicking() {
    let mut c = cad();
    c.run(W, "PartDesign_NewSketch").unwrap();
    c.task_command(W, "ok").unwrap();
    {
        let s = sketch_of(&mut c);
        tools::line(s, v2(-20.0, 0.0), v2(20.0, 0.0), false, false).unwrap();
        tools::line(s, v2(-10.0, -10.0), v2(-10.0, 10.0), false, false).unwrap();
        tools::line(s, v2(10.0, -10.0), v2(10.0, 10.0), false, false).unwrap();
    }
    c.run(W, "Sketcher_Trimming").unwrap();
    tap(&mut c, v2(3.0, 0.0));
    assert_eq!(
        sketch_of(&mut c).geos.len(),
        4,
        "the middle was cut out, leaving two halves"
    );
    c.key(W, "Escape").unwrap();
    // Extend a short line up to a boundary.
    {
        let s = sketch_of(&mut c);
        tools::line(s, v2(0.0, 20.0), v2(0.0, 25.0), false, false).unwrap();
        tools::line(s, v2(-5.0, 40.0), v2(5.0, 40.0), false, false).unwrap();
    }
    c.run(W, "Sketcher_Extend").unwrap();
    tap(&mut c, v2(0.0, 24.5));
    tap(&mut c, v2(3.0, 40.0));
    assert_eq!(sketch_of(&mut c).point(4, Pos::End), Some(v2(0.0, 40.0)));
    c.key(W, "Escape").unwrap();
    // Fillet a rectangle's corner with the tool's radius.
    let ids = tools::rectangle(sketch_of(&mut c), v2(30.0, 0.0), v2(50.0, 10.0), false).unwrap();
    c.run(W, "Sketcher_CreateFillet").unwrap();
    c.sketch_command("fillet-radius").unwrap();
    c.type_text("3").unwrap();
    c.key(W, "Enter").unwrap();
    tap(&mut c, v2(50.0, 10.0));
    let s = sketch_of(&mut c).clone();
    let arc = s.geos.last().unwrap().geom.clone();
    let Geom::Arc { c: center, r, .. } = arc else {
        panic!("{arc:?}")
    };
    assert!((r - 3.0).abs() < 1e-9 && center.dist(v2(47.0, 7.0)) < 1e-9);
    assert!(s.point(ids[1], Pos::End).unwrap().dist(v2(50.0, 7.0)) < 1e-9);
}

#[test]
fn features_are_made_edited_and_recomputed_through_their_panels() {
    let mut c = padded();
    assert!((body_shape(&c).volume() - 8000.0).abs() < 1e-6);
    // Edit the pad's length in the property editor: everything downstream follows.
    c.command(W, "tree:Pad", None).unwrap();
    c.command(W, "prop:Length", None).unwrap();
    c.type_text("15").unwrap();
    c.key(W, "Enter").unwrap();
    assert!((body_shape(&c).volume() - 12000.0).abs() < 1e-6);
    // Reopen the pad's panel by double-click; Cancel restores it untouched.
    c.double_click(W, "tree:Pad").unwrap();
    c.command(W, "field:task:Length", None).unwrap();
    c.type_text("99").unwrap();
    c.key(W, "Enter").unwrap();
    assert!(
        (body_shape(&c).volume() - 79200.0).abs() < 1e-6,
        "the panel previews live"
    );
    c.task_command(W, "cancel").unwrap();
    assert!((body_shape(&c).volume() - 12000.0).abs() < 1e-6);
    // Fillet a picked edge.
    c.set_view(StdView::Isometric);
    c.fit_all();
    let at = wpx(&c, v3(20.0, 0.0, 15.0));
    click(&mut c, at);
    assert!(c.selection[0].sub.starts_with("Edge"), "{:?}", c.selection);
    c.run(W, "PartDesign_Fillet").unwrap();
    c.command(W, "field:task:Radius", None).unwrap();
    c.type_text("2").unwrap();
    c.key(W, "Enter").unwrap();
    c.task_command(W, "ok").unwrap();
    let shape = body_shape(&c);
    assert!(shape.mesh.is_watertight());
    let removed = 12000.0 - shape.volume();
    assert!(removed > 0.8 * 40.0 && removed < 4.0 * 40.0, "{removed}");
    // A polar pattern of the fillet is refused (only additive/subtractive tools pattern).
    c.command(W, "tree:Fillet", None).unwrap();
    assert!(c.available("PartDesign_PolarPattern").is_err());
    // A pocket, then a linear pattern of it chosen in the tree.
    c.run(W, "PartDesign_NewSketch").unwrap();
    c.task_command(W, "ok").unwrap();
    tools::circle(sketch_of(&mut c), v2(8.0, 10.0), 2.0, false).unwrap();
    c.leave_sketch();
    c.run(W, "PartDesign_Pocket").unwrap();
    c.command(W, "choice:task/Type:Through all", None).unwrap();
    c.command(W, "task:toggle:Reversed", None).unwrap();
    c.task_command(W, "ok").unwrap();
    let before = body_shape(&c).volume();
    c.command(W, "tree:Pocket", None).unwrap();
    c.run(W, "PartDesign_LinearPattern").unwrap();
    c.command(W, "field:task:Length", None).unwrap();
    c.type_text("24").unwrap();
    c.key(W, "Enter").unwrap();
    c.command(W, "field:task:Occurrences", None).unwrap();
    c.type_text("3").unwrap();
    c.key(W, "Enter").unwrap();
    c.task_command(W, "ok").unwrap();
    let after = body_shape(&c).volume();
    let hole = before - after;
    assert!(hole > 0.0);
    assert!(body_shape(&c).mesh.is_watertight());
    assert!(c.model().error("LinearPattern").is_none());
}

#[test]
fn revolution_hole_and_mirror_from_the_toolbar() {
    let mut c = cad();
    c.run(W, "PartDesign_NewSketch").unwrap();
    c.task_command(W, "plane:XZ_Plane").unwrap();
    c.task_command(W, "ok").unwrap();
    tools::rectangle(sketch_of(&mut c), v2(5.0, 0.0), v2(15.0, 10.0), false).unwrap();
    c.leave_sketch();
    c.run(W, "PartDesign_Revolution").unwrap();
    c.task_command(W, "ok").unwrap();
    let v = body_shape(&c).volume();
    let want = std::f64::consts::PI * (225.0 - 25.0) * 10.0;
    assert!((v - want).abs() < 1e-9, "{v} vs {want}");
    // A hole in the top ring, from a sketch on the top face.
    let shape = body_shape(&c);
    let top = (0..shape.topo.faces.len())
        .find(|i| {
            matches!(shape.mesh.surfaces[shape.topo.faces[*i].surface as usize], cw_cad::mesh::Surface::Plane { normal, .. } if normal.z > 0.9)
        })
        .unwrap();
    c.selection = vec![Sel {
        object: "Revolution".into(),
        sub: format!("Face{}", top + 1),
        point: V3::ZERO,
    }];
    c.run(W, "PartDesign_NewSketch").unwrap();
    tools::circle(sketch_of(&mut c), v2(10.0, 0.0), 1.0, false).unwrap();
    c.leave_sketch();
    c.run(W, "PartDesign_Hole").unwrap();
    c.task_command(W, "ok").unwrap();
    let with_hole = body_shape(&c).volume();
    assert!(
        with_hole < v - 100.0,
        "a 6 mm hole 10 mm deep removed material: {v} -> {with_hole}"
    );
    assert!(body_shape(&c).mesh.is_watertight());
    // Mirror the hole across the YZ plane.
    c.command(W, "tree:Hole", None).unwrap();
    c.run(W, "PartDesign_Mirrored").unwrap();
    c.command(W, "choice:task/Plane:YZ_Plane", None).unwrap();
    c.task_command(W, "ok").unwrap();
    let mirrored = body_shape(&c).volume();
    assert!(((v - with_hole) * 2.0 - (v - mirrored)).abs() < 1e-6);
}

#[test]
fn measuring_faces_edges_and_bodies() {
    let mut c = padded();
    c.run(W, "Std_Measure").unwrap();
    c.command(W, "tree:Body", None).unwrap();
    let m = c.measurements();
    let get = |m: &[(String, String)], k: &str| {
        m.iter()
            .find(|(a, _)| a.contains(k))
            .map(|(_, v)| v.clone())
    };
    assert_eq!(get(&m, "volume").as_deref(), Some("8000 mm³"));
    assert_eq!(get(&m, "Surface area").as_deref(), Some("2800 mm²"));
    assert_eq!(get(&m, "Center of mass").as_deref(), Some("(20, 10, 5) mm"));
    assert_eq!(get(&m, "Bounding box").as_deref(), Some("40 × 20 × 10 mm"));
    // Two opposite faces: their distance; two edges at right angles: 90°.
    let shape = body_shape(&c);
    let face = |n: V3| {
        (0..shape.topo.faces.len())
            .find(|i| {
                (cw_cad::document::face_normal(&shape.mesh, &shape.topo, *i) - n).len() < 1e-9
            })
            .unwrap()
    };
    c.selection = vec![
        Sel {
            object: "Pad".into(),
            sub: format!("Face{}", face(V3::Z) + 1),
            point: V3::ZERO,
        },
        Sel {
            object: "Pad".into(),
            sub: format!("Face{}", face(-V3::Z) + 1),
            point: V3::ZERO,
        },
    ];
    let m = c.measurements();
    assert_eq!(get(&m, "Distance").as_deref(), Some("10 mm"));
    assert_eq!(
        get(&m, "Angle").as_deref(),
        Some("180 °"),
        "opposite normals"
    );
    c.selection = vec![
        Sel {
            object: "Pad".into(),
            sub: format!("Face{}", face(V3::Z) + 1),
            point: V3::ZERO,
        },
        Sel {
            object: "Pad".into(),
            sub: format!("Face{}", face(V3::X) + 1),
            point: V3::ZERO,
        },
    ];
    assert_eq!(get(&c.measurements(), "Angle").as_deref(), Some("90 °"));
    c.selection = vec![Sel {
        object: "Pad".into(),
        sub: format!("Face{}", face(V3::Z) + 1),
        point: V3::ZERO,
    }];
    assert_eq!(get(&c.measurements(), "Area").as_deref(), Some("800 mm²"));
}

#[test]
fn files_round_trip_through_the_dialog_and_effects() {
    let mut c = padded();
    // Export STL (binary) with the Body selected.
    c.command(W, "tree:Body", None).unwrap();
    let effects = c.run(W, "Std_Export").unwrap();
    assert!(
        matches!(&effects[0], AppEffect::ListDirectory { path, .. } if path == "/home/carol/Documents")
    );
    c.listed(vec!["notes/".into(), "old.stl".into(), "readme.txt".into()]);
    // STEP comes first in the list of types, as FreeCAD's export dialog offers it.
    assert_eq!(file_dialog(&c).extension(), "step");
    c.command(W, "choice:filetype:2", None).unwrap();
    let Some(Dialog::File(d)) = &c.dialog else {
        panic!()
    };
    assert_eq!(
        d.visible(),
        vec!["notes/", "old.stl"],
        "folders and matching files only"
    );
    let effects = c.file_command(W, "ok").unwrap();
    let AppEffect::WriteBytes { path, bytes, .. } = &effects[0] else {
        panic!("{effects:?}")
    };
    assert_eq!(path, "/home/carol/Documents/Body.stl");
    let mesh = cw_cad::io::read_stl(bytes).unwrap();
    assert!((mesh.volume() - 8000.0).abs() < 1e-3);
    c.written(path);
    // STEP, OBJ and SVG.
    for (ty, name, check) in [
        (0, "Body.step", "MANIFOLD_SOLID_BREP"),
        (
            1,
            "Body.stp",
            "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF",
        ),
        (4, "Body.obj", "o Body"),
        (6, "Body.svg", "<svg"),
    ] {
        c.run(W, "Std_Export").unwrap();
        c.file_command(W, &format!("type:{ty}")).unwrap();
        let effects = c.file_command(W, "ok").unwrap();
        let AppEffect::WriteFile { path, content, .. } = &effects[0] else {
            panic!()
        };
        assert!(path.ends_with(name), "{path}");
        assert!(content.contains(check));
    }
    // DXF exports a sketch.
    c.command(W, "tree:Sketch", None).unwrap();
    c.run(W, "Std_Export").unwrap();
    c.file_command(W, "type:5").unwrap();
    let effects = c.file_command(W, "ok").unwrap();
    let AppEffect::WriteFile { content, .. } = &effects[0] else {
        panic!()
    };
    let (s, _) = cw_cad::io::read_dxf(content).unwrap();
    assert_eq!(s.geos.len(), 4);
    // Import the STL back as a mesh object.
    c.run(W, "Std_Import").unwrap();
    c.listed(vec!["Body.stl".into()]);
    c.file_command(W, "entry:Body.stl").unwrap();
    let effects = c.file_command(W, "ok").unwrap();
    let AppEffect::ReadBytes { path, .. } = &effects[0] else {
        panic!()
    };
    c.bytes_loaded(path, Ok(cw_cad::io::stl_binary(&body_shape(&c).mesh, "x")))
        .unwrap();
    let mesh_obj = c
        .doc
        .objects
        .iter()
        .find(|o| matches!(o.feature, Feature::Mesh { .. }))
        .unwrap();
    assert_eq!(mesh_obj.label, "Body");
    // Save and load the document.
    let effects = c.run(W, "Std_Save").unwrap();
    assert!(matches!(&effects[0], AppEffect::ListDirectory { .. }));
    let effects = c.file_command(W, "ok").unwrap();
    let AppEffect::WriteFile { path, content, .. } = &effects[0] else {
        panic!()
    };
    assert_eq!(path, "/home/carol/Documents/Unnamed.FCStd.json");
    c.written(path);
    assert!(!c.modified);
    let mut fresh = cad();
    fresh.io_read = Some(files::Pending {
        purpose: files::Purpose::Open,
        path: path.clone(),
    });
    fresh
        .bytes_loaded(path, Ok(content.as_bytes().to_vec()))
        .unwrap();
    assert_eq!(fresh.doc, c.doc);
    // A file that will not parse is reported, not swallowed.
    fresh.io_read = Some(files::Pending {
        purpose: files::Purpose::Import,
        path: "/x/bad.stl".into(),
    });
    fresh
        .bytes_loaded("/x/bad.stl", Ok(b"nonsense".to_vec()))
        .unwrap();
    assert!(matches!(fresh.dialog, Some(Dialog::Message { .. })));
}

fn file_dialog(c: &Cad) -> &files::FileDialog {
    match &c.dialog {
        Some(Dialog::File(d)) => d,
        other => panic!("no file dialog: {other:?}"),
    }
}
fn listing_of(effects: &[AppEffect]) -> Option<&str> {
    effects.iter().rev().find_map(|e| match e {
        AppEffect::ListDirectory { path, .. } => Some(path.as_str()),
        _ => None,
    })
}

#[test]
fn file_dialog_history_places_path_bar_and_up() {
    let mut c = cad();
    let effects = c.run(W, "Std_SaveAs").unwrap();
    assert_eq!(listing_of(&effects), Some("/home/carol/Documents"));
    c.listed(vec!["Parts/".into(), "a.FCStd.json".into()]);
    // A sidebar place, a path bar segment, Up: each lists the machine's folder.
    let e = c.file_command(W, "place:/home/carol/Desktop").unwrap();
    assert_eq!(listing_of(&e), Some("/home/carol/Desktop"));
    assert!(file_dialog(&c).loading && file_dialog(&c).entries.is_empty());
    c.listed(vec![]);
    let e = c.file_command(W, "crumb:/home").unwrap();
    assert_eq!(listing_of(&e), Some("/home"));
    let e = c.file_command(W, "up").unwrap();
    assert_eq!(listing_of(&e), Some("/"));
    assert!(c.file_command(W, "up").is_err(), "nothing above the root");
    // Back walks the way it came, Forward returns; a new step drops Forward.
    let back = |c: &mut Cad| {
        let e = c.file_command(W, "back").unwrap();
        listing_of(&e).unwrap().to_owned()
    };
    assert_eq!(back(&mut c), "/home");
    assert_eq!(back(&mut c), "/home/carol/Desktop");
    assert_eq!(back(&mut c), "/home/carol/Documents");
    assert!(c.file_command(W, "back").is_err());
    let e = c.file_command(W, "forward").unwrap();
    assert_eq!(listing_of(&e), Some("/home/carol/Desktop"));
    assert_eq!(file_dialog(&c).forward.len(), 2);
    c.file_command(W, "place:/home/carol/Music").unwrap();
    assert!(file_dialog(&c).forward.is_empty());
    assert!(c.file_command(W, "forward").is_err());
    // The keyboard's history keys do the same.
    c.key(W, "Alt+ArrowLeft").unwrap();
    assert_eq!(file_dialog(&c).folder, "/home/carol/Desktop");
    c.key(W, "Ctrl+]").unwrap();
    assert_eq!(file_dialog(&c).folder, "/home/carol/Music");
    // What was typed in the name box survives the trip.
    c.type_text("Bracket").unwrap();
    c.file_command(W, "place:/home/carol/Documents").unwrap();
    assert_eq!(file_dialog(&c).name, "Bracket");
    // Explorer's Home: the pinned folders the home folder really has.
    c.file_command(W, "home").unwrap();
    c.listed(vec![
        "Videos/".into(),
        "Documents/".into(),
        "notes.txt".into(),
        ".config/".into(),
        "Desktop/".into(),
    ]);
    let d = file_dialog(&c);
    assert!(d.home_view);
    assert_eq!(d.visible(), vec!["Desktop/", "Documents/", "Videos/"]);
    assert!(c.file_command(W, "up").is_err());
    assert!(
        c.file_command(W, "ok").is_err(),
        "Home is not a folder to save in"
    );
    c.file_command(W, "open:Documents/").unwrap();
    let d = file_dialog(&c);
    assert!(!d.home_view);
    assert_eq!(d.folder, "/home/carol/Documents");
    c.file_command(W, "back").unwrap();
    assert!(file_dialog(&c).home_view, "Back returns to Home");
}

#[test]
fn file_dialog_new_folder_the_platform_way() {
    // Explorer: "New folder" at once, the next free name, selected, listed again.
    let mut c = cad();
    c.run(W, "Std_SaveAs").unwrap();
    c.listed(vec!["New folder/".into(), "new folder (2)/".into()]);
    let e = c.file_command(W, "new-folder").unwrap();
    assert!(matches!(&e[0], AppEffect::CreateDirectory { path, .. }
        if path == "/home/carol/Documents/New folder (3)"));
    assert_eq!(listing_of(&e), Some("/home/carol/Documents"));
    assert_eq!(file_dialog(&c).selected.as_deref(), Some("New folder (3)/"));
    // The Mac sheet and GTK's popover: a name, checked, then into the new folder.
    c.platform = Some(DesktopTheme::Macos);
    c.listed(vec!["Parts/".into(), "a.FCStd.json".into()]);
    c.file_command(W, "folder-prompt:untitled folder").unwrap();
    assert!(file_dialog(&c).prompt.is_some());
    assert_eq!(c.field.as_ref().unwrap().text, "untitled folder");
    c.type_text("Parts").unwrap();
    let refused = c.key(W, "Enter").unwrap_err();
    assert!(refused.contains("already taken"), "{refused}");
    assert_eq!(
        file_dialog(&c).prompt.as_ref().unwrap().error.as_deref(),
        Some(refused.as_str())
    );
    assert_eq!(
        c.field.as_ref().unwrap().text,
        "Parts",
        "the box keeps the name"
    );
    c.key(W, "Backspace").unwrap();
    c.type_text("s/x").unwrap();
    assert!(c.key(W, "Enter").unwrap_err().contains("“/”"));
    for _ in 0..2 {
        c.key(W, "Backspace").unwrap();
    }
    c.type_text("Jigs").unwrap();
    let e = c.key(W, "Enter").unwrap();
    assert!(matches!(&e[0], AppEffect::CreateDirectory { path, .. }
        if path == "/home/carol/Documents/PartsJigs"));
    assert_eq!(listing_of(&e), Some("/home/carol/Documents/PartsJigs"));
    let d = file_dialog(&c);
    assert!(d.prompt.is_none());
    assert_eq!(d.folder, "/home/carol/Documents/PartsJigs");
    assert!(matches!(
        c.field,
        Some(Field {
            target: FieldTarget::FileName,
            ..
        })
    ));
    // Escape closes the prompt, not the dialog; GTK words a clash its own way.
    c.platform = Some(DesktopTheme::Ubuntu);
    c.listed(vec!["x/".into(), "y.FCStd.json".into()]);
    c.file_command(W, "folder-prompt:").unwrap();
    c.key(W, "Escape").unwrap();
    assert!(file_dialog(&c).prompt.is_none());
    c.file_command(W, "folder-prompt:").unwrap();
    c.type_text("y.FCStd.json").unwrap();
    assert_eq!(
        c.file_command(W, "folder-create").unwrap_err(),
        "A file with that name already exists"
    );
    // Anything else in the dialog closes the popover.
    c.file_command(W, "entry:x/").unwrap();
    assert!(file_dialog(&c).prompt.is_none());
}

#[test]
fn step_exports_the_exact_solid_and_imports_it_back() {
    let mut c = padded();
    c.clock_us = 1_700_000_000_000_000;
    c.command(W, "tree:Body", None).unwrap();
    c.run(W, "Std_Export").unwrap();
    c.listed(vec![]);
    let effects = c.file_command(W, "ok").unwrap();
    let AppEffect::WriteFile { path, content, .. } = &effects[0] else {
        panic!("{effects:?}")
    };
    assert_eq!(path, "/home/carol/Documents/Body.step");
    assert!(content.contains("MANIFOLD_SOLID_BREP('Body'"));
    // The header carries the world's date, not the host's.
    let stamp = c.timestamp();
    assert!(content.contains(&stamp), "{stamp}");
    assert_eq!(stamp.len(), 19);
    let text = content.clone();
    c.written(path);
    // Import it: an exact Part::Feature with the same volume.
    c.run(W, "Std_Import").unwrap();
    c.listed(vec!["Body.step".into()]);
    c.file_command(W, "entry:Body.step").unwrap();
    let effects = c.file_command(W, "ok").unwrap();
    let AppEffect::ReadBytes { path, .. } = &effects[0] else {
        panic!()
    };
    let path = path.clone();
    c.bytes_loaded(&path, Ok(text.into_bytes())).unwrap();
    let part = c
        .doc
        .objects
        .iter()
        .find(|o| matches!(o.feature, Feature::Part { .. }))
        .expect("an imported solid");
    assert_eq!(part.label, "Body");
    let model = c.model();
    let shape = &model.shapes[&part.name];
    assert!((shape.volume() - 8000.0).abs() < 1e-9, "{}", shape.volume());
    assert_eq!(shape.topo.faces.len(), 6);
    // It can be measured and exported again.
    c.selection = vec![Sel {
        object: part.name.clone(),
        sub: String::new(),
        point: V3::ZERO,
    }];
    let m = c.measurements();
    let volume = m
        .iter()
        .find(|(k, _)| k.contains("volume"))
        .map(|(_, v)| v.clone());
    assert_eq!(volume.as_deref(), Some("8000 mm³"));
}

#[test]
fn file_dialog_type_filter_changes_listing_and_extension() {
    let mut c = padded();
    c.command(W, "tree:Body", None).unwrap();
    c.run(W, "Std_Export").unwrap();
    c.listed(vec![
        "dir/".into(),
        "a.stl".into(),
        "b.obj".into(),
        "c.svg".into(),
        "d.step".into(),
    ]);
    assert_eq!(file_dialog(&c).visible(), vec!["dir/", "d.step"]);
    assert_eq!(file_dialog(&c).name, "Body.step");
    c.command(W, "choice:filetype:2", None).unwrap();
    assert_eq!(file_dialog(&c).visible(), vec!["dir/", "a.stl"]);
    // A name typed but not committed takes the new extension.
    c.type_text("Plate").unwrap();
    c.command(W, "choice:filetype:4", None).unwrap();
    let d = file_dialog(&c);
    assert_eq!(d.visible(), vec!["dir/", "b.obj"]);
    assert_eq!(d.name, "Plate.obj");
    assert_eq!(c.field.as_ref().unwrap().text, "Plate.obj");
    c.command(W, "choice:filetype:6", None).unwrap();
    assert_eq!(file_dialog(&c).visible(), vec!["dir/", "c.svg"]);
    assert!(c.file_command(W, "type:9").is_err());
}

#[test]
fn saving_over_a_file_asks_first_in_each_platforms_words() {
    for (platform, enter_replaces) in [
        (DesktopTheme::Macos, false),
        (DesktopTheme::Windows, false),
        (DesktopTheme::Ubuntu, true),
    ] {
        let mut c = cad();
        c.platform = Some(platform);
        c.run(W, "Std_SaveAs").unwrap();
        c.listed(vec!["Unnamed.FCStd.json".into()]);
        // Save over the existing file: the question comes up and takes the keyboard.
        let e = c.key(W, "Enter").unwrap();
        assert!(e.is_empty());
        assert_eq!(
            file_dialog(&c).confirm.as_deref(),
            Some("Unnamed.FCStd.json")
        );
        assert!(c.field.is_none());
        assert!(c.file_command(W, "ok").is_err(), "the question is modal");
        assert!(c.file_command(W, "place:/").is_err());
        assert!(
            c.type_text("0").is_err(),
            "no view shortcut behind a dialog"
        );
        let e = c.key(W, "Enter").unwrap();
        if enter_replaces {
            assert!(matches!(&e[0], AppEffect::WriteFile { path, .. }
                if path == "/home/carol/Documents/Unnamed.FCStd.json"));
            assert!(c.dialog.is_none());
            continue;
        }
        // Return is Cancel / No: back in the dialog, the name box has the keyboard.
        assert!(e.is_empty(), "{platform:?}");
        assert!(file_dialog(&c).confirm.is_none());
        assert!(c.field.is_some());
        c.key(W, "Enter").unwrap();
        c.key(W, "Escape").unwrap();
        assert!(file_dialog(&c).confirm.is_none(), "Escape answers No");
        assert!(c.dialog.is_some());
        c.key(W, "Enter").unwrap();
        let e = c.file_command(W, "replace").unwrap();
        assert!(matches!(&e[0], AppEffect::WriteFile { .. }));
        assert!(c.dialog.is_none());
    }
    // Windows ignores case; another name saves at once.
    let mut c = cad();
    c.platform = Some(DesktopTheme::Windows);
    c.run(W, "Std_SaveAs").unwrap();
    c.listed(vec!["UNNAMED.fcstd.json".into()]);
    c.key(W, "Enter").unwrap();
    assert!(file_dialog(&c).confirm.is_some());
    c.file_command(W, "keep").unwrap();
    c.type_text("Other").unwrap();
    let e = c.key(W, "Enter").unwrap();
    assert!(
        matches!(&e[0], AppEffect::WriteFile { path, .. } if path.ends_with("/Other.FCStd.json"))
    );
}

/// A folder of `n` documents, `part-000.FCStd.json` onwards.
fn many(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("part-{i:03}.FCStd.json")).collect()
}
/// The rows the dialog paints, read off the scene, in order.
fn painted_rows(c: &Cad, theme: DesktopTheme) -> Vec<String> {
    let app = NativeApp::Freecad(Freecad(Box::new(c.clone())));
    let settings = crate::SystemSettings::DEFAULT;
    let env = crate::AppEnv {
        theme,
        width: 1100,
        height: 700,
        clock_us: 0,
        settings: &settings,
        clipboard: None,
        share_to: None,
        files: Default::default(),
        editor: None,
        pointer: None,
    };
    let scene = app_content_with(&AppState::Native(app), &env);
    scene
        .nodes
        .iter()
        .filter_map(|n| {
            n.interaction
                .as_deref()?
                .strip_prefix("freecad:file:entry:")
        })
        .map(str::to_owned)
        .collect()
}
fn painted_target(c: &Cad, theme: DesktopTheme, prefix: &str) -> Option<String> {
    let app = NativeApp::Freecad(Freecad(Box::new(c.clone())));
    let settings = crate::SystemSettings::DEFAULT;
    let env = crate::AppEnv {
        theme,
        width: 1100,
        height: 700,
        clock_us: 0,
        settings: &settings,
        clipboard: None,
        share_to: None,
        files: Default::default(),
        editor: None,
        pointer: None,
    };
    let scene = app_content_with(&AppState::Native(app), &env);
    scene
        .nodes
        .iter()
        .filter_map(|n| n.interaction.clone())
        .find(|t| t.starts_with(prefix))
}

#[test]
fn a_long_folder_scrolls_by_wheel_bar_and_keyboard() {
    for theme in [
        DesktopTheme::Macos,
        DesktopTheme::Windows,
        DesktopTheme::Ubuntu,
    ] {
        let mut c = cad();
        c.platform = Some(theme);
        c.run(W, "Std_Open").unwrap();
        c.listed(many(80));
        let first = painted_rows(&c, theme);
        assert_eq!(first[0], "part-000.FCStd.json", "{theme:?}");
        assert!(first.len() < 80, "{theme:?}: the list overflows");
        let cap = first.len();
        // The painted list says how many rows it shows.
        let list = painted_target(&c, theme, "freecad:file:list:").unwrap();
        assert_eq!(list, format!("freecad:file:list:{cap}"));
        c.command(W, list.strip_prefix("freecad:").unwrap(), None)
            .unwrap();
        assert_eq!(file_dialog(&c).page, cap);
        // The wheel: a notch is three rows.
        let mut app = Freecad(Box::new(c.clone()));
        assert!(app
            .wheel("freecad:file:entry:part-001.FCStd.json", 0, 0, 120)
            .unwrap());
        c = *app.0;
        assert_eq!(painted_rows(&c, theme)[0], "part-003.FCStd.json");
        // The scrollbar: a click at the bottom of the track shows the last rows.
        let bar = painted_target(&c, theme, "freecad:file:scrollbar:").unwrap();
        let height: i32 = bar.rsplit(':').next().unwrap().parse().unwrap();
        c.command(W, bar.strip_prefix("freecad:").unwrap(), Some((4, height)))
            .unwrap();
        let rows = painted_rows(&c, theme);
        assert_eq!(rows.last().unwrap(), "part-079.FCStd.json", "{theme:?}");
        // Windows' arrows step a row.
        if theme == DesktopTheme::Windows {
            c.command(W, "file:scroll-by:-1", None).unwrap();
            assert_eq!(
                painted_rows(&c, theme).last().unwrap(),
                "part-078.FCStd.json"
            );
        }
        // The keyboard: Home, Page Down, End keep the selection in view.
        c.key(W, "Home").unwrap();
        assert_eq!(painted_rows(&c, theme)[0], "part-000.FCStd.json");
        c.key(W, "PageDown").unwrap();
        let sel = file_dialog(&c).selected.clone().unwrap();
        assert_eq!(sel, format!("part-{:03}.FCStd.json", cap - 1));
        assert!(painted_rows(&c, theme).contains(&sel));
        c.key(W, "ArrowDown").unwrap();
        let sel = file_dialog(&c).selected.clone().unwrap();
        assert!(painted_rows(&c, theme).contains(&sel), "{theme:?}: {sel}");
        c.key(W, "End").unwrap();
        let rows = painted_rows(&c, theme);
        assert_eq!(rows.last().unwrap(), "part-079.FCStd.json");
        c.key(W, "ArrowUp").unwrap();
        assert_eq!(
            file_dialog(&c).selected.as_deref(),
            Some("part-078.FCStd.json")
        );
        // The last row, reached by scrolling, opens.
        c.key(W, "End").unwrap();
        let e = c.key(W, "Enter").unwrap();
        assert!(
            matches!(&e[0], AppEffect::ReadBytes { path, .. }
                if path == "/home/carol/Documents/part-079.FCStd.json"),
            "{theme:?}: {e:?}"
        );
    }
}

#[test]
fn a_short_window_scrolls_the_sidebar() {
    for theme in [
        DesktopTheme::Macos,
        DesktopTheme::Windows,
        DesktopTheme::Ubuntu,
    ] {
        let mut c = cad();
        c.platform = Some(theme);
        c.run(W, "Std_SaveAs").unwrap();
        c.listed(vec![]);
        let settings = crate::SystemSettings::DEFAULT;
        let places = |c: &Cad| -> Vec<String> {
            let files = crate::FilesEnv {
                home: "/home/carol",
                folders: crate::standard_folders(theme)
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
                trash: "/home/carol/.local/share/Trash/files".into(),
                starred: &[],
            };
            let env = crate::AppEnv {
                theme,
                width: 900,
                height: 330,
                clock_us: 0,
                settings: &settings,
                clipboard: None,
                share_to: None,
                files,
                editor: None,
                pointer: None,
            };
            let app = NativeApp::Freecad(Freecad(Box::new(c.clone())));
            app_content_with(&AppState::Native(app), &env)
                .nodes
                .iter()
                .filter_map(|n| n.interaction.clone())
                .filter(|t| {
                    t.starts_with("freecad:file:place:")
                        || t == "freecad:file:home"
                        || t.starts_with("freecad:file:side-scrollbar:")
                })
                .collect()
        };
        let before = places(&c);
        assert!(
            before.iter().any(|t| t.contains("side-scrollbar")),
            "{theme:?}: {before:?}"
        );
        let mut app = Freecad(Box::new(c));
        assert!(app.wheel(&before[0], 0, 0, 120).unwrap());
        let c = *app.0;
        assert!(file_dialog(&c).side_scroll > 0);
        let after = places(&c);
        assert_ne!(before[0], after[0], "{theme:?}: the sidebar moved");
    }
}

#[test]
fn open_dialog_selects_enters_and_opens() {
    let mut c = cad();
    c.run(W, "Std_Open").unwrap();
    c.listed(vec!["Parts/".into(), "x.FCStd.json".into(), "y.stl".into()]);
    assert_eq!(file_dialog(&c).visible(), vec!["Parts/", "x.FCStd.json"]);
    // A click selects; Open with a folder selected goes into it.
    c.file_command(W, "entry:Parts/").unwrap();
    assert_eq!(file_dialog(&c).folder, "/home/carol/Documents");
    let e = c.file_command(W, "ok").unwrap();
    assert_eq!(listing_of(&e), Some("/home/carol/Documents/Parts"));
    c.listed(vec!["b.FCStd.json".into()]);
    // A double click on a file opens it.
    let e = c.double_click(W, "file:entry:b.FCStd.json").unwrap();
    assert!(matches!(&e[0], AppEffect::ReadBytes { path, .. }
        if path == "/home/carol/Documents/Parts/b.FCStd.json"));
    assert!(c.dialog.is_none());
    // Escape cancels an open dialog.
    c.run(W, "Std_Open").unwrap();
    c.key(W, "Escape").unwrap();
    assert!(c.dialog.is_none());
}

#[test]
fn new_with_unsaved_changes_asks_first() {
    let mut c = padded();
    assert!(c.modified);
    c.run(W, "Std_New").unwrap();
    assert!(matches!(c.dialog, Some(Dialog::Unsaved { .. })));
    c.dialog_command(W, "cancel").unwrap();
    assert!(c.doc.get("Pad").is_some());
    c.run(W, "Std_New").unwrap();
    c.dialog_command(W, "discard").unwrap();
    assert!(c.doc.get("Pad").is_none());
}

fn states() -> Vec<(&'static str, Cad)> {
    let mut out = vec![("fresh", cad())];
    let mut c = cad();
    c.run(W, "PartDesign_NewSketch").unwrap();
    out.push(("pick plane", c.clone()));
    c.task_command(W, "ok").unwrap();
    let ids = tools::rectangle(sketch_of(&mut c), v2(0.0, 0.0), v2(30.0, 20.0), false).unwrap();
    c.refresh_sketch_report();
    c.sketch_edit_mut().unwrap().picked = vec![(ids[0], Pos::None)];
    c.run(W, "Sketcher_ConstrainDistanceX").unwrap();
    out.push(("dimension dialog", c.clone()));
    c.key(W, "Enter").unwrap();
    c.run(W, "Sketcher_CreateFillet").unwrap();
    out.push(("sketching", c.clone()));
    c.key(W, "Escape").unwrap();
    c.leave_sketch();
    c.run(W, "PartDesign_Pad").unwrap();
    out.push(("pad task", c.clone()));
    c.task_command(W, "ok").unwrap();
    c.command(W, "tree:Pad", None).unwrap();
    c.show_report = true;
    c.log(ReportKind::Warning, "a warning");
    out.push(("pad selected", c.clone()));
    for menu in [
        "File",
        "Edit",
        "View",
        "Tools",
        "Part Design",
        "Sketch",
        "Help",
        "workbench",
        "nav",
        "navcube",
        "overflow",
        "dd:prop/Type",
    ] {
        let mut m = c.clone();
        m.menu = Some(menu.into());
        out.push(("menu", m));
    }
    let mut v = c.clone();
    v.props = PropTab::View;
    out.push(("view props", v));
    let mut m = c.clone();
    m.run(W, "Std_Measure").unwrap();
    out.push(("measure", m));
    let mut f = c.clone();
    f.run(W, "Std_Export").unwrap();
    f.listed(vec!["a/".into(), "b.stl".into()]);
    out.push(("file dialog", f.clone()));
    f.menu = Some("dd:filetype".into());
    out.push(("file types", f.clone()));
    f.menu = Some("dd:filepath".into());
    out.push(("folder pop-up", f.clone()));
    f.menu = None;
    f.file_command(W, "entry:a/").unwrap();
    out.push(("folder selected", f.clone()));
    let mut s = c.clone();
    s.run(W, "Std_SaveAs").unwrap();
    s.listed(vec!["Parts/".into(), "Unnamed.FCStd.json".into()]);
    s.file_command(W, "place:/home/carol/Desktop").unwrap();
    s.listed(vec![]);
    s.file_command(W, "back").unwrap();
    s.listed(vec!["Parts/".into(), "Unnamed.FCStd.json".into()]);
    out.push(("save as with history", s.clone()));
    let mut n = s.clone();
    n.file_command(W, "folder-prompt:untitled folder").unwrap();
    out.push(("new folder prompt", n.clone()));
    n.type_text("Parts").unwrap();
    n.file_command(W, "folder-create").unwrap_err();
    out.push(("new folder refused", n));
    let mut k = s.clone();
    k.key(W, "Enter").unwrap();
    out.push(("replace question", k));
    let mut m = s.clone();
    m.file_command(W, "collapse").unwrap();
    out.push(("collapsed save panel", m));
    let mut h = s.clone();
    h.file_command(W, "home").unwrap();
    h.listed(vec!["Desktop/".into(), "Documents/".into()]);
    out.push(("explorer home", h));
    let mut o = c.clone();
    o.modified = false;
    o.run(W, "Std_Open").unwrap();
    o.listed(vec!["Parts/".into(), "x.FCStd.json".into()]);
    o.file_command(W, "entry:x.FCStd.json").unwrap();
    out.push(("open with a file chosen", o));
    let mut l = c.clone();
    l.modified = false;
    l.run(W, "Std_Open").unwrap();
    l.listed(many(60));
    out.push(("long folder", l.clone()));
    l.command(W, "file:scroll-by:20", None).unwrap();
    out.push(("long folder scrolled", l));
    let mut i = c.clone();
    i.run(W, "Std_Import").unwrap();
    i.listed(vec!["m.stl".into(), "d.dxf".into()]);
    out.push(("import", i));
    let mut d = c.clone();
    d.dialog = Some(Dialog::About);
    out.push(("about", d));
    let mut u = c.clone();
    u.dialog = Some(Dialog::Unsaved { then: "new".into() });
    out.push(("unsaved", u));
    let mut e = c.clone();
    let body = body_shape(&e);
    let edge = (0..body.topo.edges.len())
        .find(|i| body.topo.edges[*i].kind == cw_cad::solid::EdgeKind::Line)
        .unwrap();
    e.selection = vec![Sel {
        object: "Pad".into(),
        sub: format!("Edge{}", edge + 1),
        point: V3::ZERO,
    }];
    e.run(W, "PartDesign_Fillet").unwrap();
    out.push(("fillet task", e));
    out
}

/// Every control painted in every state, on every desktop, does something when used:
/// none of them is a picture of a button.
#[test]
fn every_painted_control_is_one_the_application_accepts() {
    for theme in [
        DesktopTheme::Ubuntu,
        DesktopTheme::Windows,
        DesktopTheme::Macos,
    ] {
        for (label, state) in states() {
            let mut state = state.clone();
            state.platform = Some(theme);
            let app = NativeApp::Freecad(Freecad(Box::new(state)));
            let settings = crate::SystemSettings::DEFAULT;
            // The machine's standard places, so the dialogs' sidebars are populated.
            let files = crate::FilesEnv {
                home: "/home/carol",
                folders: crate::standard_folders(theme)
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
                trash: "/home/carol/.local/share/Trash/files".into(),
                starred: &[],
            };
            let env = crate::AppEnv {
                theme,
                width: 1100,
                height: 700,
                clock_us: 0,
                settings: &settings,
                clipboard: None,
                share_to: None,
                files,
                editor: None,
                pointer: Some((600, 300)),
            };
            let scene = app_content_with(&AppState::Native(app.clone()), &env);
            // Controls a pointer can reach: at its centre, the control itself is on top.
            let targets: Vec<(String, cw_scene::Rect)> = scene
                .nodes
                .iter()
                .filter_map(|n| n.interaction.clone().map(|t| (t, n.bounds, n.id)))
                .filter(|(_, b, id)| {
                    let (x, y) = (b.x + b.width as i32 / 2, b.y + b.height as i32 / 2);
                    scene.hit_test(x, y).map(|h| h.id) == Some(*id)
                })
                .map(|(t, b, _)| (t, b))
                .collect();
            assert!(!targets.is_empty(), "{label}: no controls");
            for (target, bounds) in targets {
                let mut a = app.clone();
                let r = if a.drags(&target) {
                    let (x, y) = (bounds.width as i32 / 2, bounds.height as i32 / 2);
                    a.pointer(W, &target, PointerPhase::Down, x, y, 0)
                        .and_then(|_| a.pointer(W, &target, PointerPhase::Up, x, y, 0))
                } else if target == "freecad:navcube" {
                    a.click_at(W, &target, 46, 46, 0)
                } else {
                    a.click(W, &target, 0)
                };
                assert!(r.is_ok(), "{theme:?} {label}: {target} -> {r:?}");
            }
        }
    }
}

#[test]
fn snapshots_round_trip_and_redraw_identically() {
    let mut c = padded();
    c.set_view(StdView::Isometric);
    c.selection = vec![Sel {
        object: "Pad".into(),
        sub: "Face1".into(),
        point: V3::ZERO,
    }];
    let app = NativeApp::Freecad(Freecad(Box::new(c)));
    let text = serde_json::to_string(&app).unwrap();
    let back: NativeApp = serde_json::from_str(&text).unwrap();
    assert_eq!(app, back);
    let settings = crate::SystemSettings::DEFAULT;
    let env = crate::AppEnv {
        theme: DesktopTheme::Ubuntu,
        width: 900,
        height: 600,
        clock_us: 0,
        settings: &settings,
        clipboard: None,
        share_to: None,
        files: Default::default(),
        editor: None,
        pointer: None,
    };
    let a = app_content_with(&AppState::Native(app), &env);
    let b = app_content_with(&AppState::Native(back), &env);
    assert_eq!(
        a, b,
        "a restored snapshot recomputes and draws the same model"
    );
    assert!(a
        .nodes
        .iter()
        .any(|n| matches!(n.primitive, cw_scene::Primitive::Image { .. })));
}

#[test]
fn keyboard_view_shortcuts_and_arrows() {
    let mut c = padded();
    c.type_text("1").unwrap();
    assert!((c.camera.back() - v3(0.0, -1.0, 0.0)).len() < 1e-12);
    c.type_text("2").unwrap();
    assert!((c.camera.back() - V3::Z).len() < 1e-12);
    let t = c.camera.target;
    c.key(W, "ArrowLeft").unwrap();
    assert!((c.camera.target - t).len() > 0.0);
    let h = c.camera.half_height;
    c.key(W, "PageUp").unwrap();
    assert!(c.camera.half_height < h);
    // Space toggles the selected object's visibility.
    c.command(W, "tree:Body", None).unwrap();
    c.type_text(" ").unwrap();
    assert!(!c.doc.get("Body").unwrap().visible);
    // Typing with no field focused and no shortcut is refused, not swallowed.
    assert!(c.type_text("q").is_err());
}
