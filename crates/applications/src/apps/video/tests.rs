use super::*;
use crate::desktop_scene::Painter;
use crate::{AppEnv, SystemSettings};

fn env(theme: DesktopTheme, width: u32, height: u32, clock_us: u64) -> AppEnv<'static> {
    AppEnv {
        theme,
        width,
        height,
        clock_us,
        settings: &SystemSettings::DEFAULT,
        clipboard: None,
        share_to: None,
        files: Default::default(),
        editor: None,
        pointer: None,
    }
}

/// An editor with the sample movies and sounds imported the way a read delivers them.
fn loaded(product: Product) -> Editor {
    // Decoding the samples is the slow part; do it once and share the library.
    static LOADED: std::sync::OnceLock<Editor> = std::sync::OnceLock::new();
    let base = LOADED.get_or_init(|| {
        let (mut ed, _) = Editor::launch(Product::Clipchamp, "", 1);
        for (name, bytes) in cw_video::samples::files().unwrap() {
            let path = format!("Movies/{name}");
            ed.reading.push((path.clone(), Purpose::Import));
            ed.bytes(1, &path, Ok(bytes)).unwrap();
        }
        ed
    });
    let (fresh, _) = Editor::launch(product, "", 1);
    Editor {
        product,
        folder: fresh.folder,
        tab: fresh.tab,
        ..base.clone()
    }
}
fn media_id(ed: &Editor, name: &str) -> u32 {
    ed.project
        .media
        .iter()
        .find(|m| m.path.ends_with(name))
        .unwrap()
        .id
}
/// Two clips on the main track, music under them, a title above, a transition.
fn edited(product: Product) -> Editor {
    let mut ed = loaded(product);
    for name in ["Countdown.apng", "Color Bars.apng", "Music Bed.wav"] {
        let id = media_id(&ed, name);
        ed.playhead = if name.ends_with(".wav") {
            0
        } else {
            ed.project.duration()
        };
        ed.command(1, &format!("append:{id}"), 0).unwrap();
    }
    let first = ed.project.clips[0].id;
    ed.selected = Some(first);
    ed.command(1, "add-transition:cross_dissolve", 0).unwrap();
    ed.playhead = 10;
    ed.command(1, "add-title:lower", 0).unwrap();
    ed.text_rasterized(3, 2, vec![255; 6]).unwrap();
    ed.field = Field::None;
    ed
}

fn painted(ed: &Editor, theme: DesktopTheme, w: u32, h: u32) -> Vec<String> {
    let mut p = Painter::themed(theme, w, h, 0);
    let e = env(theme, w, h, 0);
    if theme.mobile() {
        mobile::render(ed, &mut p, &e);
    } else {
        desktop::render(ed, &mut p, &e);
    }
    p.scene
        .nodes
        .iter()
        .filter_map(|n| n.interaction.clone())
        .collect()
}

/// Every control painted, in every state worth painting, does something real: a click
/// (or a press and release on a drag surface) is accepted.
#[test]
fn every_painted_control_is_one_the_editor_accepts() {
    for (product, theme, w, h) in [
        (Product::Clipchamp, DesktopTheme::Windows, 1200, 760),
        (Product::Imovie, DesktopTheme::Macos, 1200, 760),
        (Product::Kdenlive, DesktopTheme::Ubuntu, 1200, 760),
        (Product::Imovie, DesktopTheme::Ios, 390, 780),
        (Product::VideoEditor, DesktopTheme::Android, 412, 820),
    ] {
        let mut states = vec![
            Editor::launch(product, "", 1).0,
            loaded(product),
            edited(product),
        ];
        let base = edited(product);
        for page in [
            "audio",
            "fade",
            "color",
            "speed",
            "transform",
            "crop",
            "overlay",
            "title",
            "actions",
        ] {
            let mut ed = base.clone();
            ed.selected = Some(ed.project.clips[0].id);
            ed.inspector = page.into();
            states.push(ed);
        }
        let mut title = base.clone();
        title.selected = title
            .project
            .clips
            .iter()
            .find(|c| matches!(c.source, Source::Title(_)))
            .map(|c| c.id);
        title.inspector = "title".into();
        states.push(title);
        let mut transition = base.clone();
        transition.selected = None;
        transition.selected_transition = Some(transition.project.transitions[0].id);
        states.push(transition);
        for tab in [
            "text",
            "titles",
            "transitions",
            "backgrounds",
            "compositions",
            "overlay",
            "audio",
        ] {
            let mut ed = base.clone();
            ed.selected = None;
            ed.tab = tab.into();
            states.push(ed);
        }
        let mut sheet = base.clone();
        sheet.sheet = Some(Sheet::Browse {
            folder: "Movies".into(),
            entries: vec!["Clips/".into(), "Countdown.apng".into()],
            loading: false,
            error: None,
            project: false,
        });
        states.push(sheet);
        let mut export = base.clone();
        export.command(1, "export", 0).unwrap();
        states.push(export.clone());
        export.command(1, "export-start", 0).unwrap();
        states.push(export);
        let mut save = base.clone();
        save.command(1, "save", 0).unwrap();
        states.push(save);
        for ed in states {
            let targets = painted(&ed, theme, w, h);
            assert!(!targets.is_empty(), "{product:?} paints no controls");
            for target in targets {
                let command = target
                    .strip_prefix("video:")
                    .unwrap_or_else(|| panic!("{target}"));
                let mut e = ed.clone();
                let result = if Editor::drags(&target) {
                    e.pointer(1, command, PointerPhase::Down, 2, 2, 0)
                        .and_then(|_| e.pointer(1, command, PointerPhase::Up, 2, 2, 0))
                } else {
                    e.command(1, command, 0)
                };
                assert!(
                    result.is_ok(),
                    "{product:?}/{theme:?}: {target} refused: {result:?}"
                );
            }
        }
    }
}

#[test]
fn clips_move_and_trim_under_the_pointer_and_snap_to_edges() {
    let mut ed = loaded(Product::Kdenlive);
    let bars = media_id(&ed, "Color Bars.apng");
    let count = media_id(&ed, "Countdown.apng");
    let v1 = ed.project.tracks_of(TrackKind::Video)[0].id;
    // Drop from the bin: the lanes' origin is 100 px right of and 200 px below the item;
    // releasing at (148, 220) is frame 48 * 24 / 48 = 24 on lane 0.
    ed.pointer(
        1,
        &format!("media:{count}:100:200:40:0"),
        PointerPhase::Down,
        5,
        5,
        0,
    )
    .unwrap();
    ed.pointer(
        1,
        &format!("media:{count}:100:200:40:0"),
        PointerPhase::Move,
        60,
        100,
        0,
    )
    .unwrap();
    assert!(matches!(ed.drag, Some(Drag::Media { .. })));
    ed.pointer(
        1,
        &format!("media:{count}:100:200:40:0"),
        PointerPhase::Up,
        148,
        220,
        0,
    )
    .unwrap();
    let c = ed.project.clips[0].clone();
    assert_eq!((c.track, c.start, c.length), (v1, 24, 72));
    // A second drop lands near the first clip's end and snaps to it.
    ed.pointer(
        1,
        &format!("media:{bars}:100:200:40:0"),
        PointerPhase::Down,
        5,
        5,
        0,
    )
    .unwrap();
    ed.pointer(
        1,
        &format!("media:{bars}:100:200:40:0"),
        PointerPhase::Up,
        100 + 194,
        215,
        0,
    )
    .unwrap();
    let b = ed
        .project
        .clips
        .iter()
        .find(|x| x.media() == Some(bars))
        .unwrap()
        .clone();
    assert_eq!(b.start, 96, "snapped to the end of the countdown");
    // Drag the bars 48 px right (24 frames); it moves, with one undo step for it.
    let undo_before = ed.undo.len();
    let surface = format!("clip:{}:40", b.id);
    ed.pointer(1, &surface, PointerPhase::Down, 10, 10, 0)
        .unwrap();
    ed.pointer(1, &surface, PointerPhase::Move, 58, 10, 0)
        .unwrap();
    ed.pointer(1, &surface, PointerPhase::Up, 58, 12, 0)
        .unwrap();
    assert_eq!(ed.project.clip(b.id).unwrap().start, 120);
    assert_eq!(ed.undo.len(), undo_before + 1);
    // Down one lane is the audio track: a picture stays on a video track.
    ed.pointer(1, &surface, PointerPhase::Down, 10, 10, 0)
        .unwrap();
    ed.pointer(1, &surface, PointerPhase::Up, 10, 55, 0)
        .unwrap();
    assert_eq!(ed.project.clip(b.id).unwrap().track, v1);
    // Up one lane, above the top video track, makes a new video track for it.
    ed.pointer(1, &surface, PointerPhase::Down, 10, 10, 0)
        .unwrap();
    ed.pointer(1, &surface, PointerPhase::Up, 10, -30, 0)
        .unwrap();
    let moved = ed.project.clip(b.id).unwrap().clone();
    assert_eq!(ed.project.tracks_of(TrackKind::Video).len(), 2);
    assert_ne!(moved.track, v1);
    // Trim the countdown's start in by 24 px (12 frames) with its handle.
    ed.pointer(1, &format!("trim-in:{}", c.id), PointerPhase::Down, 3, 5, 0)
        .unwrap();
    ed.pointer(
        1,
        &format!("trim-in:{}", c.id),
        PointerPhase::Move,
        27,
        5,
        0,
    )
    .unwrap();
    ed.pointer(1, &format!("trim-in:{}", c.id), PointerPhase::Up, 27, 5, 0)
        .unwrap();
    let c2 = ed.project.clip(c.id).unwrap().clone();
    assert_eq!((c2.start, c2.length, c2.offset), (36, 60, 1200));
    // And its end out as far as the media allows: nothing, it already ends there.
    ed.pointer(
        1,
        &format!("trim-out:{}", c.id),
        PointerPhase::Down,
        3,
        5,
        0,
    )
    .unwrap();
    ed.pointer(
        1,
        &format!("trim-out:{}", c.id),
        PointerPhase::Up,
        400,
        5,
        0,
    )
    .unwrap();
    assert_eq!(ed.project.clip(c.id).unwrap().end(), 96);
    // Undo walks every edit back, redo forward again.
    let edited = ed.project.clone();
    while ed.command(1, "undo", 0).is_ok() {}
    assert!(ed.project.clips.is_empty());
    while ed.command(1, "redo", 0).is_ok() {}
    assert_eq!(ed.project, edited);
}

#[test]
fn playback_follows_the_world_clock_and_the_shuttle_keys() {
    let mut ed = edited(Product::Imovie);
    let second = 1_000_000;
    ed.key(1, "Home", 0, false).unwrap();
    ed.key(1, " ", 5 * second, false).unwrap();
    assert_eq!(ed.now(5 * second + second / 2), 12);
    // L doubles the speed, K stops, J plays backwards.
    ed.key(1, "l", 6 * second, false).unwrap();
    assert_eq!(ed.playhead, 24);
    assert_eq!(ed.play.unwrap().rate, 200);
    assert_eq!(ed.now(7 * second), 72);
    ed.key(1, "k", 7 * second, false).unwrap();
    assert!(ed.play.is_none());
    assert_eq!(ed.playhead, 72);
    ed.key(1, "j", 7 * second, false).unwrap();
    assert_eq!(ed.now(8 * second), 48);
    ed.key(1, "k", 8 * second, false).unwrap();
    // Arrow keys step a frame; playing past the end stops there.
    ed.key(1, "ArrowRight", 9 * second, false).unwrap();
    assert_eq!(ed.playhead, 49);
    ed.key(1, " ", 9 * second, false).unwrap();
    assert_eq!(ed.now(99 * second), ed.project.duration());
    ed.key(1, "ArrowLeft", 99 * second, false).unwrap();
    assert!(ed.play.is_none());
    assert_eq!(ed.playhead, ed.project.duration() - 1);
    // Clipchamp has no shuttle keys; a letter there is refused, not guessed at.
    let mut cc = edited(Product::Clipchamp);
    assert!(cc.key(1, "l", 0, false).is_err());
    // Its split key is S; iMovie's is Command-B; Kdenlive's is Shift-R.
    cc.playhead = 30;
    cc.selected = None;
    let before = cc.project.clips.len();
    cc.key(1, "s", 0, false).unwrap();
    assert!(cc.project.clips.len() > before);
    let mut kd = edited(Product::Kdenlive);
    kd.playhead = 30;
    kd.selected = None;
    let before = kd.project.clips.len();
    kd.key(1, "Shift+R", 0, false).unwrap();
    assert!(kd.project.clips.len() > before);
}

#[test]
fn products_refuse_what_they_do_not_have() {
    let mut cc = edited(Product::Clipchamp);
    cc.selected = Some(cc.project.clips[0].id);
    assert!(cc.command(1, "reverse", 0).is_err());
    assert!(cc.command(1, "key:opacity", 0).is_err());
    assert!(cc.command(1, "ken-burns", 0).is_err());
    let lock = cc.project.tracks[0].id;
    assert!(cc.command(1, &format!("track-lock:{lock}"), 0).is_err());
    let mut im = edited(Product::Imovie);
    let v = im.project.tracks_of(TrackKind::Video)[0].id;
    assert!(im.command(1, &format!("track-hide:{v}"), 0).is_err());
    im.selected = Some(im.project.clips[0].id);
    im.command(1, "ken-burns", 0).unwrap();
    assert!(im.selected_clip().unwrap().scale.animated());
    // Kdenlive has keyframes, locks and everything else.
    let mut kd = edited(Product::Kdenlive);
    let id = kd.project.clips[0].id;
    kd.selected = Some(id);
    kd.playhead = 0;
    kd.command(1, "key:opacity", 0).unwrap();
    kd.playhead = 20;
    kd.command(1, "set:opacity:0", 0).unwrap();
    let c = kd.project.clip(id).unwrap();
    assert_eq!(c.opacity.keys.len(), 2);
    assert_eq!(c.opacity.at(10), 500);
    kd.command(1, "ease:opacity", 0).unwrap();
    assert_eq!(
        kd.project.clip(id).unwrap().opacity.keys[1].ease,
        Ease::Ease
    );
    kd.playhead = 5;
    assert!(
        kd.command(1, "ease:opacity", 0).is_err(),
        "no keyframe at the playhead"
    );
}

#[test]
fn a_saved_project_opens_again_and_finds_its_media() {
    let mut ed = edited(Product::Kdenlive);
    ed.command(1, "save", 0).unwrap();
    ed.text(1, "").unwrap();
    for _ in 0..20 {
        ed.key(1, "Backspace", 0, false).unwrap();
    }
    ed.text(1, "Launch").unwrap();
    let effects = ed.key(1, "Enter", 0, false).unwrap();
    let written = effects
        .iter()
        .find_map(|e| match e {
            AppEffect::WriteBytes { path, bytes, .. } => Some((path.clone(), bytes.clone())),
            _ => None,
        })
        .expect("the project is written");
    assert_eq!(written.0, "Videos/Launch.cwvideo");
    ed.saved(&written.0, Ok(()));
    assert!(!ed.modified);
    // Open it in a fresh editor: the project comes back and asks for each file it uses.
    let (mut fresh, _) = Editor::launch(Product::Kdenlive, "", 2);
    fresh.reading.push((written.0.clone(), Purpose::Project));
    let reads = fresh.bytes(2, &written.0, Ok(written.1)).unwrap();
    assert_eq!(fresh.project, ed.project);
    let paths: Vec<String> = reads
        .iter()
        .filter_map(|e| match e {
            AppEffect::ReadBytes { path, .. } => Some(path.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(paths.len(), ed.project.media.len());
    // One file has gone missing since: it is marked missing, the rest load.
    let files = cw_video::samples::files().unwrap();
    for path in &paths {
        let name = path.rsplit('/').next().unwrap();
        let result = if name == "Color Bars.apng" {
            Err("requested resource not found".to_owned())
        } else {
            Ok(files.iter().find(|(n, _)| *n == name).unwrap().1.clone())
        };
        fresh.bytes(2, path, result).unwrap();
    }
    let bars = media_id(&fresh, "Color Bars.apng");
    assert!(fresh.offline.contains_key(&bars));
    assert_eq!(fresh.library.len(), ed.project.media.len() - 1);
    // The rasterised title line is asked for again.
    assert!(reads
        .iter()
        .any(|e| matches!(e, AppEffect::RasterText { .. })));
}

#[test]
fn an_export_runs_a_few_frames_per_step_and_writes_both_files() {
    let mut ed = edited(Product::Clipchamp);
    ed.command(1, "export", 0).unwrap();
    ed.command(1, "export-size:160x90", 0).unwrap();
    ed.command(1, "export-fps:12", 0).unwrap();
    ed.command(1, "export-start", 0).unwrap();
    let total = ed.export.as_ref().unwrap().total;
    let mut steps = 0;
    let effects = loop {
        steps += 1;
        let effects = ed.background(1);
        if !effects.is_empty() {
            break effects;
        }
        assert!(ed.busy());
    };
    assert_eq!(steps as u32, total.div_ceil(16));
    assert!(!ed.busy());
    let paths: Vec<&str> = effects
        .iter()
        .filter_map(|e| match e {
            AppEffect::WriteBytes { path, .. } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(paths, ["Videos/My Movie.wav", "Videos/My Movie.apng"]);
    // Cancelling part-way leaves nothing behind.
    ed.command(1, "export", 0).unwrap();
    ed.command(1, "export-start", 0).unwrap();
    assert!(ed.background(1).is_empty());
    ed.command(1, "export-cancel", 0).unwrap();
    assert!(!ed.busy());
}

#[test]
fn every_product_serializes_and_restores() {
    for product in [
        Product::Clipchamp,
        Product::Imovie,
        Product::Kdenlive,
        Product::VideoEditor,
    ] {
        let ed = edited(product);
        let json = serde_json::to_string(&ed).unwrap();
        let back: Editor = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ed);
        // Media travels as base64, not as number arrays.
        assert!(json.len() < 4 * 1024 * 1024, "{} bytes", json.len());
    }
}
