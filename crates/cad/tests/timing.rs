//! Temporary timing harness (ignored): where the kernel spends its time.
use cw_cad::brep::blend::{dress, Dress};
use cw_cad::brep::boolean::{boolean, Op};
use cw_cad::brep::build::{cuboid, cylinder};
use cw_cad::brep::mass::mass_props;
use cw_cad::math::{v3, V3};
use cw_cad::step::{self, Schema};
use std::time::Instant;

fn stamp(label: &str, t: &mut Instant) {
    println!("{label}: {:?}", t.elapsed());
    *t = Instant::now();
}

#[test]
#[ignore]
fn timings() {
    let mut t = Instant::now();
    let main = cylinder(v3(-10.0, 0.0, 0.0), V3::X, 3.0, 20.0);
    let branch = cylinder(v3(0.0, 0.0, 0.0), V3::Z, 1.5, 10.0);
    let tee = boolean(&main, &branch, Op::Union).unwrap();
    stamp("union", &mut t);
    let e = (0..tee.edges.len())
        .find(|e| matches!(tee.edges[*e].curve, cw_cad::brep::Curve::Traced(_)))
        .unwrap();
    let f = dress(&tee, &[e], Dress::Fillet(0.5), "Fillet").unwrap();
    stamp("fillet", &mut t);
    let m = mass_props(&f);
    stamp("mass of the blended solid", &mut t);
    println!("volume {}", m.volume);
    let text = step::write(&f, "Tee", Schema::Ap242, "tee.step", "2026-09-18T10:00:00");
    stamp("step write", &mut t);
    println!("step size {} bytes", text.len());
    let back = step::read(&text).unwrap();
    stamp("step read", &mut t);
    let m2 = mass_props(&back[0].1);
    stamp("mass of the read solid", &mut t);
    println!("volume {} vs {}", m2.volume, m.volume);
    let pa = cw_cad::brep::mass::face_props(&f);
    let pb = cw_cad::brep::mass::face_props(&back[0].1);
    for (i, (x, y)) in pa.iter().zip(&pb).enumerate() {
        println!(
            "face {i} {} area {:.6} vs {} {:.6} centroid {:?} vs {:?}",
            f.faces[i].surface.name(),
            x.area,
            back[0].1.faces[i].surface.name(),
            y.area,
            x.centroid,
            y.centroid
        );
    }
    let (mesh, _) = cw_cad::brep::tess::tessellate(&f);
    stamp("tessellate", &mut t);
    println!("{} triangles", mesh.tris.len());
    // A plain part, for comparison.
    let plate = cuboid(V3::ZERO, v3(20.0, 12.0, 4.0));
    let hole = cylinder(v3(6.0, 6.0, -1.0), V3::Z, 2.5, 6.0);
    let part = boolean(&plate, &hole, Op::Difference).unwrap();
    stamp("plate with a hole", &mut t);
    let _ = mass_props(&part);
    stamp("its mass", &mut t);
}
