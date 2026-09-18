//! KiCad driven the way a person drives it: pointer clicks and drags on what is painted,
//! keystrokes into its fields. An RC low-pass is drawn in the Schematic Editor, simulated
//! (the plotted curve has the RC time constant), carried to the PCB Editor, placed,
//! outlined, routed, zoned, checked clean by DRC, and plotted to Gerber and drill files
//! that really are on the machine's disk and parse.
use computerworld::{reference_world, Scene, World};
use cw_applications::apps::kicad::{Frame, Kicad};
use cw_applications::{AppState, NativeApp};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const W: u32 = 1600;
const H: u32 = 1000;

struct Desk {
    world: World,
    actor: String,
    machine: &'static str,
    home: &'static str,
}

fn desk(machine: &'static str, profile: &str, home: &'static str) -> Desk {
    let mut d = reference_world();
    d.metadata["desktop_themes"] = json!({ machine: profile });
    d.metadata["desktop_apps"] = json!([
        {"id":"kicad","label":"KiCad","kind":"native","url":"","icon":"kicad"},
    ]);
    for c in &mut d.computers {
        if c.id == machine {
            c.installed_apps.push("kicad".into());
        }
    }
    let mut world = World::new(d, 7).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop(
            if machine == "alice-mac" {
                "alice"
            } else if machine == "bob-windows" {
                "bob"
            } else {
                "carol"
            },
            machine,
        ))
        .unwrap();
    Desk {
        world,
        actor,
        machine,
        home,
    }
}

impl Desk {
    fn try_act(&mut self, family: &str, op: &str, payload: Value) -> Result<Value, String> {
        let r = self
            .world
            .step(
                &self.actor,
                vec![ActionEnvelope::new(family, op, self.machine, payload)],
            )
            .unwrap();
        let o = &r.outcomes[0];
        if o.success {
            Ok(o.value.clone())
        } else {
            Err(format!("{:?}", o.error))
        }
    }
    fn act(&mut self, family: &str, op: &str, payload: Value) -> Value {
        self.try_act(family, op, payload.clone())
            .unwrap_or_else(|e| panic!("{family} {op} {payload}: {e}"))
    }
    fn scene(&self) -> Scene {
        self.world.scene(&self.actor, W, H).unwrap()
    }
    fn windows(&self) -> Vec<(u64, Kicad)> {
        let s = self.world.interfaces().session(&self.actor).unwrap();
        s.machines[self.machine]
            .desktop
            .windows
            .values()
            .filter_map(|w| match &w.state {
                AppState::Native(NativeApp::Kicad(k)) => Some((w.id, k.clone())),
                _ => None,
            })
            .collect()
    }
    fn kicad(&self, frame: Frame) -> Kicad {
        self.windows()
            .into_iter()
            .find(|(_, k)| k.frame == frame)
            .unwrap_or_else(|| panic!("no {frame:?} window"))
            .1
    }
    fn window(&self, frame: Frame) -> u64 {
        self.windows()
            .into_iter()
            .find(|(_, k)| k.frame == frame)
            .unwrap_or_else(|| panic!("no {frame:?} window"))
            .0
    }
    fn focused(&self) -> u64 {
        let s = self.world.interfaces().session(&self.actor).unwrap();
        s.machines[self.machine].desktop.focused.unwrap()
    }
    /// Maximize by the window's own title bar: a double click on it.
    fn maximize(&mut self, frame: Frame) {
        let id = self.window(frame);
        let scene = self.scene();
        let bar = format!("window:{id}:drag");
        let n = scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some(bar.as_str()))
            .expect("a title bar");
        let b = n.transform.bounds(n.bounds);
        self.pointer(
            "double_click",
            b.x + b.width as i32 / 2,
            b.y + b.height as i32 / 2,
        );
    }
    fn focus(&mut self, frame: Frame) {
        let id = self.window(frame);
        self.act("application.v1", "focus", json!({ "window": id }));
    }
    /// The node of the focused window whose control is `target` (exactly, or by prefix
    /// when `prefix` is set), with its on-screen bounds.
    fn find(&self, target: &str, prefix: bool) -> Option<(cw_scene::Rect, String)> {
        let scene = self.scene();
        let want = format!("window:{}:content:", self.focused());
        scene.nodes.iter().rev().find_map(|n| {
            let i = n.interaction.as_deref()?;
            let inner = i.strip_prefix(&want)?;
            let ok = if prefix {
                inner.starts_with(target)
            } else {
                inner == target
            };
            ok.then(|| (n.transform.bounds(n.bounds), inner.to_owned()))
        })
    }
    fn click(&mut self, target: &str) {
        let (b, _) = self
            .find(target, false)
            .unwrap_or_else(|| panic!("{target} is not on screen"));
        self.pointer("click", b.x + b.width as i32 / 2, b.y + b.height as i32 / 2);
    }
    fn click_prefix(&mut self, target: &str) {
        let (b, _) = self
            .find(target, true)
            .unwrap_or_else(|| panic!("{target}… is not on screen"));
        self.pointer("click", b.x + b.width as i32 / 2, b.y + b.height as i32 / 2);
    }
    fn double_click(&mut self, x: i32, y: i32) {
        self.pointer("double_click", x, y);
    }
    fn pointer(&mut self, op: &str, x: i32, y: i32) -> Value {
        self.act(
            "pointer.v1",
            op,
            json!({"x": x, "y": y, "width": W, "height": H}),
        )
    }
    fn drag(&mut self, from: (i32, i32), to: (i32, i32)) {
        self.pointer("down", from.0, from.1);
        let mid = ((from.0 + to.0) / 2, (from.1 + to.1) / 2);
        self.pointer("move", mid.0, mid.1);
        self.pointer("move", to.0, to.1);
        self.pointer("up", to.0, to.1);
    }
    fn text(&mut self, text: &str) {
        self.act("keyboard.v1", "type", json!({ "text": text }));
    }
    fn key(&mut self, key: &str) {
        self.act("keyboard.v1", "key", json!({ "key": key }));
    }
    /// Canvas bounds and the view it was painted with (x0, y0, zoom).
    fn canvas(&self, frame: &str) -> (cw_scene::Rect, i64, i64, i64) {
        let (b, t) = self
            .find(&format!("kicad:canvas:{frame}:"), true)
            .unwrap_or_else(|| panic!("no {frame} canvas"));
        let v: Vec<i64> = t
            .split(':')
            .skip(3)
            .take(3)
            .map(|x| x.parse().unwrap())
            .collect();
        (b, v[0], v[1], v[2])
    }
    /// Screen pixel for a schematic point in mils.
    fn sch(&self, x: i64, y: i64) -> (i32, i32) {
        let (b, x0, y0, z) = self.canvas("sch");
        (
            b.x + ((x - x0) * z / 1000) as i32,
            b.y + ((y - y0) * z / 1000) as i32,
        )
    }
    /// Screen pixel for a board point in nanometres.
    fn pcb(&self, x: i64, y: i64) -> (i32, i32) {
        let (b, x0, y0, z) = self.canvas("pcb");
        (
            b.x + ((x / 1000 - x0) * z / 1000) as i32,
            b.y + ((y / 1000 - y0) * z / 1000) as i32,
        )
    }
    /// Screen pixel for a Symbol Editor point in library mils (Y up).
    fn symed(&self, x: i64, y: i64) -> (i32, i32) {
        let (b, x0, y0, z) = self.canvas("symed");
        (
            b.x + ((x - x0) * z / 1000) as i32,
            b.y + ((-y - y0) * z / 1000) as i32,
        )
    }
    /// Screen pixel for a Footprint Editor point in nanometres.
    fn fped(&self, x: i64, y: i64) -> (i32, i32) {
        let (b, x0, y0, z) = self.canvas("fped");
        (
            b.x + ((x / 1000 - x0) * z / 1000) as i32,
            b.y + ((y / 1000 - y0) * z / 1000) as i32,
        )
    }
    /// The pixels of the focused window's 3D view.
    fn view3d_pixels(&self) -> Vec<u8> {
        let want = format!("window:{}:content:kicad:canvas:3d:", self.focused());
        let scene = self.scene();
        let canvas = scene
            .nodes
            .iter()
            .find(|n| {
                n.interaction
                    .as_deref()
                    .is_some_and(|i| i.starts_with(&want))
            })
            .expect("a 3D canvas");
        let b = canvas.transform.bounds(canvas.bounds);
        scene
            .nodes
            .iter()
            .find_map(|n| match &n.primitive {
                cw_scene::Primitive::Image { rgba, .. } if n.transform.bounds(n.bounds) == b => {
                    Some(rgba.clone())
                }
                _ => None,
            })
            .expect("the 3D view is an image")
    }
    fn click_sch(&mut self, x: i64, y: i64) {
        let (px, py) = self.sch(x, y);
        self.pointer("click", px, py);
    }
    fn click_pcb(&mut self, x: i64, y: i64) {
        let (px, py) = self.pcb(x, y);
        self.pointer("click", px, py);
    }
    fn file(&self, path: &str) -> String {
        String::from_utf8(self.world.runtime().read_file(self.machine, path).unwrap()).unwrap()
    }
    fn replace_field(&mut self, field: &str, value: &str) {
        self.click(&format!("kicad:field:{field}"));
        let len = match self
            .focused_kicad()
            .ui
            .dialog
            .as_mut()
            .and_then(|d| d.field_mut(field).cloned())
        {
            Some(v) => v.chars().count(),
            None => panic!("no field {field}"),
        };
        for _ in 0..len {
            self.key("Backspace");
        }
        self.text(value);
    }
    fn focused_kicad(&self) -> Kicad {
        let id = self.focused();
        self.windows()
            .into_iter()
            .find(|(w, _)| *w == id)
            .unwrap()
            .1
    }
    fn place(&mut self, tool: &str, filter: &str, lib_id: &str, at: (i64, i64), rotate: bool) {
        self.click(&format!("kicad:sch:tool:{tool}"));
        self.text(filter);
        self.click(&format!("kicad:dlg:pick:{lib_id}"));
        self.click("kicad:dlg:ok");
        if rotate {
            self.key("r");
        }
        self.click_sch(at.0, at.1);
    }
    fn wire(&mut self, points: &[(i64, i64)]) {
        self.click("kicad:sch:tool:wire");
        for (x, y) in points {
            self.click_sch(*x, *y);
        }
        assert!(
            self.kicad(Frame::Schematic).ui.sch.wire.is_empty(),
            "wire to {points:?} did not finish on its target"
        );
    }
}

/// Draw the RC filter through the UI; returns the project directory.
fn draw_rc(d: &mut Desk) -> String {
    d.act("application.v1", "launch", json!({"kind": "kicad"}));
    d.maximize(Frame::ProjectManager);
    d.click("kicad:pm:new");
    d.replace_field("name", "rcfilter");
    d.click("kicad:dlg:ok");
    let dir = format!("{}/Documents/KiCad/rcfilter", d.home);
    for ext in ["kicad_pro", "kicad_sch", "kicad_pcb"] {
        let text = d.file(&format!("{dir}/rcfilter.{ext}"));
        assert!(!text.is_empty(), "{ext} was not created");
    }
    d.click("kicad:pm:launch:sch");
    d.maximize(Frame::Schematic);
    // Closer in than the whole page, so every click lands well inside its 50 mil cell:
    // point at where the circuit will go and zoom in about the pointer, as KiCad's F1 does.
    let (x, y) = d.sch(3200, 3000);
    d.pointer("move", x, y);
    d.key("F1");
    d.key("F1");
    d.place(
        "symbol",
        "VPULSE",
        "Simulation_SPICE:VPULSE",
        (2000, 3000),
        false,
    );
    d.place("symbol", "resistor", "Device:R", (3000, 2500), true);
    // The capacitor is dropped off to the side and dragged into place.
    d.place("symbol", "capacitor", "Device:C", (3700, 3100), false);
    d.place(
        "symbol",
        "Conn_01x03",
        "Connector:Conn_01x03",
        (4500, 2400),
        false,
    );
    d.place("power", "GND", "power:GND", (2000, 3500), false);
    d.place("power", "GND", "power:GND", (3500, 3500), false);
    d.place("power", "GND", "power:GND", (4100, 2600), false);
    d.place("power", "PWR_FLAG", "power:PWR_FLAG", (2000, 3500), false);
    d.key("Escape");
    d.click("kicad:sch:tool:select");
    let from = d.sch(3700, 3100);
    let to = d.sch(3500, 3000);
    d.drag(from, to);
    let s = d.kicad(Frame::Schematic).session.schematic;
    let c1 = s.by_reference("C1").expect("C1 placed");
    assert_eq!(
        (c1.pos.x, c1.pos.y),
        (3500, 3000),
        "C1 was not dragged into place"
    );
    let refs: Vec<String> = s.symbols.iter().map(|x| x.reference().to_owned()).collect();
    for r in ["V1", "R1", "C1", "J1", "#PWR01", "#FLG01"] {
        assert!(refs.contains(&r.to_owned()), "{r} missing from {refs:?}");
    }
    // Wires: each ends on a pin or wire, which finishes it.
    d.wire(&[(2000, 2800), (2000, 2500), (2850, 2500)]);
    d.wire(&[(3150, 2500), (3500, 2500), (3500, 2850)]);
    d.wire(&[(4300, 2500), (3500, 2500)]);
    d.wire(&[(2000, 3200), (2000, 3500)]);
    d.wire(&[(3500, 3150), (3500, 3500)]);
    d.wire(&[(4300, 2600), (4100, 2600)]);
    d.click("kicad:sch:tool:wire");
    d.click_sch(4300, 2400);
    d.click_sch(4000, 2400);
    let (x, y) = d.sch(4000, 2400);
    d.double_click(x, y);
    // Labels name the nets and tie the connector's input to the source.
    for (at, name) in [
        ((3500, 2700), "out"),
        ((2400, 2500), "in"),
        ((4000, 2400), "in"),
    ] {
        d.click("kicad:sch:tool:label");
        d.click_sch(at.0, at.1);
        d.text(name);
        d.click("kicad:dlg:ok");
    }
    d.click("kicad:sch:tool:select");
    // Values through the properties dialog, opened on a double click.
    for (at, value) in [
        ((3000, 2500), "1k"),
        ((3500, 3000), "1u"),
        ((2000, 3000), "pulse(0 1 0 1n 1n 1 2)"),
    ] {
        d.click_sch(at.0, at.1);
        let (x, y) = d.sch(at.0, at.1);
        d.double_click(x, y);
        d.replace_field("Value", value);
        d.click("kicad:dlg:ok");
    }
    let s = d.kicad(Frame::Schematic).session.schematic;
    assert_eq!(s.by_reference("R1").unwrap().value(), "1k");
    assert_eq!(s.by_reference("C1").unwrap().value(), "1u");
    // ERC is clean: every pin connected, ground driven by the power flag.
    d.click("kicad:sch:erc");
    d.click("kicad:dlg:run");
    let erc = d.kicad(Frame::Schematic).session.erc.expect("ERC ran");
    assert!(erc.is_empty(), "{erc:#?}");
    d.click("kicad:dlg:ok");
    d.key("Ctrl+s");
    dir
}

#[test]
fn an_rc_filter_goes_from_schematic_through_simulation_to_gerbers() {
    let mut d = desk("alice-mac", "virtual-macos-golden-gate", "/Users/alice");
    let dir = draw_rc(&mut d);
    let saved = d.file(&format!("{dir}/rcfilter.kicad_sch"));
    let (on_disk, _) = cw_eda::files::read_schematic(&saved).unwrap();
    assert_eq!(
        on_disk,
        d.kicad(Frame::Schematic).session.schematic,
        "saved schematic differs"
    );

    // ---- Simulation -------------------------------------------------------------
    d.click("kicad:sch:simulator");
    d.maximize(Frame::Simulator);
    d.click("kicad:sim:settings");
    d.click("kicad:dlg:tab:3");
    d.replace_field("Time step", "10u");
    d.replace_field("Final time", "5m");
    d.click("kicad:dlg:ok");
    d.click("kicad:sim:run");
    let sim = d.kicad(Frame::Simulator).session.sim;
    assert!(sim.plot.is_some(), "no results: {:?}", sim.log);
    // Probe the output net on the schematic.
    d.click("kicad:sim:probe");
    assert_eq!(d.focused(), d.window(Frame::Schematic));
    d.click_sch(3500, 2700);
    d.key("Escape");
    d.focus(Frame::Simulator);
    let sim = d.kicad(Frame::Simulator).session.sim;
    assert_eq!(sim.shown, vec!["V(out)".to_owned()]);
    // The plotted curve crosses 1 - 1/e at the time constant, R·C = 1 ms.
    let plot = sim.plot.clone().unwrap();
    let xs = plot.xs();
    let ys: Vec<f64> = plot
        .trace("V(out)")
        .unwrap()
        .y
        .iter()
        .map(|b| f32::from_bits(*b) as f64)
        .collect();
    let target = 1.0 - (-1.0f64).exp();
    let i = ys.iter().position(|v| *v >= target).unwrap();
    let tau = xs[i - 1] + (target - ys[i - 1]) * (xs[i] - xs[i - 1]) / (ys[i] - ys[i - 1]);
    assert!(
        (tau - 1e-3).abs() < 1e-5,
        "plotted time constant {tau} s is not 1 ms (±1%)"
    );
    // A cursor dragged onto 1 ms reads the same value.
    d.click("kicad:sim:cursor:0");
    let (b, _) = d.find("kicad:canvas:plot:", true).unwrap();
    let x_at = |t: f64| b.x + (t / 5e-3 * b.width as f64) as i32;
    d.drag((x_at(1.25e-3), b.y + 40), (x_at(1e-3), b.y + 60));
    let sim = d.kicad(Frame::Simulator).session.sim;
    let cx = f32::from_bits(sim.cursors[0].unwrap()) as f64;
    assert!(
        (cx - 1e-3).abs() < 5e-3 / b.width as f64 * 1.5,
        "cursor at {cx}"
    );
    let at = sim.plot.unwrap().value_at("V(out)", cx).unwrap();
    assert!((at - target).abs() < 0.01, "cursor reads {at}");

    // ---- Board ----------------------------------------------------------------------
    d.focus(Frame::Schematic);
    d.click("kicad:sch:update-pcb");
    d.maximize(Frame::Pcb);
    d.click("kicad:dlg:apply");
    d.click("kicad:dlg:ok");
    let b = d.kicad(Frame::Pcb).session.board;
    let mut refs: Vec<&str> = b.footprints.iter().map(|f| f.reference.as_str()).collect();
    refs.sort();
    assert_eq!(
        refs,
        vec!["C1", "J1", "R1"],
        "the simulation source is not a part"
    );
    // Drag each footprint so its pad 1 lands on the grid position planned for it.
    const MM: i64 = 1_000_000;
    for (r, target) in [
        ("J1", (10 * MM, 10 * MM)),
        ("R1", (15 * MM, 10 * MM)),
        ("C1", (15 * MM, 17 * MM)),
    ] {
        let b = d.kicad(Frame::Pcb).session.board;
        let f = b.footprints.iter().find(|f| f.reference == r).unwrap();
        let pad1 = f.pad_pos(f.pads.iter().find(|p| p.number == "1").unwrap());
        let (dx, dy) = (target.0 - pad1.x, target.1 - pad1.y);
        // Grab the body a little away from the pad, and move by the whole offset.
        let grab = (pad1.x + 500_000, pad1.y);
        let from = d.pcb(grab.0, grab.1);
        let to = d.pcb(grab.0 + dx, grab.1 + dy);
        d.drag(from, to);
    }
    let b = d.kicad(Frame::Pcb).session.board;
    let pad = |r: &str, n: &str| {
        let f = b.footprints.iter().find(|f| f.reference == r).unwrap();
        f.pad_pos(f.pads.iter().find(|p| p.number == n).unwrap())
    };
    for (r, n, (x, y)) in [
        ("J1", "1", (10 * MM, 10 * MM)),
        ("R1", "1", (15 * MM, 10 * MM)),
        ("C1", "1", (15 * MM, 17 * MM)),
    ] {
        let p = pad(r, n);
        assert!(
            (p.x - x).abs() <= 250_000 && (p.y - y).abs() <= 250_000,
            "{r} pad {n} at {p:?}"
        );
    }
    // Board outline on Edge.Cuts, with the view pulled back to show where it goes.
    d.click_prefix("kicad:pcb:zoom:fit:");
    d.click_prefix("kicad:pcb:zoom:out:");
    d.click("kicad:pcb:layer:Edge.Cuts");
    d.click("kicad:pcb:tool:rect");
    d.click_pcb(5 * MM, 5 * MM);
    d.click_pcb(30 * MM, 20 * MM);
    assert!(d.kicad(Frame::Pcb).session.board.outline().is_some());
    // Route on the front copper, pad to pad.
    d.click("kicad:pcb:layer:F.Cu");
    d.click("kicad:pcb:tool:route");
    let (j1, j2, j3) = (pad("J1", "1"), pad("J1", "2"), pad("J1", "3"));
    let (r1, r2) = (pad("R1", "1"), pad("R1", "2"));
    let (c1, c2) = (pad("C1", "1"), pad("C1", "2"));
    d.click_pcb(j1.x, j1.y);
    d.click_pcb(r1.x, r1.y);
    d.click_pcb(r2.x, r2.y);
    d.click_pcb(r2.x, 12_500_000);
    d.click_pcb(j2.x, j2.y);
    d.click_pcb(c1.x, c1.y);
    d.click_pcb(c1.x, 12_500_000);
    // Ground runs under C1 rather than across the output track.
    d.click_pcb(j3.x, j3.y);
    d.click_pcb(11_250_000, 18_500_000);
    d.click_pcb(c2.x, c2.y);
    let b = d.kicad(Frame::Pcb).session.board;
    assert!(b.tracks.len() >= 5, "{} tracks", b.tracks.len());
    assert!(
        cw_eda::drc::ratsnest(&b).is_empty(),
        "{:?}",
        cw_eda::drc::ratsnest(&b)
    );
    // A ground pour on the back, drawn corner by corner and closed with a double click.
    d.click("kicad:pcb:layer:B.Cu");
    d.click("kicad:pcb:tool:zone");
    for (x, y) in [(6, 6), (29, 6), (29, 19), (6, 19)] {
        d.click_pcb(x * MM, y * MM);
    }
    let (x, y) = d.pcb(6 * MM, 19 * MM);
    d.double_click(x, y);
    d.click("kicad:dlg:ok");
    let b = d.kicad(Frame::Pcb).session.board;
    assert_eq!(b.zones.len(), 1);
    assert_eq!(b.net_name(b.zones[0].net), "GND");
    assert!(!b.zones[0].fill.is_empty(), "the zone did not fill");
    // DRC: no violations, nothing unrouted, schematic and board agree.
    d.click("kicad:pcb:tool:select");
    d.click("kicad:pcb:drc");
    d.click("kicad:dlg:run");
    let report = d.kicad(Frame::Pcb).session.drc.expect("DRC ran");
    assert!(report.violations.is_empty(), "{:#?}", report.violations);
    assert!(report.unconnected.is_empty(), "{:#?}", report.unconnected);
    assert!(report.parity.is_empty(), "{:#?}", report.parity);
    d.click("kicad:dlg:ok");
    // Fabrication outputs to the project's gerbers folder.
    d.click("kicad:pcb:plot");
    d.click("kicad:dlg:plot");
    d.click("kicad:dlg:drill");
    d.click("kicad:dlg:generate");
    d.click("kicad:dlg:cancel");
    for layer in [
        "F_Cu",
        "B_Cu",
        "F_Paste",
        "F_SilkS",
        "B_SilkS",
        "F_Mask",
        "B_Mask",
        "Edge_Cuts",
    ] {
        let text = d.file(&format!("{dir}/gerbers/rcfilter-{layer}.gbr"));
        let stats = cw_eda::gerber::parse(&text).unwrap_or_else(|e| panic!("{layer}: {e}"));
        if layer == "F_Cu" {
            assert!(stats.flashes >= 7 && stats.draws >= 5, "{stats:?}");
        }
        if layer == "B_Cu" {
            assert!(stats.regions > 0, "the zone is missing from B.Cu");
        }
    }
    let drill =
        cw_eda::gerber::parse_drill(&d.file(&format!("{dir}/gerbers/rcfilter.drl"))).unwrap();
    assert_eq!(drill.holes, 7);
    // And the board saves as a KiCad file that reads back as the same board.
    d.key("Ctrl+s");
    let (on_disk, _) =
        cw_eda::files::read_board(&d.file(&format!("{dir}/rcfilter.kicad_pcb"))).unwrap();
    let mut live = d.kicad(Frame::Pcb).session.board;
    live.rules = on_disk.rules.clone();
    assert_eq!(on_disk, live);
    let listing = d.kicad(Frame::ProjectManager).session.listing;
    assert!(listing.contains(&"gerbers/".to_owned()), "{listing:?}");

    // ---- Any angle ------------------------------------------------------------------
    // R1 turned 45° with Ctrl+R: its second pad is flashed 10.16 mm from the first
    // along the turned axis, up and to the right on the board.
    d.focus(Frame::Pcb);
    let r1 = pad("R1", "1");
    d.click_pcb(r1.x + 3 * MM, r1.y + MM);
    d.key("Ctrl+r");
    let b = d.kicad(Frame::Pcb).session.board;
    let f = b.footprints.iter().find(|f| f.reference == "R1").unwrap();
    assert_eq!(f.angle, 450, "Ctrl+R turns by 45°");
    d.click("kicad:pcb:plot");
    d.click("kicad:dlg:plot");
    d.click("kicad:dlg:cancel");
    let text = d.file(&format!("{dir}/gerbers/rcfilter-F_Cu.gbr"));
    cw_eda::gerber::parse(&text).unwrap();
    let pos = |n: &str| f.pad_pos(f.pads.iter().find(|p| p.number == n).unwrap());
    let (p1, p2) = (pos("1"), pos("2"));
    // 10.16 mm · cos 45° = 7.184 mm, to the nanometre.
    assert_eq!((p2.x - p1.x, p2.y - p1.y), (7_184_205, -7_184_205));
    for p in [p1, p2] {
        let flash = format!("X{}Y{}D03*", p.x, -p.y);
        assert!(text.contains(&flash), "no pad flashed at {flash}");
    }
}

#[test]
fn a_saved_project_reopens_from_the_project_manager() {
    let mut d = desk("carol-ubuntu", "virtual-ubuntu-24", "/home/carol");
    let dir = draw_rc(&mut d);
    // A fresh KiCad finds the project and reads it back.
    for id in d
        .windows()
        .into_iter()
        .map(|(id, _)| id)
        .collect::<Vec<_>>()
    {
        d.act("application.v1", "close", json!({ "window": id }));
    }
    d.act("application.v1", "launch", json!({"kind": "kicad"}));
    d.maximize(Frame::ProjectManager);
    d.click("kicad:pm:open");
    let found = d.kicad(Frame::ProjectManager).session.found;
    let i = found
        .iter()
        .position(|p| *p == format!("{dir}/rcfilter.kicad_pro"))
        .unwrap_or_else(|| panic!("{found:?}"));
    d.click(&format!("kicad:dlg:project:{i}"));
    d.click("kicad:dlg:ok");
    let k = d.kicad(Frame::ProjectManager);
    assert!(k.session.problem.is_none(), "{:?}", k.session.problem);
    assert_eq!(k.session.schematic.symbols.len(), 8);
    assert!(
        k.session.sim.command.is_empty(),
        "no analysis was set in this project"
    );
    assert!(k.session.listing.iter().any(|e| e == "rcfilter.kicad_sch"));
}

#[test]
fn the_three_desktops_have_kicad_and_the_phones_do_not() {
    for (machine, profile) in [
        ("alice-mac", "virtual-macos-golden-gate"),
        ("bob-windows", "virtual-windows-11"),
        ("carol-ubuntu", "virtual-ubuntu-24"),
    ] {
        let mut d = desk(machine, profile, "");
        d.act("application.v1", "launch", json!({"kind": "kicad"}));
        let scene = d.scene();
        assert!(
            scene.nodes.iter().any(|n| n
                .interaction
                .as_deref()
                .is_some_and(|i| i.ends_with(":content:kicad:pm:new"))),
            "{machine}: the project manager did not paint"
        );
    }
    // The browser demo's world installs it on desktops only.
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/browser/world-definition.js"
    ))
    .unwrap();
    let json = text
        .split_once("export default ")
        .unwrap()
        .1
        .trim()
        .trim_end_matches(';');
    let def: Value = serde_json::from_str(json).unwrap();
    for c in def["computers"].as_array().unwrap() {
        let has = c["installed_apps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "kicad");
        let desktop = [
            "virtual-macos-golden-gate",
            "virtual-windows-11",
            "virtual-ubuntu-24",
        ]
        .contains(&c["profile"].as_str().unwrap());
        assert_eq!(has, desktop, "{}", c["id"]);
    }
}

#[test]
fn the_library_editors_make_a_part_the_schematic_board_and_3d_viewer_use() {
    const MM: i64 = 1_000_000;
    let mut d = desk("carol-ubuntu", "virtual-ubuntu-24", "/home/carol");
    d.act("application.v1", "launch", json!({"kind": "kicad"}));
    d.maximize(Frame::ProjectManager);
    d.click("kicad:pm:new");
    d.replace_field("name", "parts");
    d.click("kicad:dlg:ok");
    let dir = format!("{}/Documents/KiCad/parts", d.home);

    // ---- Symbol Editor: an inverter with four pins -----------------------------------
    d.click("kicad:pm:launch:symed");
    d.maximize(Frame::SymbolEditor);
    d.click("kicad:symed:new-lib");
    d.text("mylib");
    d.click("kicad:dlg:ok");
    let table = d.file(&format!("{dir}/sym-lib-table"));
    assert!(table.contains("mylib.kicad_sym"), "{table}");
    d.click("kicad:symed:new-symbol");
    d.text("INV");
    d.click("kicad:dlg:ok");
    d.click("kicad:symed:tool:rect");
    let (x, y) = d.symed(-200, 200);
    d.pointer("click", x, y);
    let (x, y) = d.symed(200, -200);
    d.pointer("click", x, y);
    d.click("kicad:symed:tool:pin");
    for (at, name, number, orient, kind) in [
        ((-500, 0), "A", "1", "0", "input"),
        ((500, 0), "Y", "2", "180", "output"),
        ((0, 500), "VCC", "3", "270", "power_in"),
        ((0, -500), "GND", "4", "90", "power_in"),
    ] {
        let (x, y) = d.symed(at.0, at.1);
        d.pointer("click", x, y);
        d.replace_field("Name", name);
        d.replace_field("Number", number);
        d.replace_field("Length (mils)", "300");
        d.click(&format!("kicad:dlg:kind:{kind}"));
        d.click(&format!("kicad:dlg:orient:{orient}"));
        d.click("kicad:dlg:ok");
    }
    d.click("kicad:symed:properties");
    d.replace_field("Footprint", "mylib:SOT");
    d.click("kicad:dlg:model:NOT");
    d.click("kicad:dlg:ok");
    d.key("Ctrl+s");
    let k = d.kicad(Frame::SymbolEditor);
    let sym = k.session.sym_libs[0].symbols[0].clone();
    assert_eq!(sym.lib_id, "mylib:INV");
    assert_eq!(sym.pins.len(), 4);
    let a = sym.pin("1").unwrap();
    assert_eq!(
        (a.at, a.angle, a.length, a.name.as_str()),
        ((-500, 0), 0, 300, "A")
    );
    assert_eq!(
        sym.pin("4").unwrap().kind,
        cw_eda::symbols::PinType::PowerIn
    );
    assert_eq!(sym.graphics.len(), 1, "the body rectangle");
    let text = d.file(&format!("{dir}/mylib.kicad_sym"));
    assert!(text.starts_with("(kicad_symbol_lib"), "{text}");
    let back = cw_eda::files::read_symbol_lib(&text, "mylib").unwrap();
    assert_eq!(back, vec![sym.clone()], "the library does not read back");

    // ---- Footprint Editor: four SMD pads, a body and a courtyard ---------------------
    d.focus(Frame::ProjectManager);
    d.click("kicad:pm:launch:fped");
    d.maximize(Frame::FootprintEditor);
    d.click("kicad:fped:new-lib");
    d.text("mylib");
    d.click("kicad:dlg:ok");
    d.click("kicad:fped:new-footprint");
    d.text("SOT");
    d.click("kicad:dlg:ok");
    d.click("kicad:fped:tool:pad");
    for at in [(-2 * MM, 0), (2 * MM, 0), (0, -2 * MM), (0, 2 * MM)] {
        let (x, y) = d.fped(at.0, at.1);
        d.pointer("click", x, y);
        d.replace_field("Size X (mm)", "1");
        d.replace_field("Size Y (mm)", "0.6");
        d.click("kicad:dlg:ok");
    }
    d.click("kicad:fped:tool:fab");
    for at in [(-MM, -MM), (MM, MM)] {
        let (x, y) = d.fped(at.0, at.1);
        d.pointer("click", x, y);
    }
    d.click("kicad:fped:fit-courtyard");
    d.click("kicad:fped:properties");
    d.replace_field("Body height (mm)", "1.2");
    d.click("kicad:dlg:ok");
    d.key("Ctrl+s");
    let fp = d.kicad(Frame::FootprintEditor).session.fp_libs[0].footprints[0].clone();
    assert_eq!(fp.id, "mylib:SOT");
    let mut pads: Vec<(String, (i64, i64))> =
        fp.pads.iter().map(|p| (p.number.clone(), p.at)).collect();
    pads.sort();
    assert_eq!(
        pads,
        vec![
            ("1".into(), (-2 * MM, 0)),
            ("2".into(), (2 * MM, 0)),
            ("3".into(), (0, -2 * MM)),
            ("4".into(), (0, 2 * MM)),
        ]
    );
    assert_eq!(fp.fab, ((-MM, -MM), (MM, MM)));
    assert_eq!(fp.height, 1_200_000);
    assert!(fp.courtyard.0 .0 <= -2_750_000 && fp.courtyard.1 .1 >= 2_550_000);
    let text = d.file(&format!("{dir}/mylib.pretty/SOT.kicad_mod"));
    assert_eq!(cw_eda::files::read_footprint(&text, "mylib").unwrap(), fp);
    assert!(d
        .file(&format!("{dir}/fp-lib-table"))
        .contains("mylib.pretty"));

    // ---- The schematic places the new symbol, and the board its footprint -------------
    d.focus(Frame::ProjectManager);
    d.click("kicad:pm:launch:sch");
    d.maximize(Frame::Schematic);
    d.place("symbol", "INV", "mylib:INV", (3000, 3000), false);
    d.key("Escape");
    let s = d.kicad(Frame::Schematic).session.schematic;
    let u1 = s.by_reference("U1").expect("U1 placed");
    assert_eq!(u1.lib_id, "mylib:INV");
    assert_eq!(
        u1.local.as_deref(),
        Some(&sym),
        "the definition travels with it"
    );
    d.click("kicad:sch:update-pcb");
    d.maximize(Frame::Pcb);
    d.click("kicad:dlg:apply");
    d.click("kicad:dlg:ok");
    let b = d.kicad(Frame::Pcb).session.board;
    assert_eq!(b.footprints.len(), 1);
    assert_eq!(b.footprints[0].fp_id, "mylib:SOT");
    assert_eq!(b.footprints[0].pads.len(), 4);
    assert_eq!(b.footprints[0].height, 1_200_000);

    // ---- 3D Viewer ------------------------------------------------------------------
    d.click("kicad:pcb:3d");
    d.maximize(Frame::Viewer3d);
    let top = d.view3d_pixels();
    assert_eq!(
        top,
        d.view3d_pixels(),
        "the same view painted twice differs"
    );
    let (b, _) = d.find("kicad:canvas:3d:", true).unwrap();
    let c = (b.x + b.width as i32 / 2, b.y + b.height as i32 / 2);
    d.drag(c, (c.0 + 60, c.1 - 40));
    let v = d.kicad(Frame::Viewer3d).ui.v3d;
    assert_eq!((v.yaw, v.pitch), (3300, 700), "the drag orbits");
    let turned = d.view3d_pixels();
    assert_ne!(top, turned);
    d.click("kicad:v3d:toggle:bodies");
    assert_ne!(
        turned,
        d.view3d_pixels(),
        "hiding the bodies changes nothing"
    );
    d.click("kicad:v3d:toggle:bodies");
    d.click("kicad:v3d:view:top");
    assert_eq!(
        top,
        d.view3d_pixels(),
        "the top view comes back pixel for pixel"
    );
    // The wheel zooms in about the pointer.
    let r = d.act(
        "pointer.v1",
        "wheel",
        json!({"x": c.0, "y": c.1, "width": W, "height": H, "delta_y": -120}),
    );
    assert_eq!(r["handled"], json!(true));
    let v = d.kicad(Frame::Viewer3d).ui.v3d;
    assert!(v.zoom_um > 0, "the wheel did not zoom");
    assert_ne!(top, d.view3d_pixels());
}

#[test]
fn a_hierarchical_sheet_carries_a_net_between_sheets() {
    let mut d = desk("alice-mac", "virtual-macos-golden-gate", "/Users/alice");
    d.act("application.v1", "launch", json!({"kind": "kicad"}));
    d.maximize(Frame::ProjectManager);
    d.click("kicad:pm:new");
    d.replace_field("name", "hier");
    d.click("kicad:dlg:ok");
    let dir = format!("{}/Documents/KiCad/hier", d.home);
    d.click("kicad:pm:launch:sch");
    d.maximize(Frame::Schematic);
    let (x, y) = d.sch(3500, 3000);
    d.pointer("move", x, y);
    d.key("F1");
    // A sheet, drawn corner to corner.
    d.click("kicad:sch:tool:sheet");
    d.click_sch(3500, 2000);
    d.click_sch(5500, 3500);
    d.click("kicad:dlg:ok");
    d.click("kicad:sch:tool:select");
    d.click_sch(4500, 2700);
    let (x, y) = d.sch(4500, 2700);
    d.double_click(x, y);
    assert_eq!(
        d.kicad(Frame::Schematic).ui.sch.path.len(),
        1,
        "entered the sheet"
    );
    // Inside: a resistor whose pin 1 carries the hierarchical label SIG.
    d.place("symbol", "resistor", "Device:R", (3000, 3000), false);
    d.key("Escape");
    d.click("kicad:sch:tool:hlabel");
    d.click_sch(3000, 2850);
    d.text("SIG");
    d.click("kicad:dlg:ok");
    d.key("Alt+Backspace");
    assert!(
        d.kicad(Frame::Schematic).ui.sch.path.is_empty(),
        "back on the root"
    );
    // The sheet pin offers the label; a root resistor is wired to it.
    d.click("kicad:sch:tool:sheetpin");
    d.click_sch(3500, 2500);
    d.click("kicad:dlg:ok");
    d.place("symbol", "resistor", "Device:R", (2000, 3000), false);
    d.key("Escape");
    d.click("kicad:sch:tool:wire");
    d.click_sch(2000, 2850);
    d.click_sch(2000, 2500);
    d.click_sch(3500, 2500);
    let (x, y) = d.sch(3500, 2500);
    d.double_click(x, y);
    let s = d.kicad(Frame::Schematic).session.schematic;
    assert_eq!(s.sheets.len(), 1);
    assert_eq!(s.sheets[0].pins.len(), 1);
    assert_eq!(s.sheets[0].pins[0].name, "SIG");
    let conn = cw_eda::connectivity::analyze(&s);
    let inner = conn.net_of_ref_pin("R1", "1").expect("R1.1 is on a net");
    let outer = conn.net_of_ref_pin("R2", "1").expect("R2.1 is on a net");
    assert_eq!(inner.name, outer.name, "the sheet pin joins the two sheets");
    assert_eq!(inner.pins.len(), 2);
    // ERC finds no hierarchy problem; the design saves as two files.
    d.click("kicad:sch:erc");
    d.click("kicad:dlg:run");
    let erc = d.kicad(Frame::Schematic).session.erc.expect("ERC ran");
    assert!(
        !erc.iter().any(|v| v.message.contains("ierarchical")),
        "{erc:#?}"
    );
    d.click("kicad:dlg:ok");
    d.key("Ctrl+s");
    let root = d.file(&format!("{dir}/hier.kicad_sch"));
    assert!(root.contains("(sheet") && root.contains("hier-sheet1.kicad_sch"));
    let child = d.file(&format!("{dir}/hier-sheet1.kicad_sch"));
    assert!(child.contains("(hierarchical_label \"SIG\""), "{child}");
    let net = cw_eda::netlist::kicad_netlist(&s, "hier.kicad_sch", "");
    assert!(net.contains("/Sheet1/"), "{net}");
}

#[test]
fn the_wheel_zooms_about_the_pointer_and_pans_with_shift_and_ctrl() {
    let mut d = desk("carol-ubuntu", "virtual-ubuntu-24", "/home/carol");
    draw_rc(&mut d);
    d.focus(Frame::Schematic);
    let wheel = |d: &mut Desk, dy: i32, modifiers: &[&str]| {
        let (b, ..) = d.canvas("sch");
        d.act(
            "pointer.v1",
            "wheel",
            json!({"x": b.x + b.width as i32 / 2, "y": b.y + b.height as i32 / 2,
                   "width": W, "height": H, "delta_y": dy, "modifiers": modifiers}),
        );
        d.canvas("sch")
    };
    let (_, _, _, z) = d.canvas("sch");
    // Wheel up zooms in about the pointer.
    let (_, zx, zy, zoomed) = wheel(&mut d, -120, &[]);
    assert!(zoomed > z, "zoom {z} -> {zoomed}");
    // Shift pans up and down, Ctrl left and right; neither zooms.
    let (_, sx, sy, sz) = wheel(&mut d, 120, &["shift"]);
    assert_eq!(sz, zoomed);
    assert_eq!(sx, zx, "shift panned sideways");
    assert_ne!(sy, zy, "shift did not pan vertically");
    let (_, cx, cy, cz) = wheel(&mut d, 120, &["ctrl"]);
    assert_eq!(cz, zoomed);
    assert_ne!(cx, sx, "ctrl did not pan horizontally");
    assert_eq!(cy, sy);
}
