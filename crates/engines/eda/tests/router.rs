//! The interactive router: walkaround finds clear 45° paths round obstacles of other nets
//! and rule areas; what it builds passes DRC.
use cw_eda::drc;
use cw_eda::footprints::MM;
use cw_eda::geom::Pt;
use cw_eda::pcb::{Board, DrawShape, Drawing, Footprint, Layer, Track, Via, Zone};
use cw_eda::router::{via_collision, Router};

/// Two resistors to join on net A, a wall of net B between them with gaps at both ends,
/// a via of net B, and a keepout that closes the lower gap.
fn board() -> (Board, usize, Pt, Pt) {
    let mut b = Board::new("route");
    let id = b.take_id();
    b.drawings.push(Drawing {
        id,
        layer: Layer::EdgeCuts,
        shape: DrawShape::Rect {
            a: Pt::new(0, 0),
            b: Pt::new(40 * MM, 30 * MM),
        },
        width: 50_000,
    });
    let a = b.net_index("A");
    let other = b.net_index("B");
    let r = "Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal";
    for (i, x) in [(0, 3 * MM), (1, 25 * MM)] {
        let id = b.take_id();
        let mut f =
            Footprint::from_library(id, r, &format!("R{}", i + 1), "10k", Pt::new(x, 15 * MM))
                .unwrap();
        f.pads[if i == 0 { 1 } else { 0 }].net = a;
        b.footprints.push(f);
    }
    let id = b.take_id();
    b.tracks.push(Track {
        id,
        a: Pt::new(20 * MM, 6 * MM),
        b: Pt::new(20 * MM, 24 * MM),
        width: 250_000,
        layer: Layer::FCu,
        net: other,
    });
    let id = b.take_id();
    b.vias.push(Via {
        id,
        pos: Pt::new(20 * MM, 4 * MM),
        diameter: 600_000,
        drill: 300_000,
        net: other,
    });
    let id = b.take_id();
    b.zones.push(Zone {
        id,
        net: 0,
        layer: Layer::FCu,
        outline: vec![
            Pt::new(17 * MM, 24_400_000),
            Pt::new(23 * MM, 24_400_000),
            Pt::new(23 * MM, 30 * MM),
            Pt::new(17 * MM, 30 * MM),
        ],
        clearance: 0,
        min_width: 250_000,
        thermal_gap: 500_000,
        spoke_width: 500_000,
        fill: vec![],
        filled: false,
        keepout: true,
    });
    let start = b.footprints[0].pad_pos(&b.footprints[0].pads[1]);
    let end = b.footprints[1].pad_pos(&b.footprints[1].pads[0]);
    (b, a, start, end)
}

fn octilinear(pts: &[Pt]) -> bool {
    pts.windows(2).all(|w| {
        let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
        dx == 0 || dy == 0 || dx == dy
    })
}

#[test]
fn walkaround_goes_round_other_nets_and_keepouts_and_passes_drc() {
    let (mut b, a, start, end) = board();
    let router = Router::new(&b, a, Layer::FCu, 250_000);
    // Straight across would run into the wall: highlight-collisions mode says so.
    let direct = cw_eda::pcb::posture45(start, end, false);
    let hits = router.collisions(&direct);
    assert!(hits.iter().any(|c| c.item == "Track [B]"), "{hits:?}");
    // Walkaround goes round.
    let path = router.walkaround(start, end, false).expect("a path");
    assert_eq!(path.first(), Some(&start));
    assert_eq!(path.last(), Some(&end));
    assert!(octilinear(&path), "{path:?}");
    assert!(router.collisions(&path).is_empty(), "{path:?}");
    // It passes over the wall's upper end, between it and the via (0.9 mm of room);
    // the keepout closes the way below.
    assert!(path.iter().any(|p| p.y < 6 * MM), "{path:?}");
    assert!(path.iter().all(|p| p.y > 4 * MM), "{path:?}");
    for w in path.windows(2) {
        let id = b.take_id();
        b.tracks.push(Track {
            id,
            a: w[0],
            b: w[1],
            width: 250_000,
            layer: Layer::FCu,
            net: a,
        });
    }
    let v = drc::check(&b, None);
    let bad: Vec<_> = v
        .iter()
        .filter(|x| {
            matches!(
                x.rule.as_str(),
                "clearance" | "shorting_items" | "items_not_allowed" | "copper_edge_clearance"
            )
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
    assert!(
        drc::ratsnest(&b).iter().all(|r| r.net != a),
        "net A is routed"
    );
    // Deterministic.
    let again = Router::new(&board().0, a, Layer::FCu, 250_000).walkaround(start, end, false);
    assert_eq!(again.as_ref(), Some(&path));
}

#[test]
fn a_walled_off_target_has_no_path_and_vias_are_checked() {
    let (mut b, a, start, end) = board();
    // Close the top gap too: now the pads are separated.
    let other = b.net_index("B");
    let id = b.take_id();
    b.tracks.push(Track {
        id,
        a: Pt::new(20 * MM, 6 * MM),
        b: Pt::new(20 * MM, 0),
        width: 250_000,
        layer: Layer::FCu,
        net: other,
    });
    let router = Router::new(&b, a, Layer::FCu, 250_000);
    assert_eq!(router.walkaround(start, end, false), None);
    // On the other layer the way is open.
    let bottom = Router::new(&b, a, Layer::BCu, 250_000);
    let path = bottom.walkaround(start, end, false).unwrap();
    assert!(octilinear(&path));
    // A via next to the wall is refused, one in the open is fine.
    assert_eq!(
        via_collision(&b, a, Pt::new(20 * MM + 300_000, 15 * MM), 600_000).as_deref(),
        Some("Track [B]")
    );
    assert_eq!(
        via_collision(&b, a, Pt::new(10 * MM, 10 * MM), 600_000),
        None
    );
    assert!(via_collision(&b, a, Pt::new(20 * MM, 26 * MM), 600_000).is_some());
}
