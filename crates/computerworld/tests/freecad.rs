//! FreeCAD, driven the way a person drives it: pointer clicks and drags on the rendered
//! screen and typed values. Assertions are about the model and the machine — the solid's
//! volume, the file on disk — not about what the application claims.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, WorldDefinition};
use serde_json::{json, Value};

const W: u32 = 1280;
const H: u32 = 800;

fn definition() -> WorldDefinition {
    let mut d = reference_world();
    d.metadata["desktop_themes"] = json!({
        "alice-mac": "virtual-macos-golden-gate",
        "bob-windows": "virtual-windows-11",
        "carol-ubuntu": "virtual-ubuntu-24",
    });
    d
}

struct Desk {
    world: World,
    actor: String,
    machine: &'static str,
}
impl Desk {
    fn new(machine: &'static str, user: &str) -> Self {
        let mut world = World::new(definition(), 5).unwrap();
        let mut config = EnvironmentConfig::desktop(user, machine);
        config.observations.push("pixels.v1".into());
        let actor = world.environment(config).unwrap();
        Self {
            world,
            actor,
            machine,
        }
    }
    fn try_act(&mut self, family: &str, op: &str, payload: Value) -> Result<Value, String> {
        let result = self
            .world
            .step(
                &self.actor,
                vec![ActionEnvelope::new(family, op, self.machine, payload)],
            )
            .unwrap();
        let outcome = &result.outcomes[0];
        if outcome.success {
            Ok(outcome.value.clone())
        } else {
            Err(format!("{:?}", outcome.error))
        }
    }
    fn act(&mut self, family: &str, op: &str, payload: Value) -> Value {
        self.try_act(family, op, payload)
            .unwrap_or_else(|e| panic!("{family}.{op} failed: {e}"))
    }
    fn launch(&mut self) {
        self.act("application.v1", "launch", json!({"kind": "freecad"}));
        self.act(
            "application.v1",
            "shell",
            json!({"target": "shell:maximize"}),
        );
    }
    fn key(&mut self, key: &str) {
        self.act("keyboard.v1", "key", json!({"key": key}));
    }
    fn typed(&mut self, text: &str) {
        self.act("keyboard.v1", "type", json!({"text": text}));
    }
    /// Screen bounds and full target of the topmost control whose target (after
    /// `:content:`) starts with `prefix`.
    fn locate(&self, prefix: &str) -> Option<(String, cw_scene::Rect)> {
        let scene = self.world.scene(&self.actor, W, H).unwrap();
        let needle = format!(":content:{prefix}");
        let found: Vec<(String, cw_scene::Rect)> = scene
            .nodes
            .iter()
            .rev()
            .filter_map(|n| {
                let i = n.interaction.as_deref()?;
                let at = i.find(&needle)?;
                Some((i[at + 9..].to_owned(), n.transform.bounds(n.bounds)))
            })
            .collect();
        // An exact target beats a longer one that merely starts the same way.
        found
            .iter()
            .find(|(t, _)| t == prefix)
            .or(found.first())
            .cloned()
    }
    fn at(&self, prefix: &str) -> cw_scene::Rect {
        self.locate(prefix)
            .unwrap_or_else(|| panic!("no control starting {prefix}"))
            .1
    }
    fn pointer(&mut self, op: &str, x: i32, y: i32, button: u64) -> Value {
        self.act(
            "pointer.v1",
            op,
            json!({"x": x, "y": y, "width": W, "height": H, "button": button}),
        )
    }
    fn click_at(&mut self, x: i32, y: i32) {
        self.pointer("down", x, y, 0);
        let payload = json!({"x": x, "y": y, "width": W, "height": H, "button": 0});
        if let Err(e) = self.try_act("pointer.v1", "up", payload) {
            let s = self.state();
            panic!(
                "clicking ({x}, {y}) failed: {e}; status {:?} dialog {:?} press {:?} report {:?}",
                s["status"], s["dialog"], s["press"], s["report"]
            );
        }
    }
    /// Click the middle of the control whose target starts with `prefix`.
    fn click(&mut self, prefix: &str) {
        let r = self.at(prefix);
        let (x, y) = (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
        self.pointer("down", x, y, 0);
        let payload = json!({"x": x, "y": y, "width": W, "height": H, "button": 0});
        if let Err(e) = self.try_act("pointer.v1", "up", payload) {
            panic!(
                "clicking {prefix} failed: {e}; status {:?}",
                self.state()["status"]
            );
        }
    }
    /// Click the middle of the text `label` where it is painted.
    fn click_text(&mut self, label: &str) {
        let scene = self.world.scene(&self.actor, W, H).unwrap();
        let r = scene
            .nodes
            .iter()
            .rev()
            .find(|n| n.painted_text() == Some(label))
            .map(|n| n.transform.bounds(n.bounds))
            .unwrap_or_else(|| panic!("no text {label}"));
        self.click_at(r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
    }
    fn drag(&mut self, from: (i32, i32), to: (i32, i32), button: u64) {
        self.pointer("down", from.0, from.1, button);
        for k in 1..=6 {
            let x = from.0 + (to.0 - from.0) * k / 6;
            let y = from.1 + (to.1 - from.1) * k / 6;
            self.pointer("move", x, y, button);
        }
        self.pointer("up", to.0, to.1, button);
    }
    /// The FreeCAD window's state as the snapshot holds it.
    fn state(&self) -> Value {
        let session = self.world.interfaces().session(&self.actor).unwrap();
        let desktop = serde_json::to_value(&session.machines[self.machine].desktop).unwrap();
        desktop["windows"]
            .as_object()
            .unwrap()
            .values()
            .find(|w| w["state"]["app"] == "freecad")
            .expect("a FreeCAD window")["state"]
            .clone()
    }
    fn view(&self) -> cw_scene::Rect {
        self.at("freecad:view:")
    }
    fn semantic(&mut self) -> String {
        let obs = self.world.observe(&self.actor).unwrap();
        serde_json::to_string(&obs).unwrap()
    }
    /// Where a model point appears on screen, through the camera the view was drawn
    /// with — what a person sees, located the way they would locate it.
    fn project(&self, p: cw_cad::V3) -> (i32, i32) {
        let cam: cw_cad::view::Camera =
            serde_json::from_value(self.state()["camera"].clone()).unwrap();
        let v = self.view();
        let (x, y, _) = cam
            .project(p, v.width, v.height)
            .expect("in front of the camera");
        (v.x + x.round() as i32, v.y + y.round() as i32)
    }
    /// The document the application holds, recomputed independently: ground truth.
    fn model(&self) -> (cw_cad::document::Document, cw_cad::document::Model) {
        let mut doc: cw_cad::document::Document =
            serde_json::from_value(self.state()["doc"].clone()).unwrap();
        let model = cw_cad::document::recompute(&mut doc);
        (doc, model)
    }
    fn body_volume(&self) -> f64 {
        let (doc, model) = self.model();
        let body = doc.bodies()[0].to_owned();
        model.body_shape[&body].mesh.volume()
    }
    fn solver(&self) -> String {
        let r: cw_cad::sketch::SolveReport =
            serde_json::from_value(self.state()["task"]["report"].clone()).unwrap();
        r.message()
    }
    /// Type into whatever field has focus, replacing it, and press Enter.
    fn enter(&mut self, text: &str) {
        self.typed(text);
        self.key("Enter");
    }
    /// A real double click on the control whose target starts with `prefix`: the host
    /// sends the click, then the double click.
    fn double_click(&mut self, prefix: &str) {
        let r = self.at(prefix);
        let (x, y) = (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
        for op in ["click", "double_click"] {
            let payload = json!({"x": x, "y": y, "width": W, "height": H});
            if let Err(e) = self.try_act("pointer.v1", op, payload) {
                panic!(
                    "{op} on {prefix} failed: {e}; status {:?}",
                    self.state()["status"]
                );
            }
        }
    }
    fn dialog(&self) -> Value {
        self.state()["dialog"].clone()
    }
    fn folder(&self) -> String {
        self.dialog()["folder"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }
    fn exists(&mut self, path: &str) -> bool {
        self.try_act("filesystem.v1", "read", json!({"path": path}))
            .is_ok()
    }
    fn read_file(&mut self, path: &str) -> Vec<u8> {
        let v = self.act("filesystem.v1", "read", json!({"path": path}));
        v["bytes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect()
    }
}

/// Sketch a rectangle, constrain it fully, pad it, pocket a hole through it from a
/// sketch on its top face, measure the body and export it as STL — all by pointer and
/// keyboard — then read the file back from the machine and check the solid it holds.
#[test]
fn sketch_constrain_pad_pocket_measure_and_export() {
    use cw_cad::v3;
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    d.click("freecad:cmd:PartDesign_NewSketch");
    d.click("freecad:task:ok");
    assert!(d.semantic().contains("Empty sketch"));
    // A rectangle from the origin (the click snaps to the root point).
    d.click("freecad:cmd:Sketcher_CreateRectangle");
    let o = d.project(v3(0.0, 0.0, 0.0));
    d.click_at(o.0, o.1);
    let far = d.project(v3(36.0, 23.0, 0.0));
    d.click_at(far.0, far.1);
    d.key("Escape");
    // FreeCAD's rectangle: four lines, coincident corners, two horizontal, two vertical;
    // with its corner on the origin, width and height are all that is left.
    assert_eq!(d.solver(), "Under constrained: 2 DoFs");
    // Width: select the bottom edge in the Elements list, constrain horizontal distance.
    d.click("freecad:sk:element:0");
    d.click("freecad:cmd:Sketcher_ConstrainDistanceX");
    assert!(
        d.locate("freecad:dialog:ok").is_some(),
        "the Insert length dialog opens"
    );
    d.enter("40");
    d.click("freecad:sk:element:1");
    d.click("freecad:cmd:Sketcher_ConstrainDistanceY");
    d.enter("20 mm");
    assert_eq!(d.solver(), "Fully constrained");
    d.click("freecad:sk:close");

    // Pad it 10 mm, typing the length into the task panel.
    d.click("freecad:cmd:PartDesign_Pad");
    d.click("freecad:field:task:Length");
    d.enter("10");
    d.click("freecad:task:ok");
    assert!(
        (d.body_volume() - 8000.0).abs() < 1e-6,
        "{}",
        d.body_volume()
    );

    // A hole: pick the top face in the 3D view, sketch a circle on it, pocket through all.
    d.click("freecad:cmd:Std_ViewIsometric");
    d.click("freecad:cmd:Std_ViewFitAll");
    let top = d.project(v3(20.0, 10.0, 10.0));
    d.click_at(top.0, top.1);
    let sel = d.state()["selection"][0]["sub"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(sel.starts_with("Face"), "picked {sel}");
    d.click("freecad:cmd:PartDesign_NewSketch");
    let c = d.project(v3(20.0, 10.0, 10.0));
    let rim = d.project(v3(24.0, 10.0, 10.0));
    d.click("freecad:cmd:Sketcher_CreateCircle");
    d.click_at(c.0, c.1);
    d.click_at(rim.0, rim.1);
    d.key("Escape");
    d.click("freecad:sk:element:0");
    d.click("freecad:cmd:Sketcher_ConstrainRadius");
    d.enter("5");
    d.click("freecad:sk:close");
    d.click("freecad:cmd:PartDesign_Pocket");
    d.click("freecad:choice:open:task/Type");
    d.click("freecad:choice:task/Type:Through all");
    let pocket = d
        .model()
        .0
        .get("Pocket")
        .map(|o| serde_json::to_value(&o.feature).unwrap())
        .unwrap();
    assert_eq!(pocket["Type"], "ThroughAll", "{pocket}");
    d.click("freecad:task:ok");
    let n = cw_cad::sketch::profile::SEGMENTS as f64;
    let hole = 0.5 * n * 25.0 * (std::f64::consts::TAU / n).sin() * 10.0;
    assert!(
        (d.body_volume() - (8000.0 - hole)).abs() < 1e-6,
        "{}",
        d.body_volume()
    );
    let (doc, model) = d.model();
    let body = doc.bodies()[0].to_owned();
    assert!(model.body_shape[&body].mesh.is_watertight());

    // Measure the body: select it in the tree and open the Measure tool.
    d.click("freecad:tree:Body");
    d.click("freecad:cmd:Std_Measure");
    let page = d.semantic();
    assert!(page.contains("Body volume"), "{page}");
    d.click("freecad:task:ok");

    // Export it as STL into ~/Documents, then read the file back off the machine.
    d.click("freecad:tree:Body");
    d.click("freecad:menu:File");
    d.click("freecad:cmd:Std_Export");
    assert_eq!(d.state()["dialog"]["folder"], "/home/carol/Documents");
    d.click("freecad:file:ok");
    let bytes = d.read_file("/home/carol/Documents/Body.stl");
    assert_eq!(
        bytes.len(),
        84 + 50 * u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize
    );
    let mesh = cw_cad::io::read_stl(&bytes).unwrap();
    assert!(mesh.is_watertight());
    assert!(
        (mesh.volume() - (8000.0 - hole)).abs() < 0.05,
        "{}",
        mesh.volume()
    );
    // ASCII too, by choosing the type in the dialog.
    d.click("freecad:menu:File");
    d.click("freecad:cmd:Std_Export");
    d.click("freecad:choice:open:filetype");
    d.click("freecad:choice:filetype:1");
    d.click("freecad:file:ok");
    let text = String::from_utf8(d.read_file("/home/carol/Documents/Body.ast")).unwrap();
    assert!(text.starts_with("solid Body"));
    let ascii = cw_cad::io::read_stl(text.as_bytes()).unwrap();
    assert_eq!(ascii.volume(), model.body_shape[&body].mesh.volume());
}

/// Save to the machine, start a new document, open the saved one again; undo and redo.
#[test]
fn save_open_undo_and_redo() {
    let mut d = Desk::new("alice-mac", "alice");
    d.launch();
    d.click("freecad:cmd:PartDesign_NewSketch");
    d.click("freecad:task:ok");
    d.click("freecad:cmd:Sketcher_CreateCircle");
    let c = d.project(cw_cad::v3(0.0, 0.0, 0.0));
    let r = d.project(cw_cad::v3(8.0, 0.0, 0.0));
    d.click_at(c.0, c.1);
    d.click_at(r.0, r.1);
    d.key("Escape");
    d.click("freecad:sk:close");
    d.click("freecad:cmd:PartDesign_Pad");
    d.click("freecad:task:ok");
    let v = d.body_volume();
    assert!(v > 0.0);
    // Undo the pad, redo it.
    d.key("Ctrl+z");
    let (doc, _) = d.model();
    assert!(doc.get("Pad").is_none(), "undo removed the pad");
    d.key("Ctrl+y");
    assert_eq!(d.body_volume(), v);
    // Save: a new document asks for a name first.
    d.key("Ctrl+s");
    d.click("freecad:file:ok");
    assert_eq!(d.state()["modified"], false);
    let saved =
        String::from_utf8(d.read_file("/Users/alice/Documents/Unnamed.FCStd.json")).unwrap();
    assert!(saved.contains("\"TypeId\": \"PartDesign::Pad\""));
    d.click("freecad:cmd:Std_New");
    assert!(d.model().0.get("Pad").is_none());
    d.click("freecad:cmd:Std_Open");
    d.click("freecad:file:entry:Unnamed.FCStd.json");
    d.click("freecad:file:ok");
    assert_eq!(
        d.body_volume(),
        v,
        "the reopened document recomputes to the same solid"
    );
}

/// Orbit with a left drag, pan with a right drag, zoom with the wheel, and turn to a
/// standard view with the navigation cube.
#[test]
fn the_view_orbits_pans_zooms_and_answers_the_navigation_cube() {
    let mut d = Desk::new("bob-windows", "bob");
    d.launch();
    let cam = |d: &Desk| -> cw_cad::view::Camera {
        serde_json::from_value(d.state()["camera"].clone()).unwrap()
    };
    let v = d.view();
    let mid = (v.x + v.width as i32 / 2, v.y + v.height as i32 / 2);
    let before = cam(&d);
    d.drag(mid, (mid.0 + 120, mid.1 + 40), 0);
    let orbited = cam(&d);
    assert!(
        (orbited.right - before.right).len() > 0.1,
        "left drag orbits"
    );
    assert_eq!(orbited.target, before.target);
    d.drag(mid, (mid.0 - 80, mid.1), 2);
    let panned = cam(&d);
    assert!(
        (panned.target - orbited.target).len() > 1.0,
        "right drag pans"
    );
    assert_eq!(panned.right, orbited.right);
    let wheel = d.act(
        "pointer.v1",
        "wheel",
        json!({"x": mid.0, "y": mid.1, "width": W, "height": H, "delta_y": -240}),
    );
    assert_eq!(wheel["handled"], true);
    assert!(
        cam(&d).half_height < panned.half_height,
        "wheel up zooms in"
    );
    // In the isometric view FRONT is the cube's lower-left face.
    d.click("freecad:cmd:Std_ViewIsometric");
    d.click_text("FRONT");
    let f = cam(&d);
    assert!(
        (f.back() - cw_cad::v3(0.0, -1.0, 0.0)).len() < 1e-9,
        "{:?}",
        f.back()
    );
    // A wheel turn outside any application does nothing and says so.
    let outside = d.act(
        "pointer.v1",
        "wheel",
        json!({"x": 2, "y": 2, "width": W, "height": H, "delta_y": 120}),
    );
    assert_eq!(outside["handled"], false);
}

/// Each desktop's own file dialog, driven by pointer and keyboard: Save As goes
/// through the sidebar's standard places, the path bar (the Mac's folder pop-up),
/// history, and New Folder into a folder that really appears on the machine; the file
/// is written there; saving over it asks the platform's question; and the document
/// comes back through the Open dialog by double clicks.
#[test]
fn native_file_dialogs_save_and_open_on_every_desktop() {
    for (machine, user) in [
        ("alice-mac", "alice"),
        ("bob-windows", "bob"),
        ("carol-ubuntu", "carol"),
    ] {
        let mut d = Desk::new(machine, user);
        d.launch();
        let home = d.state()["home"].as_str().unwrap().to_owned();
        assert!(
            !home.is_empty(),
            "{machine}: the window knows the home folder"
        );
        let docs = format!("{home}/Documents");
        let desktop = format!("{home}/Desktop");

        d.key("Ctrl+Shift+S");
        assert_eq!(d.dialog()["purpose"], "save_as");
        assert_eq!(d.folder(), docs, "{machine}: Save As starts in Documents");
        // A sidebar place.
        d.click(&format!("freecad:file:place:{desktop}"));
        assert_eq!(d.folder(), desktop, "{machine}: sidebar Desktop");
        // The path bar (on the Mac, the folder pop-up) up to the home folder.
        if machine == "alice-mac" {
            d.click("freecad:choice:open:filepath");
            d.click(&format!("freecad:choice:filepath:{home}"));
        } else {
            d.click(&format!("freecad:file:crumb:{home}"));
        }
        assert_eq!(d.folder(), home, "{machine}: path bar");
        // History: Back to the Desktop, Forward home again (GTK's chooser has no
        // history buttons; Alt+Left and Alt+Right are its keys).
        if machine == "carol-ubuntu" {
            d.key("Alt+ArrowLeft");
        } else {
            d.click("freecad:file:back");
        }
        assert_eq!(d.folder(), desktop, "{machine}: back");
        if machine == "carol-ubuntu" {
            d.key("Alt+ArrowRight");
        } else {
            d.click("freecad:file:forward");
        }
        assert_eq!(d.folder(), home, "{machine}: forward");
        // Into Documents by a double click on its row.
        d.double_click("freecad:file:entry:Documents/");
        assert_eq!(d.folder(), docs, "{machine}: double click opens a folder");

        // New Folder, the platform's way.
        let parts = match machine {
            "bob-windows" => {
                d.click("freecad:file:new-folder");
                assert_eq!(d.dialog()["selected"], "New folder/");
                let listed = d.dialog()["entries"].clone();
                assert!(
                    listed
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|e| e == "New folder/"),
                    "the new folder is listed from the machine: {listed}"
                );
                d.double_click("freecad:file:entry:New folder/");
                format!("{docs}/New folder")
            }
            "alice-mac" => {
                d.click("freecad:file:folder-prompt:");
                d.typed("Parts");
                d.key("Enter");
                format!("{docs}/Parts")
            }
            _ => {
                d.click("freecad:file:folder-prompt:");
                d.typed("Parts");
                d.click("freecad:file:folder-create");
                format!("{docs}/Parts")
            }
        };
        assert_eq!(d.folder(), parts, "{machine}: into the new folder");
        assert_eq!(d.dialog()["entries"], json!([]), "a new folder is empty");
        // Name it and save with Return.
        d.click("freecad:field:file-name");
        d.typed("Bracket");
        d.key("Enter");
        let saved = format!("{parts}/Bracket.FCStd.json");
        assert!(d.state()["dialog"].is_null(), "{machine}: saved and closed");
        assert!(d.exists(&saved), "{machine}: {saved} is on the machine");
        assert_eq!(d.state()["path"], saved.as_str());

        // Save As over it asks first.
        d.key("Ctrl+Shift+S");
        assert_eq!(d.folder(), parts);
        d.key("Enter");
        assert_eq!(
            d.dialog()["confirm"],
            "Bracket.FCStd.json",
            "{machine}: replace question"
        );
        d.key("Enter");
        if machine == "carol-ubuntu" {
            // GTK's default response is Replace.
            assert!(d.state()["dialog"].is_null());
        } else {
            // The Mac's alert and Confirm Save As default to Cancel / No.
            assert!(
                d.dialog()["confirm"].is_null(),
                "{machine}: Return declined"
            );
            assert!(!d.state()["dialog"].is_null());
            d.key("Enter");
            d.click("freecad:file:replace");
            assert!(d.state()["dialog"].is_null(), "{machine}: replaced");
        }

        // A new document, then the saved one back through Open.
        d.click("freecad:cmd:Std_New");
        assert_eq!(d.state()["path"], "");
        d.key("Ctrl+o");
        assert_eq!(d.dialog()["purpose"], "open");
        assert_eq!(d.folder(), docs);
        let folder = parts.rsplit('/').next().unwrap().to_owned();
        d.double_click(&format!("freecad:file:entry:{folder}/"));
        d.double_click("freecad:file:entry:Bracket.FCStd.json");
        assert!(d.state()["dialog"].is_null(), "{machine}: opened");
        assert_eq!(d.state()["path"], saved.as_str());
        assert_eq!(d.state()["doc"]["Label"], "Bracket");
    }
}
