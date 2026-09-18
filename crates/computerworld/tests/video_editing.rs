//! The video editors end to end, driven the way a person drives them: media imported
//! from the machine's own movies folder through the import sheet, clips dragged from
//! the bin onto the timeline with the pointer, a transition and a title added, the
//! programme played with the world clock until a timecode, the monitor's pixels checked
//! against the engine's frame, and an export encoded over simulation steps that
//! re-imports and plays back identically.
use computerworld::{reference_world, World};
use cw_applications::AppState;
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use cw_scene::{Primitive, Rect};
use cw_video::{Compositor, Media, MediaKind, Source, TrackKind};
use serde_json::{json, Value};

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../worlds/company-2026/files/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

const SAMPLES: [&str; 5] = [
    "Countdown.apng",
    "Color Bars.apng",
    "Sunset.apng",
    "Countdown Beeps.wav",
    "Music Bed.wav",
];

struct Session {
    world: World,
    actor: String,
    machine: &'static str,
    w: u32,
    h: u32,
}

fn session(theme: &str, app: &str, folder: &str, (w, h): (u32, u32)) -> Session {
    let machine = "alice-mac";
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ machine: theme });
    let c = definition
        .computers
        .iter_mut()
        .find(|c| c.id == machine)
        .unwrap();
    c.installed_apps.push(app.into());
    for name in SAMPLES {
        c.initial_binary_files.insert(
            format!("{folder}/{name}"),
            cw_video::blob::encode(&sample(name)),
        );
    }
    let mut world = World::new(definition, 11).unwrap();
    let mut config = EnvironmentConfig::desktop("alice", machine);
    config.observations.push("pixels.v1".into());
    let actor = world.environment(config).unwrap();
    Session {
        world,
        actor,
        machine,
        w,
        h,
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
    fn launch(&mut self, kind: &str) -> u64 {
        let v = self.act(
            "application.v1",
            "launch",
            json!({"kind": kind, "argument": ""}),
        );
        v["window"].as_u64().unwrap()
    }
    fn find(&self, target: &str) -> Option<(Rect, String)> {
        let scene = self.world.scene(&self.actor, self.w, self.h).unwrap();
        let suffix = format!(":content:{target}");
        scene.nodes.iter().rev().find_map(|n| {
            let t = n.interaction.as_deref()?;
            (t.ends_with(&suffix) || (target.ends_with(':') && t.contains(&suffix)))
                .then(|| (n.transform.bounds(n.bounds), t.to_owned()))
        })
    }
    fn bounds(&self, target: &str) -> Rect {
        self.find(target)
            .unwrap_or_else(|| panic!("nothing on screen does {target}"))
            .0
    }
    fn pointer(&mut self, op: &str, (x, y): (i32, i32)) {
        self.act(
            "pointer.v1",
            op,
            json!({"x": x, "y": y, "width": self.w, "height": self.h}),
        );
    }
    fn centre(r: Rect) -> (i32, i32) {
        (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2)
    }
    fn click(&mut self, target: &str) {
        let r = self.bounds(target);
        self.pointer("click", Self::centre(r));
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
    fn type_text(&mut self, text: &str) {
        self.act("keyboard.v1", "type", json!({ "text": text }));
    }
    /// Let `seconds` of world time pass, as `sleep` does in the machine's shell.
    fn wait(&mut self, seconds: &str) {
        let r = self.act(
            "terminal.v1",
            "execute",
            json!({"command": format!("sleep {seconds}")}),
        );
        assert_eq!(r["exit_code"], 0, "{r}");
    }
    fn editor(&self, window: u64) -> cw_applications::apps::video::Editor {
        match &self
            .world
            .interfaces()
            .session(&self.actor)
            .unwrap()
            .machines[self.machine]
            .desktop
            .windows[&window]
            .state
        {
            AppState::Native(app) => app.video().expect("a video editor").clone(),
            other => panic!("not a video editor: {other:?}"),
        }
    }
    fn file(&self, path: &str) -> Vec<u8> {
        self.world.runtime().read_file(self.machine, path).unwrap()
    }
    fn home(&self) -> String {
        self.world
            .interfaces()
            .session(&self.actor)
            .unwrap()
            .machines[self.machine]
            .desktop
            .home_folder()
    }
    /// The monitor's picture as drawn in the scene: the largest image node.
    fn monitor(&self) -> (u32, u32, Vec<u8>) {
        let scene = self.world.scene(&self.actor, self.w, self.h).unwrap();
        scene
            .nodes
            .iter()
            .filter_map(|n| match &n.primitive {
                Primitive::Image {
                    width,
                    height,
                    rgba,
                } => Some((*width, *height, rgba.clone())),
                _ => None,
            })
            .max_by_key(|(w, h, _)| w * h)
            .expect("a monitor image")
    }
    /// Screen point of timeline frame `frame` on the lane of track `lane_index`, using
    /// the ruler's own geometry (its target carries the first frame it shows).
    fn lane_point(
        &self,
        ed: &cw_applications::apps::video::Editor,
        frame: i64,
        lane: i32,
        lane_h: i32,
    ) -> (i32, i32) {
        let (ruler, target) = self.find("video:ruler:").expect("a ruler");
        let scroll: i64 = target.rsplit(':').next().unwrap().parse().unwrap();
        let x = ruler.x + ed.pixels(frame - scroll) as i32 + 2;
        let y = ruler.y + ruler.height as i32 + lane * lane_h + lane_h / 2;
        (x, y)
    }
}

/// Import two clips through the sheet, drag them onto the timeline, add a cross
/// dissolve and a title, play to a timecode, check the monitor, export and re-import.
fn full_workflow(theme: &str, app: &str, folder: &str, lane_h: i32) {
    let mut s = session(theme, app, folder, (1280, 800));
    let home = s.home();
    let w = s.launch(app);
    // Import: the sheet lists the movies folder; picking imports each file.
    s.click("video:import");
    let ed = s.editor(w);
    assert!(ed.sheet.is_some(), "the import sheet opened");
    // The sheet starts in the product's own folder, relative to home.
    s.click("video:pick:Countdown.apng");
    s.click("video:pick:Color Bars.apng");
    s.click("video:pick:Music Bed.wav");
    s.click("video:close-sheet");
    let ed = s.editor(w);
    assert_eq!(ed.project.media.len(), 3, "{:?}", ed.status);
    assert!(ed
        .library
        .values()
        .all(|m| !m.frames.is_empty() || m.kind == MediaKind::Audio));
    let ids: Vec<u32> = ed.project.media.iter().map(|m| m.id).collect();
    let (countdown, bars, music) = (ids[0], ids[1], ids[2]);
    // Drag the countdown from the bin onto the start of the video lane.
    let lanes = ed.lanes();
    let v1 = ed.project.tracks_of(TrackKind::Video)[0].id;
    let a1 = ed.project.tracks_of(TrackKind::Audio)[0].id;
    let v_lane = lanes.iter().position(|t| *t == v1).unwrap() as i32;
    let a_lane = lanes.iter().position(|t| *t == a1).unwrap() as i32;
    let item = s.bounds(&format!("video:media:{countdown}:"));
    let drop = s.lane_point(&ed, 0, v_lane, lane_h);
    s.drag(&[Session::centre(item), (drop.0 + 40, drop.1), drop]);
    let ed = s.editor(w);
    assert_eq!(ed.project.clips.len(), 1, "{}", ed.status);
    let first = ed.project.clips[0].clone();
    assert_eq!((first.track, first.start, first.length), (v1, 0, 72));
    // The colour bars dropped at frame 60 land on the countdown and settle after it.
    let item = s.bounds(&format!("video:media:{bars}:"));
    let drop = s.lane_point(&ed, 60, v_lane, lane_h);
    s.drag(&[Session::centre(item), (drop.0 - 30, drop.1 - 10), drop]);
    let ed = s.editor(w);
    let second = ed
        .project
        .clips
        .iter()
        .find(|c| c.media() == Some(bars))
        .expect("the bars are on the timeline")
        .clone();
    assert_eq!((second.track, second.start, second.length), (v1, 72, 48));
    // Music under both, on the audio lane.
    let item = s.bounds(&format!("video:media:{music}:"));
    let drop = s.lane_point(&ed, 0, a_lane, lane_h);
    s.drag(&[Session::centre(item), drop]);
    let ed = s.editor(w);
    let music_clip = ed
        .project
        .clips
        .iter()
        .find(|c| c.media() == Some(music))
        .expect("the music is on the timeline")
        .clone();
    assert_eq!((music_clip.track, music_clip.start), (a1, 0));
    // A cross dissolve on the cut: select the countdown, then the transition.
    let clip_target = format!("video:clip:{}:", first.id);
    s.click(&clip_target);
    match app {
        "clipchamp" => s.click("video:tab:transitions"),
        "imovie" => s.click("video:tab:transitions"),
        _ => s.click("video:tab:compositions"),
    }
    s.click("video:add-transition:cross_dissolve");
    let ed = s.editor(w);
    assert_eq!(ed.project.transitions.len(), 1);
    assert_eq!(ed.project.transitions[0].frames, 24);
    // A title at the playhead, typed in.
    s.key("Home");
    match app {
        "clipchamp" => s.click("video:tab:text"),
        "imovie" => s.click("video:tab:titles"),
        _ => {}
    }
    s.click(if app == "kdenlive" {
        "video:add-title:plain"
    } else {
        "video:add-title:lower"
    });
    let ed = s.editor(w);
    assert_eq!(ed.field, cw_applications::apps::video::Field::Title);
    for _ in 0..12 {
        s.key("Backspace");
    }
    s.type_text("Launch Day");
    s.key("Enter");
    let ed = s.editor(w);
    let title = ed
        .project
        .clips
        .iter()
        .find(|c| matches!(c.source, Source::Title(_)))
        .expect("a title")
        .clone();
    match &title.source {
        Source::Title(t) => assert_eq!(t.text, "Launch Day"),
        _ => unreachable!(),
    }
    assert!(
        ed.masks.contains_key(&match &title.source {
            Source::Title(t) => t.raster_key(),
            _ => unreachable!(),
        }),
        "the renderer drew the title's glyphs"
    );
    // Play with the space bar and let a second and a half of world time pass.
    s.key("Home");
    s.key("Escape");
    s.key(" ");
    s.wait("1.5");
    s.key(" ");
    let ed = s.editor(w);
    assert!(ed.play.is_none());
    assert_eq!(ed.playhead, 36, "1.5 s at 24 fps");
    // The monitor shows exactly the engine's frame at the playhead: title over countdown.
    let (mw, mh, pixels) = s.monitor();
    let expected = Compositor {
        project: &ed.project,
        library: &ed.library,
        masks: &ed.masks,
    }
    .frame(36);
    assert_eq!((mw, mh), (320, 180));
    assert_eq!(pixels, expected.pixels());
    // The title really is in the picture: the frame differs from the countdown alone.
    let bare = ed.library[&countdown].frame(1_500_000).unwrap();
    assert_ne!(expected, bare);
    // Inside the dissolve the frame mixes both clips.
    s.key("End");
    s.key("Home");
    for _ in 0..70 {
        s.key("ArrowRight");
    }
    let ed = s.editor(w);
    assert_eq!(ed.playhead, 70);
    let (_, _, mid) = s.monitor();
    let a = ed.library[&countdown].frame(70 * 1_000_000 / 24).unwrap();
    assert_ne!(
        mid,
        a.pixels(),
        "the dissolve shows more than the outgoing clip"
    );
    // Export: 160x90 at 12 fps, encoded a few frames per step.
    s.click("video:export");
    for _ in 0..20 {
        s.key("Backspace");
    }
    s.type_text("Trailer");
    s.click("video:export-size:160x90");
    s.click("video:export-fps:12");
    s.click("video:export-start");
    let ed = s.editor(w);
    let job = ed.export.clone().expect("an export is running");
    let total = job.total;
    assert_eq!(total, 60, "120 frames of timeline at 12 of 24 fps");
    let mut steps = 0;
    while s.editor(w).export.is_some() {
        s.pointer("move", (5, 5));
        steps += 1;
        assert!(steps < 200, "the export never finished");
    }
    assert!(
        steps >= 2,
        "the export advanced over several steps, took {steps}"
    );
    let ed = s.editor(w);
    assert!(ed.status.contains("Exported"), "{}", ed.status);
    let movie = s.file(&format!("{home}/{folder}/Trailer.apng"));
    let sound = s.file(&format!("{home}/{folder}/Trailer.wav"));
    // The movie is a real APNG of the programme at the chosen size, frame for frame.
    let back = Media::import("Trailer.apng", &movie, 22_050).unwrap();
    assert_eq!((back.width, back.height, back.frames.len()), (160, 90, 60));
    let comp = Compositor {
        project: &ed.project,
        library: &ed.library,
        masks: &ed.masks,
    };
    for i in [0u32, 17, 35, 59] {
        let expect = cw_raster::transform::resize(
            &comp.frame(i64::from(i) * 2),
            160,
            90,
            cw_raster::transform::Resample::Bilinear,
        );
        assert_eq!(
            back.frame(i64::from(i) * 1_000_000 / 12).unwrap(),
            expect,
            "frame {i}"
        );
    }
    let heard = cw_video::wav::decode(&sound).unwrap();
    assert_eq!(
        heard.samples,
        cw_video::audio::mixdown(&ed.project, &ed.library)
    );
    // Re-import the export: it holds exactly the frames that were written.
    if app == "clipchamp" {
        s.click("video:tab:media");
    }
    s.click("video:import");
    s.click("video:pick:Trailer.apng");
    s.click("video:close-sheet");
    let ed = s.editor(w);
    let trailer = ed
        .project
        .media
        .iter()
        .find(|m| m.path.ends_with("Trailer.apng"))
        .unwrap()
        .id;
    let again = &ed.library[&trailer];
    assert_eq!(
        (&again.frames, &again.starts_us),
        (&back.frames, &back.starts_us)
    );
    assert_eq!(again.duration_us, 5_000_000);
}

#[test]
fn clipchamp_imports_edits_plays_and_exports() {
    full_workflow("virtual-windows-11", "clipchamp", "Videos", 44);
}
#[test]
fn imovie_imports_edits_plays_and_exports() {
    full_workflow("virtual-macos-golden-gate", "imovie", "Movies", 50);
}
#[test]
fn kdenlive_imports_edits_plays_and_exports() {
    full_workflow("virtual-ubuntu-24", "kdenlive", "Videos", 40);
}

/// The phone editors: tap + to pick clips (which lays each at the playhead), tap a clip
/// and split it at the playhead, add a transition, play, and share an export.
fn phone_workflow(theme: &str, app: &str, size: (u32, u32)) {
    let mut s = session(theme, app, "Movies", size);
    let home = s.home();
    let w = s.launch(app);
    for name in ["Countdown.apng", "Sunset.apng"] {
        s.click("video:import");
        s.click(&format!("video:pick-add:{name}"));
        // The picker closes on its own after adding.
        assert!(s.editor(w).sheet.is_none());
        s.key("End");
    }
    let ed = s.editor(w);
    let v1 = ed.project.tracks_of(TrackKind::Video)[0].id;
    let clips: Vec<_> = ed.project.on_track(v1).into_iter().cloned().collect();
    assert_eq!(clips.len(), 2);
    assert_eq!((clips[0].start, clips[1].start), (0, 72));
    // Tap the first clip; its tools replace the add tools. Split it at one second.
    s.key("Home");
    for _ in 0..24 {
        s.key("ArrowRight");
    }
    s.click(&format!("video:clip:{}:", clips[0].id));
    s.click("video:inspector:actions");
    s.click("video:split");
    let ed = s.editor(w);
    assert_eq!(ed.project.on_track(v1).len(), 3);
    // A transition on the cut after the first piece.
    s.click("video:deselect");
    s.click("video:tab:transitions");
    s.click("video:add-transition:cross_dissolve");
    assert_eq!(s.editor(w).project.transitions.len(), 1);
    // Tap play and let a second pass.
    s.key("Home");
    s.click("video:play");
    s.wait("1");
    s.click("video:play");
    let ed = s.editor(w);
    assert_eq!(ed.playhead, 24);
    let (_, _, pixels) = s.monitor();
    let expected = Compositor {
        project: &ed.project,
        library: &ed.library,
        masks: &ed.masks,
    }
    .frame(24);
    assert_eq!(pixels, expected.pixels());
    // Share an export.
    s.click("video:export");
    s.click("video:export-size:160x90");
    s.click("video:export-start");
    let mut steps = 0;
    while s.editor(w).export.is_some() {
        s.pointer("move", (5, 5));
        steps += 1;
        assert!(steps < 200);
    }
    let movie = s.file(&format!("{home}/Movies/My Movie.apng"));
    let back = Media::import("My Movie.apng", &movie, 22_050).unwrap();
    assert_eq!((back.width, back.height), (160, 90));
    assert_eq!(back.duration_us, 6_000_000);
}

#[test]
fn imovie_on_the_phone_adds_splits_plays_and_shares() {
    phone_workflow("virtual-ios-18", "imovie", (390, 844));
}
#[test]
fn the_android_editor_adds_splits_plays_and_exports() {
    phone_workflow("virtual-android-12", "videoeditor", (412, 892));
}
