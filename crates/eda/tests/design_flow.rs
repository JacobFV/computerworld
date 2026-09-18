//! A whole design through the engine: draw an RC low-pass, check it electrically,
//! simulate it, export its netlists and BOM, lay out and route the board, fill a ground
//! zone, pass DRC, write Gerbers and a drill file that parse back, and round-trip every
//! file.
use cw_eda::drc;
use cw_eda::erc;
use cw_eda::files;
use cw_eda::footprints::MM;
use cw_eda::geom::{Pt, Xf};
use cw_eda::gerber;
use cw_eda::netlist;
use cw_eda::pcb::{Board, DrawShape, Drawing, Layer, Track, Zone};
use cw_eda::schematic::{ortho, LabelKind, Schematic};
use cw_eda::spice;
use cw_eda::zones;

fn wire(s: &mut Schematic, a: Pt, b: Pt, vertical_first: bool) {
    for (p, q) in ortho(a, b, vertical_first) {
        s.add_wire(p, q);
    }
}

/// V1 (pulse) → R1 1 kΩ → out, C1 1 µF to ground, J1 bringing in/out/GND to the board.
pub fn rc_schematic() -> Schematic {
    let mut s = Schematic::new("rc-flow");
    let v1 = s
        .place("Simulation_SPICE:VPULSE", Pt::new(2000, 3000), Xf::IDENTITY)
        .unwrap();
    s.symbol_mut(v1)
        .unwrap()
        .set_field("Value", "pulse(0 1 0 1n 1n 1 2)");
    let r1 = s
        .place("Device:R", Pt::new(3000, 2500), Xf::ROT_CCW)
        .unwrap();
    s.symbol_mut(r1).unwrap().set_field("Value", "1k");
    let c1 = s
        .place("Device:C", Pt::new(3500, 3000), Xf::IDENTITY)
        .unwrap();
    s.symbol_mut(c1).unwrap().set_field("Value", "1u");
    s.place("Connector:Conn_01x03", Pt::new(4500, 2400), Xf::IDENTITY)
        .unwrap();
    s.place("power:GND", Pt::new(2000, 3500), Xf::IDENTITY)
        .unwrap();
    s.place("power:GND", Pt::new(3500, 3500), Xf::IDENTITY)
        .unwrap();
    s.place("power:GND", Pt::new(4100, 2600), Xf::IDENTITY)
        .unwrap();
    s.place("power:PWR_FLAG", Pt::new(2000, 3500), Xf::IDENTITY)
        .unwrap();
    wire(&mut s, Pt::new(2000, 2800), Pt::new(2850, 2500), true);
    wire(&mut s, Pt::new(3150, 2500), Pt::new(3500, 2850), false);
    wire(&mut s, Pt::new(4300, 2500), Pt::new(3500, 2500), false);
    wire(&mut s, Pt::new(4300, 2400), Pt::new(4000, 2400), false);
    wire(&mut s, Pt::new(4300, 2600), Pt::new(4100, 2600), false);
    wire(&mut s, Pt::new(3500, 3150), Pt::new(3500, 3500), false);
    wire(&mut s, Pt::new(2000, 3200), Pt::new(2000, 3500), false);
    s.add_label(Pt::new(2000, 2500), "in", LabelKind::Local);
    s.add_label(Pt::new(4000, 2400), "in", LabelKind::Local);
    s.add_label(Pt::new(3500, 2700), "out", LabelKind::Local);
    s.set_sim_command(".tran 10u 5m");
    s.cleanup_junctions();
    s.annotate(true, false);
    s
}

#[test]
fn the_rc_schematic_is_erc_clean_and_has_the_expected_nets() {
    let s = rc_schematic();
    let violations = erc::check(&s);
    assert!(violations.is_empty(), "{violations:#?}");
    let conn = cw_eda::connectivity::analyze(&s);
    let names: Vec<&str> = conn.nets.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, vec!["GND", "in", "out"]);
    let out = conn.net_named("out").unwrap();
    let pins: Vec<String> = out
        .pins
        .iter()
        .map(|p| format!("{}.{}", p.reference, p.number))
        .collect();
    assert_eq!(pins, vec!["C1.1", "J1.2", "R1.2"]);
    // A junction where the J1 wire tees into the R1–C1 corner.
    assert!(s.junctions.iter().any(|j| j.pos == Pt::new(3500, 2500)));
}

#[test]
fn erc_catches_a_missing_power_flag_and_an_unconnected_pin() {
    let mut s = rc_schematic();
    let flag = s
        .symbols
        .iter()
        .find(|x| x.lib_id == "power:PWR_FLAG")
        .unwrap()
        .id;
    s.delete(&[cw_eda::schematic::Item::Symbol(flag)]);
    let v = erc::check(&s);
    assert!(v.iter().any(|v| v.rule == "power_pin_not_driven"), "{v:#?}");
    let w = s
        .wires
        .iter()
        .find(|w| w.a == Pt::new(4300, 2600))
        .unwrap()
        .id;
    s.delete(&[cw_eda::schematic::Item::Wire(w)]);
    let v = erc::check(&s);
    assert!(
        v.iter()
            .any(|v| v.rule == "pin_not_connected" && v.items.iter().any(|i| i.contains("J1"))),
        "{v:#?}"
    );
}

#[test]
fn netlists_and_bom_describe_the_circuit() {
    let s = rc_schematic();
    let text = netlist::kicad_netlist(&s, "/home/u/rc/rc.kicad_sch", "2026-09-17");
    assert!(text.starts_with("(export"));
    let parsed = netlist::parse_kicad_netlist(&text).unwrap();
    let refs: Vec<&str> = parsed.components.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(
        refs,
        vec!["C1", "J1", "R1"],
        "sim-only V1 and power symbols are not parts"
    );
    let gnd = parsed.nets.iter().find(|n| n.0 == "GND").unwrap();
    assert_eq!(
        gnd.1,
        vec![("C1".into(), "2".into()), ("J1".into(), "3".into())]
    );
    let deck = netlist::spice_netlist(&s, "rc", None).unwrap();
    assert!(deck.text.contains("R1 in out 1k"), "{}", deck.text);
    assert!(deck.text.contains("C1 out 0 1u"), "{}", deck.text);
    assert!(deck.text.contains(".tran 10u 5m"), "{}", deck.text);
    // Connectors start excluded from simulation, as board-only parts.
    assert_eq!(deck.skipped, vec!["J1 is excluded from simulation"]);
    let bom = netlist::bom_csv(&s);
    let lines: Vec<&str> = bom.lines().collect();
    assert_eq!(
        lines[0],
        "\"Reference\",\"Value\",\"Datasheet\",\"Footprint\",\"Qty\",\"DNP\""
    );
    assert_eq!(lines.len(), 4, "{bom}");
    assert!(lines.iter().any(|l| l.starts_with("\"R1\",\"1k\"")));
}

#[test]
fn the_exported_spice_deck_charges_with_the_rc_time_constant() {
    let s = rc_schematic();
    let deck = netlist::spice_netlist(&s, "rc", None).unwrap();
    let result = spice::parse(&deck.text).unwrap().run().unwrap();
    let v = result.value_at("V(out)", 1e-3).unwrap();
    assert!((v - (1.0 - (-1.0f64).exp())).abs() < 1e-4, "{v}");
}

#[test]
fn a_symbol_without_a_model_stops_simulation_until_excluded() {
    let mut s = rc_schematic();
    s.place("Timer:NE555P", Pt::new(6000, 3000), Xf::IDENTITY)
        .unwrap();
    s.annotate(true, false);
    let err = netlist::spice_netlist(&s, "rc", None).unwrap_err();
    assert!(
        err.contains("U1") && err.contains("no simulation model"),
        "{err}"
    );
    let u = s
        .symbols
        .iter_mut()
        .find(|x| x.reference() == "U1")
        .unwrap();
    u.exclude_from_sim = true;
    let deck = netlist::spice_netlist(&s, "rc", None).unwrap();
    assert_eq!(
        deck.skipped,
        vec![
            "J1 is excluded from simulation",
            "U1 is excluded from simulation"
        ]
    );
}

/// Update the board from the schematic, then place, outline, route and zone it.
pub fn routed_board(s: &Schematic) -> Board {
    let mut b = Board::new("rc-flow");
    let changes = b.update_from_schematic(s, true);
    assert!(
        changes.iter().any(|c| c.message
            == "Add R1 (Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal)."),
        "{changes:#?}"
    );
    assert!(changes.iter().all(|c| !c.warning), "{changes:#?}");
    assert_eq!(b.footprints.len(), 3);
    let place = |b: &mut Board, r: &str, x: i64, y: i64| {
        let f = b.footprints.iter_mut().find(|f| f.reference == r).unwrap();
        f.pos = Pt::new(x, y);
    };
    place(&mut b, "J1", 10 * MM, 10 * MM);
    place(&mut b, "R1", 15 * MM, 10 * MM);
    place(&mut b, "C1", 15 * MM, 15_080_000);
    let id = b.take_id();
    b.drawings.push(Drawing {
        id,
        layer: Layer::EdgeCuts,
        shape: DrawShape::Rect {
            a: Pt::new(5 * MM, 5 * MM),
            b: Pt::new(30 * MM, 20 * MM),
        },
        width: 50_000,
    });
    let net = |b: &Board, n: &str| b.nets.iter().position(|x| x == n).unwrap();
    let (inn, out, gnd) = (net(&b, "in"), net(&b, "out"), net(&b, "GND"));
    let track = |b: &mut Board, a: Pt, c: Pt, net: usize| {
        let id = b.take_id();
        b.tracks.push(Track {
            id,
            a,
            b: c,
            width: 250_000,
            layer: Layer::FCu,
            net,
        });
    };
    track(
        &mut b,
        Pt::new(10 * MM, 10 * MM),
        Pt::new(15 * MM, 10 * MM),
        inn,
    );
    track(
        &mut b,
        Pt::new(25_160_000, 10 * MM),
        Pt::new(25_160_000, 12_540_000),
        out,
    );
    track(
        &mut b,
        Pt::new(25_160_000, 12_540_000),
        Pt::new(10 * MM, 12_540_000),
        out,
    );
    track(
        &mut b,
        Pt::new(15 * MM, 15_080_000),
        Pt::new(15 * MM, 12_540_000),
        out,
    );
    let id = b.take_id();
    b.zones.push(Zone {
        id,
        net: gnd,
        layer: Layer::BCu,
        outline: vec![
            Pt::new(5 * MM, 5 * MM),
            Pt::new(30 * MM, 5 * MM),
            Pt::new(30 * MM, 20 * MM),
            Pt::new(5 * MM, 20 * MM),
        ],
        clearance: 500_000,
        min_width: 250_000,
        thermal_gap: 500_000,
        spoke_width: 500_000,
        fill: vec![],
        filled: false,
    });
    b
}

#[test]
fn the_routed_board_passes_drc_once_the_ground_zone_is_filled() {
    let s = rc_schematic();
    let mut b = routed_board(&s);
    // Before the fill, ground is still a ratsnest line.
    let rats = drc::ratsnest(&b);
    assert_eq!(rats.len(), 1, "{rats:?}");
    assert_eq!(b.net_name(rats[0].net), "GND");
    zones::fill_all(&mut b);
    assert!(!b.zones[0].fill.is_empty());
    assert!(drc::ratsnest(&b).is_empty(), "{:?}", drc::ratsnest(&b));
    let refs: Vec<(String, String)> = s
        .symbols
        .iter()
        .filter(|x| x.on_board && !x.is_power())
        .map(|x| (x.reference().to_owned(), x.field("Footprint").to_owned()))
        .collect();
    let v = drc::check(&b, Some(&refs));
    assert!(v.is_empty(), "{v:#?}");
    // Every zone rectangle keeps its clearance from other nets' copper.
    for c in b.copper() {
        if c.net == b.zones[0].net || c.layer != Layer::BCu {
            continue;
        }
        for r in &b.zones[0].fill {
            let d = cw_eda::pcb::Shape::Rect(*r).distance(&c.shape);
            assert!(d >= 500_000.0, "zone within {d} nm of {:?}", c.owner);
        }
    }
}

#[test]
fn drc_reports_clearance_width_annular_ring_courtyards_and_unrouted_nets() {
    let s = rc_schematic();
    let mut b = routed_board(&s);
    zones::fill_all(&mut b);
    // A track of another net squeezed past R1's pad, too thin.
    let id = b.take_id();
    let gnd = b.nets.iter().position(|n| n == "GND").unwrap();
    b.tracks.push(Track {
        id,
        a: Pt::new(12 * MM, 11_000_000),
        b: Pt::new(20 * MM, 11_000_000),
        width: 100_000,
        layer: Layer::FCu,
        net: gnd,
    });
    let id = b.take_id();
    b.vias.push(cw_eda::pcb::Via {
        id,
        pos: Pt::new(28 * MM, 18 * MM),
        diameter: 400_000,
        drill: 300_000,
        net: gnd,
    });
    let c1 = b
        .footprints
        .iter()
        .position(|f| f.reference == "C1")
        .unwrap();
    b.footprints[c1].pos = Pt::new(16 * MM, 11 * MM);
    let v = drc::check(&b, None);
    for rule in [
        "clearance",
        "track_width",
        "annular_width",
        "courtyards_overlap",
    ] {
        assert!(
            v.iter()
                .any(|x| x.rule == rule || (rule == "clearance" && x.rule == "shorting_items")),
            "{rule} not reported: {v:#?}"
        );
    }
    b.tracks.clear();
    assert!(!drc::unconnected(&b).is_empty());
    b.drawings.clear();
    assert!(drc::check(&b, None)
        .iter()
        .any(|x| x.rule == "invalid_outline"));
}

#[test]
fn gerbers_and_drill_files_parse_back() {
    let s = rc_schematic();
    let mut b = routed_board(&s);
    zones::fill_all(&mut b);
    for layer in gerber::DEFAULT_LAYERS {
        let text = gerber::layer(&b, "rc", layer);
        let stats =
            gerber::parse(&text).unwrap_or_else(|e| panic!("{}: {e}\n{text}", layer.name()));
        match layer {
            Layer::FCu => {
                assert_eq!(stats.flashes, 7, "seven pads on the front");
                assert_eq!(stats.draws, 4, "four tracks");
                assert_eq!(stats.file_function, "Copper,L1,Top");
            }
            Layer::BCu => {
                assert!(stats.regions > 0, "the zone plots as regions");
                assert_eq!(stats.draws, 0);
            }
            Layer::EdgeCuts => {
                assert_eq!(stats.draws, 4);
                // The outline spans 25 × 15 mm, in Gerber's Y-up frame.
                assert_eq!(stats.max.0 - stats.min.0, 25_000_000);
                assert_eq!(stats.max.1 - stats.min.1, 15_000_000);
            }
            Layer::FSilkS => assert!(stats.draws > 20, "outlines and reference text"),
            _ => {}
        }
    }
    let drill = gerber::drill(&b, "rc");
    let d = gerber::parse_drill(&drill).unwrap();
    assert!(d.metric);
    assert_eq!(d.holes, 7);
    assert_eq!(d.tools.len(), 2, "0.8 mm and 1.0 mm holes");
    for bad in [
        "%FSLAX46Y46*%\nX1Y1D01*\nM02*\n",
        "%FSLAX46Y46*%\n%MOMM*%\nD10*\nM02*\n",
        "%FSLAX46Y46*%\n%MOMM*%\nG36*\nX0Y0D02*\nX1Y0D01*\nG37*\nM02*\n",
    ] {
        assert!(gerber::parse(bad).is_err(), "{bad}");
    }
    let svg = cw_eda::svg::plot(
        &b,
        &[Layer::BCu, Layer::FCu, Layer::FSilkS, Layer::EdgeCuts],
        "rc",
    );
    assert!(svg.contains("<svg") && svg.contains("id=\"F.Cu\""));
}

#[test]
fn project_files_round_trip() {
    let s = rc_schematic();
    let text = files::write_schematic(&s, "rc");
    assert!(text.starts_with("(kicad_sch\n\t(version 20231120)"));
    let (back, warnings) = files::read_schematic(&text).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(back, s);
    assert_eq!(files::write_schematic(&back, "rc"), text);
    let mut b = routed_board(&s);
    zones::fill_all(&mut b);
    b.footprints[0].rot = 1;
    b.footprints[1].back = true;
    let text = files::write_board(&b);
    assert!(text.starts_with("(kicad_pcb\n\t(version 20240108)"));
    let (back, warnings) = files::read_board(&text).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back, b);
    let pro = files::write_project("rc", &b.rules);
    assert_eq!(files::read_project(&pro).unwrap(), b.rules);
    assert!(files::read_schematic("(kicad_pcb)").is_err());
    assert!(files::read_board("(kicad_pcb (footprint").is_err());
}
