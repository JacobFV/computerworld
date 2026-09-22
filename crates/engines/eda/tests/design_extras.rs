//! Hierarchical sheets and buses, project libraries, and footprints at any angle.
use cw_eda::connectivity::analyze;
use cw_eda::erc;
use cw_eda::files;
use cw_eda::footprints::{LibFootprint, LibPad, PadKind, PadShape, MM};
use cw_eda::geom::{Pt, Xf};
use cw_eda::gerber;
use cw_eda::netlist;
use cw_eda::pcb::{Board, DrawShape, Drawing, Footprint, Layer, Shape, Track};
use cw_eda::schematic::{LabelKind, Schematic, SheetPin};
use cw_eda::symbols::{self, Fill, Graphic, LibSymbol, PinType, Spice};

// ---- Hierarchy ---------------------------------------------------------------------------

/// Root: a source into sheet "amp" through its IN pin, and a resistor to ground from
/// its OUT pin. Inside "amp": R from IN to OUT.
fn two_level() -> (Schematic, u64) {
    let mut root = Schematic::new("hier");
    let v = root
        .place("Simulation_SPICE:VDC", Pt::new(1000, 2000), Xf::IDENTITY)
        .unwrap();
    let _ = v;
    root.place("power:GND", Pt::new(1000, 2400), Xf::IDENTITY)
        .unwrap();
    root.add_wire(Pt::new(1000, 2200), Pt::new(1000, 2400));
    let sh = root
        .add_sheet(
            Pt::new(2000, 1500),
            Pt::new(1000, 1000),
            "amp",
            "amp.kicad_sch",
        )
        .unwrap();
    {
        let s = root.sheet_mut(sh).unwrap();
        s.pins.push(SheetPin {
            name: "IN".into(),
            shape: PinType::Input,
            pos: Pt::new(2000, 1800),
        });
        s.pins.push(SheetPin {
            name: "OUT".into(),
            shape: PinType::Output,
            pos: Pt::new(3000, 1800),
        });
    }
    // Source + (1000,1800) to IN.
    root.add_wire(Pt::new(1000, 1800), Pt::new(2000, 1800));
    // OUT to a load resistor to ground.
    root.place("Device:R", Pt::new(3500, 1950), Xf::IDENTITY)
        .unwrap();
    root.add_wire(Pt::new(3000, 1800), Pt::new(3500, 1800));
    root.place("power:GND", Pt::new(3500, 2400), Xf::IDENTITY)
        .unwrap();
    root.add_wire(Pt::new(3500, 2100), Pt::new(3500, 2400));
    let inner = root.at_path_mut(&[sh]).unwrap();
    inner
        .place("Device:R", Pt::new(2000, 2000), Xf::IDENTITY)
        .unwrap();
    inner.add_label(Pt::new(2000, 1850), "IN", LabelKind::Hierarchical);
    inner.add_label(Pt::new(2000, 2150), "OUT", LabelKind::Hierarchical);
    root.annotate(true, false);
    (root, sh)
}

#[test]
fn sheet_pins_meet_hierarchical_labels() {
    let (root, sh) = two_level();
    let c = analyze(&root);
    let inner_r = root.at_path(&[sh]).unwrap().symbols[0]
        .reference()
        .to_owned();
    assert_eq!(inner_r, "R2", "annotation runs on into the sheet");
    // The source's + pin, the sheet's IN pin and the inner resistor's pin 1 are one net,
    // named from inside the sheet, as KiCad names a net only a hierarchical label names.
    let v1 = c.net_of_ref_pin("V1", "1").unwrap();
    assert_eq!(v1.name, "/amp/IN");
    assert!(v1
        .pins
        .iter()
        .any(|p| p.reference == "R2" && p.number == "1"));
    let out = c.net_of_ref_pin("R2", "2").unwrap();
    assert_eq!(out.name, "/amp/OUT");
    assert!(out
        .pins
        .iter()
        .any(|p| p.reference == "R1" && p.number == "1"));
    // SPICE sees the whole design.
    let deck = netlist::spice_netlist(&root, "hier", Some(".op")).unwrap();
    let r = cw_eda::spice::parse(&deck.text).unwrap().run().unwrap();
    // 5 V over R2 (10k) into R1 (10k): the middle sits at 2.5 V.
    let mid = r.value_at("V(/amp/OUT)", 0.0).unwrap();
    assert!((mid - 2.5).abs() < 1e-6, "{mid}\n{}", deck.text);
    // The KiCad netlist lists each component with its sheet.
    let net = netlist::kicad_netlist(&root, "hier.kicad_sch", "d");
    assert!(net.contains("(names \"/amp/\")"), "{net}");
    let parsed = netlist::parse_kicad_netlist(&net).unwrap();
    let names: Vec<&str> = parsed.nets.iter().map(|n| n.0.as_str()).collect();
    assert!(
        names.contains(&"/amp/IN") && names.contains(&"/amp/OUT"),
        "{names:?}"
    );
    // ERC: every pin has a label; mismatches are reported.
    let v = erc::check(&root);
    assert!(!v.iter().any(|x| x.rule == "hier_label_mismatch"), "{v:#?}");
    let mut broken = root.clone();
    broken.sheet_mut(sh).unwrap().pins.push(SheetPin {
        name: "EN".into(),
        shape: PinType::Input,
        pos: Pt::new(2000, 2200),
    });
    let v = erc::check(&broken);
    assert!(
        v.iter()
            .any(|x| x.rule == "hier_label_mismatch" && x.message.contains("EN")),
        "{v:#?}"
    );
}

#[test]
fn hierarchical_designs_save_to_one_file_per_sheet_and_load_back() {
    let (root, sh) = two_level();
    let files_out = files::write_design(&root, "hier", "hier.kicad_sch");
    assert_eq!(files_out.len(), 2);
    assert_eq!(files_out[1].0, "amp.kicad_sch");
    assert!(files_out[0].1.contains("(sheet\n"), "{}", files_out[0].1);
    assert!(files_out[0]
        .1
        .contains("(property \"Sheetfile\" \"amp.kicad_sch\""));
    assert!(files_out[1].1.contains("(hierarchical_label \"IN\""));
    let lookup = |f: &str| {
        files_out
            .iter()
            .find(|(n, _)| n == f)
            .map(|(_, t)| t.clone())
    };
    let (back, warnings) = files::read_design(&files_out[0].1, &lookup).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back.sheets.len(), 1);
    assert_eq!(back.sheets[0].pins.len(), 2);
    assert_eq!(back.sheets[0].contents.symbols.len(), 1);
    // Same nets, same names, same identities.
    let (a, b) = (analyze(&root), analyze(&back));
    let names = |c: &cw_eda::connectivity::Connectivity| {
        c.nets.iter().map(|n| n.name.clone()).collect::<Vec<_>>()
    };
    assert_eq!(names(&a), names(&b));
    assert_eq!(root.sheets[0].uid, back.sheets[0].uid);
    let _ = sh;
}

#[test]
fn buses_carry_members_through_bus_entries_and_sheet_pins() {
    let mut root = Schematic::new("bus");
    // Four resistors whose top pins sit on wires labelled D0..D3, joined to a bus
    // labelled D[0..3] by bus entries.
    for i in 0..4i64 {
        let x = 1000 + 400 * i;
        root.place("Device:R", Pt::new(x, 2000), Xf::IDENTITY)
            .unwrap();
        root.place("power:GND", Pt::new(x, 2300), Xf::IDENTITY)
            .unwrap();
        root.add_wire(Pt::new(x, 2150), Pt::new(x, 2300));
        root.add_wire(Pt::new(x, 1850), Pt::new(x, 1600));
        root.add_bus_entry(Pt::new(x - 100, 1500), Pt::new(100, 100));
        root.add_label(Pt::new(x, 1700), &format!("D{i}"), LabelKind::Local);
    }
    root.add_bus(Pt::new(800, 1500), Pt::new(3500, 1500));
    root.add_label(Pt::new(800, 1500), "D[0..3]", LabelKind::Local);
    // A sheet whose bus pin carries the four members in.
    let sh = root
        .add_sheet(
            Pt::new(3500, 1200),
            Pt::new(1000, 800),
            "io",
            "io.kicad_sch",
        )
        .unwrap();
    root.sheet_mut(sh).unwrap().pins.push(SheetPin {
        name: "DATA[0..3]".into(),
        shape: PinType::Bidirectional,
        pos: Pt::new(3500, 1500),
    });
    {
        let inner = root.at_path_mut(&[sh]).unwrap();
        inner.add_bus(Pt::new(1000, 1000), Pt::new(3000, 1000));
        inner.add_label(Pt::new(1000, 1000), "DATA[0..3]", LabelKind::Hierarchical);
        inner.add_label(Pt::new(2800, 1000), "X[0..3]", LabelKind::Local);
        // A resistor on member X2 inside.
        inner
            .place("Device:R", Pt::new(2000, 1500), Xf::IDENTITY)
            .unwrap();
        inner.add_wire(Pt::new(2000, 1350), Pt::new(2000, 1100));
        inner.add_bus_entry(Pt::new(1900, 1000), Pt::new(100, 100));
        inner.add_label(Pt::new(2000, 1200), "X2", LabelKind::Local);
        inner
            .place("power:GND", Pt::new(2000, 1800), Xf::IDENTITY)
            .unwrap();
        inner.add_wire(Pt::new(2000, 1650), Pt::new(2000, 1800));
    }
    root.annotate(true, false);
    let c = analyze(&root);
    // Member 2 of the root bus (D2) is member 2 inside (X2).
    let d2 = c.net_named("D2").expect("D2 exists");
    let refs: Vec<(&str, &str)> = d2
        .pins
        .iter()
        .map(|p| (p.reference.as_str(), p.number.as_str()))
        .collect();
    assert!(refs.contains(&("R3", "1")), "{refs:?}");
    assert!(refs.contains(&("R5", "1")), "{refs:?}");
    assert!(!c
        .net_named("D1")
        .unwrap()
        .pins
        .iter()
        .any(|p| p.reference == "R5"));
    let bus_members = c.bus_wires.values().next().unwrap();
    assert_eq!(bus_members, &vec!["D0", "D1", "D2", "D3"]);
    let v = erc::check(&root);
    assert!(
        !v.iter()
            .any(|x| x.rule.starts_with("bus") || x.rule == "hier_label_mismatch"),
        "{v:#?}"
    );
    // The netlist has D2 joining both resistors.
    let net = netlist::kicad_netlist(&root, "bus.kicad_sch", "d");
    let parsed = netlist::parse_kicad_netlist(&net).unwrap();
    let d2 = parsed.nets.iter().find(|n| n.0 == "D2").unwrap();
    assert!(d2.1.contains(&("R5".into(), "1".into())), "{d2:?}");
    // A bus label on a plain wire is a bus-to-net conflict.
    root.add_wire(Pt::new(500, 3000), Pt::new(900, 3000));
    root.add_label(Pt::new(500, 3000), "Q[0..1]", LabelKind::Local);
    let v = erc::check(&root);
    assert!(v.iter().any(|x| x.rule == "bus_to_net_conflict"), "{v:#?}");
}

// ---- Project libraries ---------------------------------------------------------------------

fn my_symbol() -> LibSymbol {
    let mut s = LibSymbol::blank("MyLib:DualBuffer", "U");
    s.description = "Two buffers in one package".into();
    s.keywords = "buffer dual".into();
    s.units = 2;
    s.footprint = "MyFootprints:SOIC-4_Test".into();
    s.footprints = vec![s.footprint.clone()];
    s.fields.push(("Manufacturer".into(), "Acme".into()));
    s.graphics.push(Graphic::Rect {
        a: (-200, 200),
        b: (200, -200),
        fill: Fill::Background,
    });
    // The supply pin is common to both units (unit 0), listed first as KiCad writes it.
    let mut vcc = symbols::pin("5", "VCC", (0, 400), 270, 200, PinType::PowerIn);
    vcc.hidden = true;
    s.pins.push(vcc);
    for (unit, (a, y)) in [(1u32, ("1", "2")), (2, ("3", "4"))] {
        let mut p = symbols::pin(a, "A", (-400, 0), 0, 200, PinType::Input);
        p.unit = unit;
        s.pins.push(p);
        let mut q = symbols::pin(y, "Y", (400, 0), 180, 200, PinType::Output);
        q.unit = unit;
        s.pins.push(q);
        s.unit_graphics.push((
            unit,
            Graphic::Poly {
                pts: vec![(-100, 100), (100, 0), (-100, -100), (-100, 100)],
                fill: Fill::None,
                width: 10,
            },
        ));
    }
    s.spice = Spice::None;
    s
}
fn my_footprint() -> LibFootprint {
    let mut f = LibFootprint::blank("MyFootprints:SOIC-4_Test");
    f.description = "Four pads".into();
    for (i, (x, y)) in [
        (-2_000_000, -635_000),
        (-2_000_000, 635_000),
        (2_000_000, 635_000),
        (2_000_000, -635_000),
    ]
    .iter()
    .enumerate()
    {
        f.pads.push(LibPad {
            number: (i + 1).to_string(),
            kind: PadKind::Smd,
            shape: PadShape::RoundRect,
            at: (*x, *y),
            size: (1_500_000, 600_000),
            drill: 0,
        });
    }
    f.pads.push(LibPad {
        number: "5".into(),
        kind: PadKind::ThroughHole,
        shape: PadShape::Circle,
        at: (0, 0),
        size: (1_600_000, 1_600_000),
        drill: 800_000,
    });
    f.silk = vec![((-1_000_000, -1_500_000), (1_000_000, -1_500_000))];
    f.fab = ((-1_000_000, -1_250_000), (1_000_000, 1_250_000));
    f.height = 1_750_000;
    f.fit_courtyard();
    f
}

#[test]
fn symbol_libraries_round_trip_in_kicad_syntax() {
    let s = my_symbol();
    let text = files::write_symbol_lib(std::slice::from_ref(&s));
    assert!(
        text.starts_with("(kicad_symbol_lib\n\t(version 20231120)"),
        "{text}"
    );
    assert!(text.contains("(symbol \"DualBuffer_1_1\""), "{text}");
    assert!(text.contains("(symbol \"DualBuffer_2_1\""), "{text}");
    let back = files::read_symbol_lib(&text, "MyLib").unwrap();
    assert_eq!(back, vec![s.clone()]);
    // Used in a schematic: the design carries the definition, and units work.
    let mut sch = Schematic::new("lib");
    let a = sch.place_symbol(&s, true, Pt::new(1000, 1000), Xf::IDENTITY);
    let b = sch.place_symbol(&s, true, Pt::new(2000, 1000), Xf::IDENTITY);
    sch.symbol_mut(b).unwrap().unit = 2;
    sch.annotate(true, false);
    assert_eq!(sch.symbol(a).unwrap().reference(), "U1");
    assert_eq!(
        sch.symbol(b).unwrap().reference(),
        "U1",
        "units share a reference"
    );
    assert_eq!(sch.symbol(b).unwrap().unit_reference(), "U1B");
    let numbers: Vec<&str> = sch
        .symbol(b)
        .unwrap()
        .pins()
        .iter()
        .map(|(p, _)| p.number.as_str())
        .collect();
    assert_eq!(numbers, vec!["5", "3", "4"]);
    assert!(!erc::check(&sch)
        .iter()
        .any(|v| v.rule == "duplicate_reference"));
    let text = files::write_schematic(&sch, "lib");
    let (back, warnings) = files::read_schematic(&text).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back.symbols.len(), 2);
    assert_eq!(back.symbols[1].unit, 2);
    assert_eq!(back.symbols[0].lib().unwrap().pins, s.pins);
    // Library table.
    let table = files::write_lib_table(false, &[("MyLib".into(), "MyLib.kicad_sym".into())]);
    assert!(
        table.contains("(uri \"${KIPRJMOD}/MyLib.kicad_sym\")"),
        "{table}"
    );
    assert_eq!(
        files::read_lib_table(&table).unwrap(),
        vec![("MyLib".to_owned(), "MyLib.kicad_sym".to_owned())]
    );
}

#[test]
fn footprint_libraries_round_trip_and_feed_update_pcb() {
    let f = my_footprint();
    let text = files::write_footprint(&f);
    assert!(text.starts_with("(footprint \"SOIC-4_Test\""), "{text}");
    assert!(text.contains("(pad \"5\" thru_hole circle"), "{text}");
    let back = files::read_footprint(&text, "MyFootprints").unwrap();
    assert_eq!(back, f);
    // A symbol that names it goes onto the board with those pads.
    let mut sch = Schematic::new("fp");
    let s = my_symbol();
    let a = sch.place_symbol(&s, true, Pt::new(1000, 1000), Xf::IDENTITY);
    let b = sch.place_symbol(&s, true, Pt::new(2000, 1000), Xf::IDENTITY);
    sch.symbol_mut(b).unwrap().unit = 2;
    // Unit A's output (pin 2) drives unit B's input (pin 3).
    let pin_at = |sch: &Schematic, id: u64, n: &str| {
        sch.symbol(id)
            .unwrap()
            .pins()
            .into_iter()
            .find(|(p, _)| p.number == n)
            .unwrap()
            .1
    };
    let ya = pin_at(&sch, a, "2");
    let ab = pin_at(&sch, b, "3");
    sch.add_wire(ya, Pt::new(ya.x, ya.y + 500));
    sch.add_wire(Pt::new(ya.x, ya.y + 500), Pt::new(ab.x, ya.y + 500));
    sch.add_wire(Pt::new(ab.x, ya.y + 500), ab);
    sch.annotate(true, false);
    let mut board = Board::new("fp");
    let changes = board.update_from_schematic_with(&sch, true, std::slice::from_ref(&f));
    assert!(changes.iter().all(|c| !c.warning), "{changes:#?}");
    assert_eq!(board.footprints.len(), 1, "one part for both units");
    let fp = &board.footprints[0];
    assert_eq!(fp.pads.len(), 5);
    assert_eq!(fp.height, 1_750_000);
    let net = |n: &str| fp.pads.iter().find(|p| p.number == n).unwrap().net;
    assert_ne!(net("2"), 0);
    assert_eq!(
        net("2"),
        net("3"),
        "the wire joins pad 2 (U1A Y) and pad 3 (U1B A)"
    );
    // Without the project library the footprint cannot be found.
    let mut other = Board::new("fp");
    let changes = other.update_from_schematic(&sch, true);
    assert!(changes
        .iter()
        .any(|c| c.warning && c.message.contains("not found")));
}

// ---- Footprints at any angle ------------------------------------------------------------------

fn dip_at(angle: i32, pos: Pt) -> Footprint {
    let mut f =
        Footprint::from_library(1, "Package_DIP:DIP-8_W7.62mm", "U1", "NE555P", pos).unwrap();
    f.angle = angle;
    f
}

#[test]
fn footprints_turn_to_any_angle() {
    let f = dip_at(450, Pt::new(50 * MM, 50 * MM));
    let p8 = f.pads.iter().find(|p| p.number == "8").unwrap();
    // Pad 8 sits 7.62 mm to the right of pad 1; turned 45° counter-clockwise on a Y-down
    // board it goes right and up by 7.62/√2 mm, to the nanometre.
    let d = (7.62e6 / 2f64.sqrt()).round() as i64;
    assert_eq!(f.pad_pos(p8), Pt::new(50 * MM + d, 50 * MM - d));
    // Its rectangle-ish oval stays an oval along the turned axis; pad 1's rectangle is a
    // turned quadrilateral.
    let p1 = f.pads.iter().find(|p| p.number == "1").unwrap();
    let Shape::Poly(c) = f.pad_shape(p1) else {
        panic!("pad 1 should be a turned rectangle");
    };
    let side = |a: Pt, b: Pt| a.dist(b);
    assert!((side(c[0], c[1]) - 1.6e6).abs() < 2.0 && (side(c[1], c[2]) - 1.6e6).abs() < 2.0);
    // Inverse transform.
    assert_eq!(f.to_local(f.pad_pos(p8)), p8.at);
    // Board files keep the angle.
    let mut b = Board::new("rot");
    b.footprints.push(f.clone());
    let text = files::write_board(&b);
    assert!(text.contains("(at 50 50 45)"), "{text}");
    let (back, _) = files::read_board(&text).unwrap();
    assert_eq!(back.footprints[0].angle, 450);
    assert_eq!(
        back.footprints[0].pad_pos(&back.footprints[0].pads[7]),
        f.pad_pos(p8)
    );
    // 22.5° is fine too.
    let g = dip_at(225, Pt::new(0, 0));
    assert_eq!(cw_eda::pcb::angle_text(g.angle), "22.5");
    assert_eq!(cw_eda::pcb::parse_angle("-22.5"), Some(3375));
}

#[test]
fn rotated_pads_plot_as_turned_regions_in_gerbers() {
    let mut b = Board::new("rot");
    b.footprints.push(dip_at(300, Pt::new(20 * MM, 20 * MM)));
    let text = gerber::layer(&b, "rot", Layer::FCu);
    let stats = gerber::parse(&text).unwrap();
    // Pad 1 (rectangular) is a region with its turned corners; the seven round pads
    // flash as circles.
    assert_eq!(stats.regions, 1, "{text}");
    assert_eq!(stats.flashes, 7, "{text}");
    let f = &b.footprints[0];
    let Shape::Poly(c) = f.pad_shape(&f.pads[0]) else {
        panic!()
    };
    for p in c {
        let coord = format!("X{}Y{}", p.x, -p.y);
        assert!(text.contains(&coord), "corner {coord} in\n{text}");
    }
    // The drill file puts the holes at the turned positions.
    let drill = gerber::drill(&b, "rot");
    let p8 = f.pad_pos(&f.pads[7]);
    let x = cw_eda::geom::mm(p8.x, 1_000_000);
    assert!(drill.contains(&format!("X{x}Y")), "{drill}");
}

#[test]
fn drc_follows_turned_pads_and_courtyards() {
    let mut b = Board::new("rot");
    let id = b.take_id();
    b.drawings.push(Drawing {
        id,
        layer: Layer::EdgeCuts,
        shape: DrawShape::Rect {
            a: Pt::new(0, 0),
            b: Pt::new(80 * MM, 60 * MM),
        },
        width: 50_000,
    });
    // Two DIPs turned 45°, one down and to the right of the other along the diagonal:
    // their bounding boxes overlap, their turned courtyards do not.
    let mut a = dip_at(450, Pt::new(20 * MM, 30 * MM));
    let mut c = dip_at(450, Pt::new(28 * MM, 38 * MM));
    a.id = b.take_id();
    c.id = b.take_id();
    c.reference = "U2".into();
    let (ya, yc) = (a.courtyard().unwrap(), c.courtyard().unwrap());
    assert!(ya.overlaps(&yc), "boxes overlap");
    b.footprints.push(a.clone());
    b.footprints.push(c);
    let v = cw_eda::drc::check(&b, None);
    assert!(!v.iter().any(|x| x.rule == "courtyards_overlap"), "{v:#?}");
    // Moved closer, they do overlap.
    b.footprints[1].pos = Pt::new(26 * MM, 36 * MM);
    let v = cw_eda::drc::check(&b, None);
    assert!(v.iter().any(|x| x.rule == "courtyards_overlap"), "{v:#?}");
    b.footprints.truncate(1);
    // A track of another net passing 0.1 mm from a turned pad's corner violates the
    // 0.2 mm clearance; 0.3 mm away it does not.
    let net = b.net_index("SIG");
    let pad1 = b.footprints[0].pad_shape(&b.footprints[0].pads[0]);
    let Shape::Poly(q) = pad1 else { panic!() };
    let top = *q.iter().min_by_key(|p| p.y).unwrap();
    for (gap, bad) in [(100_000 + 125_000, true), (300_000 + 125_000, false)] {
        let mut t = b.clone();
        let id = t.take_id();
        t.tracks.push(Track {
            id,
            a: Pt::new(top.x - 5 * MM, top.y - gap),
            b: Pt::new(top.x + 5 * MM, top.y - gap),
            width: 250_000,
            layer: Layer::FCu,
            net,
        });
        let v = cw_eda::drc::check(&t, None);
        let found = v
            .iter()
            .any(|x| x.rule == "clearance" || x.rule == "shorting_items");
        assert_eq!(found, bad, "gap {gap}: {v:#?}");
    }
}
