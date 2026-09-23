//! The KiCad projects seeded into users' home folders are designed by this engine, not
//! by hand: `Documents/KiCad/sensor-node` on every desktop under
//! `worlds/company-2026/computers`, and `Documents/KiCad/rc-filter` on bob-windows, must
//! be exactly what these generators write. Regenerate with
//! `CW_UPDATE_SAMPLES=1 cargo test -p cw-eda --test samples`, then run
//! `scripts/build-content.sh` to seed them.
//!
//! Both designs are checked here the way the in-world KiCad checks them: ERC clean,
//! every net routed, the ground zone filled, DRC clean, and every file reading back as
//! the same design.
use cw_eda::drc;
use cw_eda::erc;
use cw_eda::files;
use cw_eda::footprints::MM;
use cw_eda::geom::{Pt, Xf};
use cw_eda::netlist;
use cw_eda::pcb::{Board, DrawShape, Drawing, Layer, Track, Zone};
use cw_eda::router::Router;
use cw_eda::schematic::{ortho, LabelKind, Schematic};
use cw_eda::zones;
use std::path::PathBuf;

/// Where a sample lands, given as `<desktop>/<path in its home>`: `all` is every
/// desktop, and `macos`, `windows` or `ubuntu` the one desktop of that family.
fn homes(path: &str) -> Vec<PathBuf> {
    let computers =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../worlds/company-2026/computers");
    let (which, rest) = path.split_once('/').unwrap();
    [
        ("macos", "alice-mac/root/Users/alice"),
        ("windows", "bob-windows/root/C/Users/bob"),
        ("ubuntu", "carol-ubuntu/root/home/carol"),
    ]
    .iter()
    .filter(|(family, _)| which == "all" || which == *family)
    .map(|(_, home)| computers.join(home).join(rest))
    .collect()
}

fn wire(s: &mut Schematic, a: Pt, b: Pt, vertical_first: bool) {
    for (p, q) in ortho(a, b, vertical_first) {
        s.add_wire(p, q);
    }
}

/// Where pin `number` of symbol `id` connects on the page, and the unit step (100 mil)
/// leading away from the symbol body.
fn pin(s: &Schematic, id: u64, number: &str) -> (Pt, Pt) {
    let sym = s.symbol(id).expect("symbol");
    let lib = sym.lib().expect("library symbol");
    let p = lib.pin(number).unwrap_or_else(|| panic!("pin {number}"));
    let end = sym.to_page(p.at);
    // A pin's angle points from its end into the body; outward is the other way.
    let (dx, dy) = match p.angle {
        0 => (-100, 0),
        90 => (0, -100),
        180 => (100, 0),
        _ => (0, 100),
    };
    // Library y is up, the page's is down, and the instance transform applies after.
    let out = sym.xf.apply(Pt::new(dx, -dy));
    (end, out)
}
fn step(p: Pt, out: Pt, n: i64) -> Pt {
    Pt::new(p.x + out.x * n, p.y + out.y * n)
}

/// A short stub from a pin ending in a local label.
fn label(s: &mut Schematic, id: u64, number: &str, name: &str) {
    let (end, out) = pin(s, id, number);
    let at = step(end, out, 2);
    s.add_wire(end, at);
    s.add_label(at, name, LabelKind::Local);
}
/// A stub from a pin to a power symbol: ground hangs below, a rail rises above.
fn rail(s: &mut Schematic, id: u64, number: &str, lib_id: &str) {
    let (end, out) = pin(s, id, number);
    let mut at = step(end, out, 1);
    s.add_wire(end, at);
    let ground = lib_id == "power:GND";
    let vertical = out.x == 0 && ((out.y > 0) == ground);
    if !vertical {
        let next = Pt::new(at.x, if ground { at.y + 100 } else { at.y - 100 });
        s.add_wire(at, next);
        at = next;
    }
    s.place(lib_id, at, Xf::IDENTITY).unwrap();
}
fn value(s: &mut Schematic, id: u64, v: &str) {
    s.symbol_mut(id).unwrap().set_field("Value", v);
}

/// A 5 V sensor node: a 2-pin power header behind a reverse-protection diode and bulk
/// capacitor, an ATtiny85 with its decoupling and reset pull-up, an op-amp buffering a
/// resistive sensor on a 3-pin header into the ADC, a status LED, and a debug header.
pub fn sensor_node() -> Schematic {
    let mut s = Schematic::new("sensor-node");
    // ---- Power input: J1 → D1 → +5V, C1 bulk.
    let j1 = s
        .place("Connector:Conn_01x02", Pt::new(1500, 2000), Xf::MIRROR_Y)
        .unwrap();
    value(&mut s, j1, "Power 5V");
    let d1 = s
        .place("Device:D", Pt::new(2500, 2000), Xf::MIRROR_Y)
        .unwrap();
    value(&mut s, d1, "1N5819");
    let (a, _) = pin(&s, d1, "2");
    let (k, _) = pin(&s, d1, "1");
    let (j1p1, _) = pin(&s, j1, "1");
    wire(&mut s, j1p1, a, false);
    rail(&mut s, j1, "2", "power:GND");
    let node = Pt::new(3200, 2000);
    s.add_wire(k, node);
    s.place("power:+5V", node, Xf::IDENTITY).unwrap();
    s.place("power:PWR_FLAG", node, Xf::IDENTITY).unwrap();
    let c1 = s
        .place("Device:C", Pt::new(3200, 2450), Xf::IDENTITY)
        .unwrap();
    value(&mut s, c1, "10u");
    let (c1top, _) = pin(&s, c1, "1");
    s.add_wire(node, c1top);
    rail(&mut s, c1, "2", "power:GND");
    // Ground gets its flag once, on the input connector's ground symbol.
    s.place("power:PWR_FLAG", Pt::new(1800, 2200), Xf::IDENTITY)
        .unwrap();
    // ---- The microcontroller.
    let u1 = s
        .place(
            "MCU_Microchip_ATtiny:ATtiny85-20P",
            Pt::new(5500, 3200),
            Xf::IDENTITY,
        )
        .unwrap();
    rail(&mut s, u1, "8", "power:+5V");
    rail(&mut s, u1, "4", "power:GND");
    label(&mut s, u1, "5", "LED");
    label(&mut s, u1, "6", "TX");
    label(&mut s, u1, "7", "SENSE");
    label(&mut s, u1, "1", "~RESET");
    for n in ["2", "3"] {
        let (end, _) = pin(&s, u1, n);
        s.add_no_connect(end);
    }
    let c2 = s
        .place("Device:C", Pt::new(4300, 3200), Xf::IDENTITY)
        .unwrap();
    value(&mut s, c2, "100n");
    rail(&mut s, c2, "1", "power:+5V");
    rail(&mut s, c2, "2", "power:GND");
    let r2 = s
        .place("Device:R", Pt::new(7300, 2600), Xf::IDENTITY)
        .unwrap();
    value(&mut s, r2, "10k");
    rail(&mut s, r2, "1", "power:+5V");
    label(&mut s, r2, "2", "~RESET");
    // ---- Status LED.
    let r1 = s
        .place("Device:R", Pt::new(8500, 2800), Xf::IDENTITY)
        .unwrap();
    value(&mut s, r1, "330");
    label(&mut s, r1, "1", "LED");
    let d2 = s
        .place("Device:LED", Pt::new(8500, 3500), Xf::ROT_CCW)
        .unwrap();
    value(&mut s, d2, "GREEN");
    let (r1b, _) = pin(&s, r1, "2");
    let (d2a, _) = pin(&s, d2, "2");
    wire(&mut s, r1b, d2a, true);
    rail(&mut s, d2, "1", "power:GND");
    // ---- Sensor header and its pull-up divider.
    let j2 = s
        .place("Connector:Conn_01x03", Pt::new(1500, 5000), Xf::MIRROR_Y)
        .unwrap();
    value(&mut s, j2, "Sensor");
    rail(&mut s, j2, "1", "power:+5V");
    label(&mut s, j2, "2", "SENSOR");
    rail(&mut s, j2, "3", "power:GND");
    let r3 = s
        .place("Device:R", Pt::new(3000, 4600), Xf::IDENTITY)
        .unwrap();
    value(&mut s, r3, "10k");
    rail(&mut s, r3, "1", "power:+5V");
    label(&mut s, r3, "2", "SENSOR");
    // ---- Op-amp buffer: unity gain, output to the ADC.
    let u2 = s
        .place("Simulation_SPICE:OPAMP", Pt::new(5500, 5000), Xf::IDENTITY)
        .unwrap();
    value(&mut s, u2, "MCP6001");
    label(&mut s, u2, "1", "SENSOR");
    rail(&mut s, u2, "3", "power:+5V");
    rail(&mut s, u2, "4", "power:GND");
    let (out, out_dir) = pin(&s, u2, "5");
    let tap = step(out, out_dir, 2);
    s.add_wire(out, tap);
    s.add_label(tap, "SENSE", LabelKind::Local);
    let (minus, minus_dir) = pin(&s, u2, "2");
    let turn = step(minus, minus_dir, 2);
    s.add_wire(minus, turn);
    let low = Pt::new(turn.x, 5700);
    s.add_wire(turn, low);
    s.add_wire(low, Pt::new(tap.x, 5700));
    s.add_wire(Pt::new(tap.x, 5700), tap);
    let c3 = s
        .place("Device:C", Pt::new(7000, 5000), Xf::IDENTITY)
        .unwrap();
    value(&mut s, c3, "100n");
    rail(&mut s, c3, "1", "power:+5V");
    rail(&mut s, c3, "2", "power:GND");
    // ---- Debug header: serial out, reset, ground.
    let j3 = s
        .place("Connector:Conn_01x03", Pt::new(8700, 5000), Xf::IDENTITY)
        .unwrap();
    value(&mut s, j3, "Debug");
    label(&mut s, j3, "1", "TX");
    label(&mut s, j3, "2", "~RESET");
    rail(&mut s, j3, "3", "power:GND");
    s.add_text(
        Pt::new(1500, 1500),
        "Sensor node rev A — 5 V in, ATtiny85, buffered analog sensor",
    );
    s.cleanup_junctions();
    s.annotate(true, false);
    s
}

/// Route every net but ground on the front, then hand ground to a back-side zone.
fn route(b: &mut Board, gnd: usize) {
    for layer in [Layer::FCu, Layer::BCu] {
        for _ in 0..100 {
            let rats: Vec<_> = drc::ratsnest(b)
                .into_iter()
                .filter(|r| r.net != gnd)
                .collect();
            let Some(rat) = rats.first() else {
                return;
            };
            let router = Router::new(b, rat.net, layer, b.rules.track_width);
            let path = router
                .walkaround(rat.a, rat.b, true)
                .or_else(|| router.walkaround(rat.b, rat.a, true));
            let Some(path) = path else {
                if layer == Layer::BCu {
                    panic!(
                        "no route for {} from {:?} to {:?}",
                        b.net_name(rat.net),
                        rat.a,
                        rat.b
                    );
                }
                break;
            };
            for w in path.windows(2) {
                let id = b.take_id();
                b.tracks.push(Track {
                    id,
                    a: w[0],
                    b: w[1],
                    width: b.rules.track_width,
                    layer,
                    net: rat.net,
                });
            }
        }
    }
}

fn place(b: &mut Board, r: &str, x_mm: f64, y_mm: f64, angle: i32) {
    let f = b
        .footprints
        .iter_mut()
        .find(|f| f.reference == r)
        .unwrap_or_else(|| panic!("{r} is not on the board"));
    f.pos = Pt::new((x_mm * MM as f64) as i64, (y_mm * MM as f64) as i64);
    f.angle = angle;
}
fn outline(b: &mut Board, w_mm: i64, h_mm: i64) {
    let id = b.take_id();
    b.drawings.push(Drawing {
        id,
        layer: Layer::EdgeCuts,
        shape: DrawShape::Rect {
            a: Pt::new(0, 0),
            b: Pt::new(w_mm * MM, h_mm * MM),
        },
        width: 50_000,
    });
}
fn ground_zone(b: &mut Board, w_mm: i64, h_mm: i64) {
    let gnd = b.nets.iter().position(|n| n == "GND").unwrap();
    let id = b.take_id();
    b.zones.push(Zone {
        id,
        net: gnd,
        layer: Layer::BCu,
        outline: vec![
            Pt::new(0, 0),
            Pt::new(w_mm * MM, 0),
            Pt::new(w_mm * MM, h_mm * MM),
            Pt::new(0, h_mm * MM),
        ],
        clearance: 300_000,
        min_width: 250_000,
        thermal_gap: 500_000,
        spoke_width: 500_000,
        fill: vec![],
        filled: false,
        keepout: false,
    });
}

/// The sensor node's board: 70 × 45 mm, headers along the edges, the two DIPs in the
/// middle, ground poured on the back.
pub fn sensor_node_board(s: &Schematic) -> Board {
    let mut b = Board::new("sensor-node");
    let changes = b.update_from_schematic(s, true);
    assert!(changes.iter().all(|c| !c.warning), "{changes:#?}");
    assert_eq!(b.footprints.len(), 13);
    place(&mut b, "J1", 5.0, 8.0, 0);
    place(&mut b, "D1", 12.0, 6.0, 0);
    place(&mut b, "C1", 22.0, 12.0, 0);
    place(&mut b, "C2", 26.0, 20.0, 900);
    place(&mut b, "U1", 34.0, 16.0, 0);
    place(&mut b, "R2", 44.0, 6.0, 0);
    place(&mut b, "R1", 46.0, 30.0, 0);
    place(&mut b, "D2", 62.0, 30.0, 900);
    place(&mut b, "J2", 5.0, 26.0, 0);
    place(&mut b, "R3", 10.0, 38.0, 0);
    place(&mut b, "U2", 24.0, 32.0, 0);
    place(&mut b, "C3", 36.0, 40.0, 0);
    place(&mut b, "J3", 64.0, 8.0, 0);
    outline(&mut b, 70, 45);
    let gnd = b.nets.iter().position(|n| n == "GND").unwrap();
    route(&mut b, gnd);
    ground_zone(&mut b, 70, 45);
    zones::fill_all(&mut b);
    b
}

/// The RC low-pass from the design-flow tests, drawn as bob keeps it: a filter to
/// measure on the bench.
pub fn rc_filter() -> Schematic {
    let mut s = Schematic::new("rc-filter");
    let r1 = s
        .place("Device:R", Pt::new(3000, 2500), Xf::ROT_CCW)
        .unwrap();
    value(&mut s, r1, "1k");
    let c1 = s
        .place("Device:C", Pt::new(3500, 3000), Xf::IDENTITY)
        .unwrap();
    value(&mut s, c1, "1u");
    let j1 = s
        .place("Connector:Conn_01x03", Pt::new(4500, 2400), Xf::IDENTITY)
        .unwrap();
    value(&mut s, j1, "in/out/GND");
    let v1 = s
        .place("Simulation_SPICE:VPULSE", Pt::new(2000, 3000), Xf::IDENTITY)
        .unwrap();
    value(&mut s, v1, "pulse(0 1 0 1n 1n 1 2)");
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
    s.add_text(Pt::new(2000, 2000), "RC low-pass, fc = 159 Hz");
    s.set_sim_command(".tran 10u 5m");
    s.cleanup_junctions();
    s.annotate(true, false);
    s
}
pub fn rc_filter_board(s: &Schematic) -> Board {
    let mut b = Board::new("rc-filter");
    let changes = b.update_from_schematic(s, true);
    assert!(changes.iter().all(|c| !c.warning), "{changes:#?}");
    place(&mut b, "J1", 5.0, 5.0, 0);
    place(&mut b, "R1", 10.0, 5.0, 0);
    place(&mut b, "C1", 10.0, 10.08, 0);
    outline(&mut b, 25, 15);
    let gnd = b.nets.iter().position(|n| n == "GND").unwrap();
    route(&mut b, gnd);
    ground_zone(&mut b, 25, 15);
    zones::fill_all(&mut b);
    b
}

/// The files of one project, as the Project Manager's New Project writes them.
fn project_files(name: &str, s: &Schematic, b: &Board) -> Vec<(String, String)> {
    let mut out = vec![(
        format!("{name}.kicad_pro"),
        files::write_project(name, &b.rules),
    )];
    out.extend(files::write_design(s, name, &format!("{name}.kicad_sch")));
    out.push((format!("{name}.kicad_pcb"), files::write_board(b)));
    out.push((format!("{name}-bom.csv"), netlist::bom_csv(s)));
    out
}

fn check(dir: &str, files: &[(String, String)]) {
    for folder in homes(dir) {
        if std::env::var_os("CW_UPDATE_SAMPLES").is_some() {
            std::fs::create_dir_all(&folder).unwrap();
            for (name, text) in files {
                std::fs::write(folder.join(name), text).unwrap();
            }
            continue;
        }
        for (name, text) in files {
            let current = std::fs::read_to_string(folder.join(name)).unwrap_or_default();
            assert!(
                current == *text,
                "{}/{name} is not what the engine writes; regenerate with CW_UPDATE_SAMPLES=1",
                folder.display()
            );
        }
    }
}

fn design_checks(name: &str, s: &Schematic, b: &Board) {
    let v = erc::check(s);
    assert!(v.is_empty(), "{name} ERC: {v:#?}");
    let rats = drc::ratsnest(b);
    assert!(rats.is_empty(), "{name} unrouted: {rats:?}");
    let refs: Vec<(String, String)> = s
        .symbols
        .iter()
        .filter(|x| x.on_board && !x.is_power())
        .map(|x| (x.reference().to_owned(), x.field("Footprint").to_owned()))
        .collect();
    let v = drc::check(b, Some(&refs));
    assert!(v.is_empty(), "{name} DRC: {v:#?}");
    // And every file reads back as the same design.
    let text = files::write_schematic(s, name);
    let (back, warnings) = files::read_schematic(&text).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(&back, s);
    let text = files::write_board(b);
    let (back, warnings) = files::read_board(&text).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(&back, b);
    let pro = files::write_project(name, &b.rules);
    assert_eq!(files::read_project(&pro).unwrap(), b.rules);
}

#[test]
fn the_seeded_sensor_node_is_a_clean_routed_design() {
    let s = sensor_node();
    let conn = cw_eda::connectivity::analyze(&s);
    let mut names: Vec<&str> = conn.nets.iter().map(|n| n.name.as_str()).collect();
    names.sort_unstable();
    for net in ["+5V", "GND", "LED", "SENSE", "SENSOR", "TX", "~RESET"] {
        assert!(names.contains(&net), "{net} missing from {names:?}");
    }
    let sense = conn.net_named("SENSE").unwrap();
    let pins: Vec<String> = sense
        .pins
        .iter()
        .map(|p| format!("{}.{}", p.reference, p.number))
        .collect();
    assert_eq!(pins, vec!["U1.7", "U2.2", "U2.5"]);
    let b = sensor_node_board(&s);
    design_checks("sensor-node", &s, &b);
    assert!(b.tracks.len() >= 10, "{} tracks", b.tracks.len());
    let bom = netlist::bom_csv(&s);
    assert_eq!(bom.lines().count(), 12, "grouped rows: {bom}");
    check(
        "all/Documents/KiCad/sensor-node",
        &project_files("sensor-node", &s, &b),
    );
}

#[test]
fn the_seeded_rc_filter_is_a_clean_routed_design() {
    let s = rc_filter();
    let b = rc_filter_board(&s);
    design_checks("rc-filter", &s, &b);
    let deck = netlist::spice_netlist(&s, "rc-filter", None).unwrap();
    assert!(deck.text.contains("R1 in out 1k"), "{}", deck.text);
    check(
        "windows/Documents/KiCad/rc-filter",
        &project_files("rc-filter", &s, &b),
    );
}
