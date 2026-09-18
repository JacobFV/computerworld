use super::*;
use crate::desktop_scene::app_content_with;
use crate::AppState;

fn env(theme: DesktopTheme) -> crate::AppEnv<'static> {
    crate::AppEnv {
        theme,
        width: 1400,
        height: 900,
        clock_us: 0,
        settings: &crate::SystemSettings::DEFAULT,
        clipboard: None,
        share_to: None,
        files: Default::default(),
        editor: None,
        pointer: None,
    }
}
fn targets(app: &Kicad, theme: DesktopTheme) -> Vec<String> {
    let scene = app_content_with(
        &AppState::Native(crate::NativeApp::Kicad(app.clone())),
        &env(theme),
    );
    scene
        .nodes
        .iter()
        .filter_map(|n| n.interaction.clone())
        .collect()
}

#[test]
fn every_frame_paints_on_every_desktop_and_its_controls_dispatch() {
    for frame in [
        "",
        "sch|/home/u/Documents/KiCad/p/p.kicad_pro",
        "pcb|/home/u/Documents/KiCad/p/p.kicad_pro",
        "sim|/home/u/Documents/KiCad/p/p.kicad_pro",
        "symed|/home/u/Documents/KiCad/p/p.kicad_pro",
        "fped|/home/u/Documents/KiCad/p/p.kicad_pro",
        "3d|/home/u/Documents/KiCad/p/p.kicad_pro",
    ] {
        let (mut app, _) = Kicad::launch(frame, 1, 0);
        app.files_read(1, "open", vec![]);
        // The library editors with something open, so their editing controls paint.
        app.session.sym_libs.push(SymLib {
            name: "mine".into(),
            file: "mine.kicad_sym".into(),
            symbols: vec![LibSymbol::blank("mine:AMP", "U")],
            dirty: false,
        });
        app.session.fp_libs.push(FpLib {
            name: "mine".into(),
            dir: "mine.pretty".into(),
            footprints: vec![LibFootprint::blank("mine:SOT")],
            dirty: false,
        });
        if app.frame == Frame::SymbolEditor {
            app.click(1, "kicad:symed:open:mine:AMP", 0).unwrap();
        }
        if app.frame == Frame::FootprintEditor {
            app.click(1, "kicad:fped:open:mine:SOT", 0).unwrap();
        }
        for theme in [
            DesktopTheme::Macos,
            DesktopTheme::Windows,
            DesktopTheme::Ubuntu,
        ] {
            let t = targets(&app, theme);
            assert!(!t.is_empty(), "{frame} paints no controls");
            for target in t {
                assert!(
                    target.starts_with("kicad:"),
                    "{target} is not a KiCad target"
                );
                // Every painted control is a command the app knows (it may refuse for
                // want of a selection, but it must not be unknown).
                let mut probe = app.clone();
                if let Err(e) = probe.click(1, &target, 0) {
                    assert!(!e.contains("unknown"), "{target}: {e}");
                }
            }
        }
    }
}

/// A board with an outline, a through-hole and an SMD footprint (one turned 45°), a
/// track and a via.
fn sample_board() -> Board {
    use cw_eda::footprints::MM;
    use cw_eda::pcb::{DrawShape, Drawing, Footprint, Track, Via};
    let mut b = Board::new("p.kicad_pro");
    let id = b.take_id();
    b.drawings.push(Drawing {
        id,
        layer: Layer::EdgeCuts,
        shape: DrawShape::Rect {
            a: Pt::new(0, 0),
            b: Pt::new(30 * MM, 20 * MM),
        },
        width: 100_000,
    });
    for (fp, at, angle) in [
        (
            "Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal",
            (5, 5),
            0,
        ),
        ("Resistor_SMD:R_0805_2012Metric", (20, 12), 450),
    ] {
        let def = cw_eda::footprints::find(fp).expect("library footprint");
        let id = b.take_id();
        let mut f = Footprint::from_def(id, def, "U1", "x", Pt::new(at.0 * MM, at.1 * MM));
        f.angle = angle;
        b.footprints.push(f);
    }
    let id = b.take_id();
    b.tracks.push(Track {
        id,
        a: Pt::new(3 * MM, 15 * MM),
        b: Pt::new(12 * MM, 15 * MM),
        width: 250_000,
        layer: Layer::FCu,
        net: 0,
    });
    let id = b.take_id();
    b.vias.push(Via {
        id,
        pos: Pt::new(12 * MM, 15 * MM),
        diameter: 600_000,
        drill: 300_000,
        net: 0,
    });
    b
}

#[test]
fn the_3d_view_is_deterministic_and_shows_the_board() {
    let b = sample_board();
    let s = view3d::scene(&b);
    // Two THT pads and the via are drilled through the substrate.
    let tht = b
        .footprints
        .iter()
        .flat_map(|f| f.pads.iter())
        .filter(|p| p.drill > 0)
        .count();
    assert_eq!(s.holes, tht + 1);
    assert!(s.outlined);
    assert!(!s.bodies.tris.is_empty() && !s.copper.tris.is_empty());
    // The substrate is a closed solid 1.6 mm thick.
    let bb = s.board.bounds().unwrap();
    assert!((bb.max.z - bb.min.z - view3d::BOARD_THICKNESS).abs() < 1e-9);
    assert!(s.board.is_watertight());
    let mut v = view3d::View3dUi::default();
    let a = view3d::raster(&b, &v, 320, 200);
    let again = view3d::raster(&b, &v, 320, 200);
    assert_eq!(a, again, "the same view rendered different pixels");
    // Looking down: the mask green covers the middle of the board.
    let px = |img: &[u8], x: usize, y: usize| {
        let i = (y * 320 + x) * 4;
        (img[i], img[i + 1], img[i + 2])
    };
    let (r, g, bl) = px(&a, 160, 100);
    assert!(
        g > r && g > bl,
        "centre pixel {:?} is not solder mask",
        (r, g, bl)
    );
    // Orbiting changes the picture; hiding the bodies changes it again.
    v.pitch = 350;
    v.yaw = 450;
    let turned = view3d::raster(&b, &v, 320, 200);
    assert_ne!(a, turned);
    v.hidden.push("bodies".into());
    assert_ne!(turned, view3d::raster(&b, &v, 320, 200));
}
