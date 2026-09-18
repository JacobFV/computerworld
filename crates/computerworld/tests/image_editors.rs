//! The image editors end to end: pointer drags paint real pixels, filters and dialogs
//! run through the editors' own menus, files are encoded to PNG on the machine and
//! decode back to the same pixels, and Photos hands a picture to the platform's editor.
use computerworld::{reference_world, World};
use cw_applications::apps::imaging::Studio;
use cw_applications::{AppState, NativeApp};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use cw_scene::Rect;
use serde_json::{json, Value};

const W: u32 = 1280;
const H: u32 = 800;

struct Session {
    world: World,
    actor: String,
    machine: &'static str,
}

fn session(machine: &'static str, theme: &str, apps: &[&str]) -> Session {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ machine: theme });
    if let Some(c) = definition.computers.iter_mut().find(|c| c.id == machine) {
        c.installed_apps.extend(apps.iter().map(|a| a.to_string()));
    }
    let mut world = World::new(definition, 7).unwrap();
    let mut config = EnvironmentConfig::desktop("alice", machine);
    config.observations.push("pixels.v1".into());
    let actor = world.environment(config).unwrap();
    Session {
        world,
        actor,
        machine,
    }
}

impl Session {
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
        self.try_act(family, op, payload.clone())
            .unwrap_or_else(|e| panic!("{family} {op} {payload}: {e}"))
    }
    fn launch(&mut self, kind: &str, argument: &str) -> u64 {
        let v = self.act(
            "application.v1",
            "launch",
            json!({"kind": kind, "argument": argument}),
        );
        v["window"].as_u64().unwrap()
    }
    fn control(&mut self, window: u64, target: &str) {
        self.act(
            "application.v1",
            "shell",
            json!({"target": format!("window:{window}:content:{target}")}),
        );
    }
    fn pointer(&mut self, op: &str, (x, y): (i32, i32)) {
        self.act(
            "pointer.v1",
            op,
            json!({"x": x, "y": y, "width": W, "height": H}),
        );
    }
    fn drag(&mut self, points: &[(i32, i32)]) {
        self.pointer("down", points[0]);
        for p in &points[1..] {
            self.pointer("move", *p);
        }
        self.pointer("up", *points.last().unwrap());
    }
    fn key(&mut self, key: &str) {
        self.act("keyboard.v1", "key", json!({ "key": key }));
    }
    fn state(&self, window: u64) -> AppState {
        self.world
            .interfaces()
            .session(&self.actor)
            .unwrap()
            .machines[self.machine]
            .desktop
            .windows[&window]
            .state
            .clone()
    }
    fn studio(&self, window: u64) -> Studio {
        match self.state(window) {
            AppState::Native(NativeApp::Paint(a)) => *a.0,
            AppState::Native(NativeApp::Gimp(a)) => *a.0,
            AppState::Native(NativeApp::Pinta(a)) => *a.0,
            AppState::Native(NativeApp::Preview(a)) => *a.0,
            AppState::Native(NativeApp::Pixelmator(a)) => *a.0,
            AppState::Native(NativeApp::Sketchbook(a)) => *a.0,
            AppState::Native(NativeApp::Photos(p)) => *p.editing.expect("editing a photo"),
            other => panic!("not an image editor: {other:?}"),
        }
    }
    /// Where the canvas is on screen, and its drag target.
    fn canvas(&self) -> (Rect, String) {
        let scene = self.world.scene(&self.actor, W, H).unwrap();
        scene
            .nodes
            .iter()
            .rev()
            .find_map(|n| {
                let t = n.interaction.as_deref()?;
                t.contains(":canvas:")
                    .then(|| (n.transform.bounds(n.bounds), t.to_owned()))
            })
            .expect("a canvas on screen")
    }
    /// Screen position of image pixel `(x, y)`'s centre.
    fn at(&self, window: u64, (x, y): (i32, i32)) -> (i32, i32) {
        let (r, _) = self.canvas();
        let s = self.studio(window);
        let z = s.effective_zoom(r.width, r.height) as i32;
        let (ox, oy) = s.origin(r.width, r.height);
        (
            r.x + ox + (x * z + z / 2) / 100,
            r.y + oy + (y * z + z / 2) / 100,
        )
    }
    fn file(&self, path: &str) -> Vec<u8> {
        self.world.runtime().read_file(self.machine, path).unwrap()
    }
    fn has_target(&self, target: &str) -> bool {
        let scene = self.world.scene(&self.actor, W, H).unwrap();
        scene.nodes.iter().any(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|t| t.ends_with(target))
        })
    }
}

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}
fn px(img: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.0 + x) * 4) as usize;
    [img.2[i], img.2[i + 1], img.2[i + 2], img.2[i + 3]]
}

#[test]
fn paint_draws_with_pointer_drags_saves_a_png_and_reopens_it() {
    let mut s = session("bob-windows", "virtual-windows-11", &["paint"]);
    let window = s.launch("paint", "");
    s.act("application.v1", "maximize", json!({}));
    let studio = s.studio(window);
    assert_eq!(studio.tool.id(), "pencil");
    let doc = studio.doc.as_ref().unwrap();
    assert_eq!((doc.width(), doc.height()), (960, 540));
    // A horizontal pencil stroke from (100, 200) to (300, 200) through a waypoint.
    let (a, b, c) = (
        s.at(window, (100, 200)),
        s.at(window, (200, 200)),
        s.at(window, (300, 200)),
    );
    s.drag(&[a, b, c]);
    let studio = s.studio(window);
    let doc = studio.doc.as_ref().unwrap();
    let mut black = 0;
    for x in 100..=300 {
        if doc.pixel(x, 200) == cw_raster::BLACK {
            black += 1;
        }
    }
    assert!(
        black >= 195,
        "the stroke is continuous: {black} of 201 pixels"
    );
    assert_eq!(doc.pixel(100, 190), cw_raster::WHITE);
    assert!(studio.modified);
    assert!(s.has_target("paint:undo"), "Undo is live after a stroke");

    // Ctrl+S on an untitled image asks for a name; the name is typed and Enter saves.
    s.key("Ctrl+s");
    for _ in 0.."Untitled.png".len() {
        s.key("Backspace");
    }
    s.act("keyboard.v1", "type", json!({"text": "sketch"}));
    s.key("Enter");
    let studio = s.studio(window);
    assert!(!studio.modified);
    assert!(studio.path.ends_with("sketch.png"), "{}", studio.path);
    let png = decode(&s.file(&studio.path));
    assert_eq!((png.0, png.1), (960, 540));
    assert_eq!(px(&png, 150, 200), [0, 0, 0, 255]);
    assert_eq!(px(&png, 150, 150), [255, 255, 255, 255]);

    // A second Paint window opens the file and shows exactly those pixels.
    let again = s.launch("paint", &studio.path);
    let reopened = s.studio(again);
    let doc = reopened.doc.as_ref().expect("the saved file decoded");
    for (x, y) in [(150, 200), (150, 150), (0, 0), (959, 539)] {
        assert_eq!(doc.pixel(x, y), px(&png, x as u32, y as u32));
    }
}

#[test]
fn gimp_runs_filters_from_its_menus_exports_and_pinta_reopens_and_pastes() {
    let mut s = session("carol-ubuntu", "virtual-ubuntu-24", &["gimp", "pinta"]);
    let gimp = s.launch("gimp", "");
    s.act("application.v1", "maximize", json!({}));
    assert!(s.studio(gimp).doc.is_none(), "GIMP starts with no image");
    // File ▸ New… through the menu bar.
    s.control(gimp, "gimp:menu:file");
    assert!(s.has_target("gimp:dialog:new-image"));
    s.control(gimp, "gimp:dialog:new-image");
    s.control(gimp, "gimp:set:width:64");
    s.control(gimp, "gimp:set:height:48");
    s.control(gimp, "gimp:apply");
    // A hard 5 px brush line across the middle.
    s.control(gimp, "gimp:tool:brush");
    s.control(gimp, "gimp:set:size:5");
    let (a, b) = (s.at(gimp, (8, 24)), s.at(gimp, (56, 24)));
    s.drag(&[a, b]);
    let before = s.studio(gimp).doc.unwrap();
    assert_eq!(before.pixel(32, 24), cw_raster::BLACK);
    assert_eq!(before.pixel(32, 29), cw_raster::WHITE);
    // Filters ▸ Blur ▸ Gaussian Blur…, radius 3.
    s.control(gimp, "gimp:menu:filters");
    s.control(gimp, "gimp:dialog:gaussian-blur");
    s.control(gimp, "gimp:set:radius:3");
    s.control(gimp, "gimp:apply");
    let blurred = s.studio(gimp).doc.unwrap();
    let edge = blurred.pixel(32, 28);
    assert!(
        edge[0] > 0 && edge[0] < 255,
        "blur softened the edge: {edge:?}"
    );
    assert_eq!(edge[0], edge[1]);
    assert_eq!(blurred.undo_label(), Some("Gaussian Blur"));
    // Colors ▸ Invert.
    s.control(gimp, "gimp:menu:colors");
    s.control(gimp, "gimp:action:invert");
    let inverted = s.studio(gimp).doc.unwrap();
    assert_eq!(inverted.pixel(0, 0), cw_raster::BLACK);
    // File ▸ Export As…
    s.control(gimp, "gimp:menu:file");
    s.control(gimp, "gimp:save-as");
    for _ in 0.."Untitled.png".len() {
        s.key("Backspace");
    }
    s.act("keyboard.v1", "type", json!({"text": "export"}));
    s.control(gimp, "gimp:save-confirm");
    let path = s.studio(gimp).path;
    assert!(path.ends_with("Pictures/export.png"), "{path}");
    let png = decode(&s.file(&path));
    assert_eq!((png.0, png.1), (64, 48));
    for (x, y) in [(0, 0), (32, 24), (32, 28)] {
        assert_eq!(px(&png, x, y), inverted.pixel(x as i32, y as i32));
    }

    // Copy everything in GIMP, then open the export in Pinta and paste as a layer.
    s.control(gimp, "gimp:select-all");
    s.control(gimp, "gimp:copy");
    let pinta = s.launch("pinta", "");
    s.control(pinta, "pinta:open");
    s.control(pinta, "pinta:open:export.png");
    let opened = s.studio(pinta).doc.unwrap();
    assert_eq!((opened.width(), opened.height()), (64, 48));
    assert_eq!(opened.pixel(32, 28), inverted.pixel(32, 28));
    s.control(pinta, "pinta:paste");
    let pasted = s.studio(pinta).doc.unwrap();
    assert_eq!(pasted.layers().len(), 2);
    assert_eq!(pasted.layers()[1].name, "Pasted Layer");
    assert_eq!(
        pasted.layers()[1].canvas.get(32, 24),
        inverted.pixel(32, 24)
    );
}

#[test]
fn the_text_tool_stamps_the_renderers_glyphs() {
    let mut s = session("carol-ubuntu", "virtual-ubuntu-24", &["pinta"]);
    let pinta = s.launch("pinta", "");
    s.act("application.v1", "maximize", json!({}));
    s.control(pinta, "pinta:tool:text");
    s.control(pinta, "pinta:set:font-size:40");
    let at = s.at(pinta, (100, 100));
    s.pointer("click", at);
    s.act("keyboard.v1", "type", json!({"text": "Hello"}));
    s.key("Enter");
    let doc = s.studio(pinta).doc.unwrap();
    let mut inked = 0;
    for y in 100..170 {
        for x in 100..260 {
            if doc.pixel(x, y)[0] < 128 {
                inked += 1;
            }
        }
    }
    assert!(inked > 300, "glyph pixels stamped: {inked}");
    assert_eq!(doc.pixel(50, 50), cw_raster::WHITE);
    assert_eq!(doc.undo_label(), Some("Text"));
}

#[test]
fn photos_hands_a_picture_to_the_platforms_editor() {
    let mut s = session(
        "alice-mac",
        "virtual-macos-golden-gate",
        &["photos", "preview"],
    );
    s.launch("terminal", "");
    s.act(
        "application.v1",
        "shell",
        json!({"target": "shell:screenshot"}),
    );
    let photos = s.launch("photos", "Pictures");
    s.control(photos, "photos:open:screen-0.png");
    assert!(s.has_target("photos:edit-with:preview"));
    s.control(photos, "photos:edit-with:preview");
    let preview = s.world.interfaces().session(&s.actor).unwrap().machines[s.machine]
        .desktop
        .focused
        .unwrap();
    let doc = s.studio(preview).doc.expect("Preview decoded the photo");
    assert_eq!((doc.width(), doc.height()), (1280, 800));

    // With no editor installed the button is painted but announced disabled.
    let mut bare = session("alice-mac", "virtual-macos-golden-gate", &["photos"]);
    bare.launch("terminal", "");
    bare.act(
        "application.v1",
        "shell",
        json!({"target": "shell:screenshot"}),
    );
    let photos = bare.launch("photos", "Pictures");
    bare.control(photos, "photos:open:screen-0.png");
    assert!(!bare.has_target("photos:edit-with:preview"));
    assert!(bare
        .try_act(
            "application.v1",
            "shell",
            json!({"target": format!("window:{photos}:content:photos:edit-with:preview")}),
        )
        .is_err());
}

#[test]
fn phone_photo_editors_bake_their_look_into_the_saved_file() {
    // iOS: edit in place, Done saves over the PNG.
    let mut s = session("alice-mac", "virtual-ios-18", &["photos"]);
    s.launch("terminal", "");
    s.act(
        "application.v1",
        "shell",
        json!({"target": "shell:screenshot"}),
    );
    let photos = s.launch("photos", "Pictures");
    s.control(photos, "photos:open:screen-0.png");
    s.control(photos, "photos:begin-edit:ios");
    let editor = s.studio(photos);
    assert_eq!(editor.doc.as_ref().map(|d| d.width()), Some(1280));
    s.control(photos, "photos:edit:focus:saturation");
    s.control(photos, "photos:edit:set:saturation:-100");
    let path = s.studio(photos).path;
    s.control(photos, "photos:edit:look:done");
    let state = match s.state(photos) {
        AppState::Native(NativeApp::Photos(p)) => p,
        _ => unreachable!(),
    };
    assert!(state.editing.is_none(), "Done leaves edit mode");
    let png = decode(&s.file(&path));
    for (x, y) in [(640, 400), (100, 700), (1200, 20)] {
        let p = px(&png, x, y);
        assert_eq!(
            (p[0], p[1]),
            (p[1], p[2]),
            "saturation -100 is grey at {x},{y}: {p:?}"
        );
    }

    // Google Photos: Save copy leaves the original and adds a copy beside it.
    let mut g = session("alice-mac", "virtual-android-12", &["photos"]);
    g.launch("terminal", "");
    g.act(
        "application.v1",
        "shell",
        json!({"target": "shell:screenshot"}),
    );
    let photos = g.launch("photos", "Pictures");
    g.control(photos, "photos:open:screen-0.png");
    let original = g.file("Pictures/screen-0.png");
    g.control(photos, "photos:begin-edit:android");
    g.control(photos, "photos:edit:tab:filters");
    g.control(photos, "photos:edit:preset:onyx");
    g.control(photos, "photos:edit:look:done");
    assert_eq!(
        g.file("Pictures/screen-0.png"),
        original,
        "the original is untouched"
    );
    let copy = decode(&g.file("Pictures/screen-0-edited.png"));
    let p = px(&copy, 640, 400);
    assert_eq!((p[0], p[1]), (p[1], p[2]), "Onyx is monochrome: {p:?}");
    let state = match g.state(photos) {
        AppState::Native(NativeApp::Photos(p)) => p,
        _ => unreachable!(),
    };
    assert_eq!(state.open.as_deref(), Some("screen-0-edited.png"));
    assert!(state.entries.iter().any(|e| e == "screen-0-edited.png"));
}

#[test]
fn a_finger_draws_on_a_phone_canvas_without_triggering_shell_gestures() {
    let mut s = session("alice-mac", "virtual-android-12", &["sketchbook"]);
    let sketch = s.launch("sketchbook", "");
    // A long upward drag from near the bottom edge would be "go home" anywhere else.
    let start = s.at(sketch, (360, 1250));
    assert!(
        start.1 > H as i32 - 90,
        "the stroke starts in the home-gesture zone"
    );
    let mid = s.at(sketch, (360, 700));
    let end = s.at(sketch, (360, 40));
    s.drag(&[start, mid, end]);
    let studio = s.studio(sketch);
    assert!(
        studio.doc.as_ref().unwrap().can_undo(),
        "the stroke was drawn"
    );
    let desktop = &s.world.interfaces().session(&s.actor).unwrap().machines[s.machine].desktop;
    assert_eq!(
        desktop.focused,
        Some(sketch),
        "no gesture took the app away"
    );
}

#[test]
fn jpeg_photos_open_in_the_editors() {
    let mut s = session("carol-ubuntu", "virtual-ubuntu-24", &["gimp"]);
    let jpeg = include_bytes!("../../render/assets/wallpapers/macos.jpg");
    let actor = s.actor.clone();
    s.world
        .runtime_mut()
        .write_file("carol-ubuntu", &actor, "Pictures/wall.jpg", jpeg)
        .unwrap();
    let gimp = s.launch("gimp", "Pictures/wall.jpg");
    let studio = s.studio(gimp);
    let doc = studio.doc.clone().expect("the JPEG decoded");
    assert_eq!((doc.width(), doc.height()), (1586, 992));
    assert_eq!(doc.pixel(10, 10)[3], 255);
    // Saving a JPEG never writes PNG bytes over it: Overwrite is not offered.
    assert!(studio.save_target().is_none());
}

#[test]
fn the_same_drags_give_the_same_world() {
    let run = || {
        let mut s = session("bob-windows", "virtual-windows-11", &["paint"]);
        let window = s.launch("paint", "");
        s.control(window, "paint:tool:brush");
        s.control(window, "paint:set:size:12");
        let pts: Vec<_> = [(50, 50), (400, 90), (700, 300)]
            .iter()
            .map(|p| s.at(window, *p))
            .collect();
        s.drag(&pts);
        s.control(window, "paint:shape:star");
        let (a, b) = (s.at(window, (500, 100)), s.at(window, (650, 250)));
        s.drag(&[a, b]);
        let doc = s.studio(window).doc.unwrap();
        (doc.composite().hash(), s.world.state_hash().unwrap())
    };
    assert_eq!(run(), run());
}
