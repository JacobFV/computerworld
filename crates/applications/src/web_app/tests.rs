use super::*;

fn env(theme: DesktopTheme, width: u32, height: u32) -> crate::AppEnv<'static> {
    crate::AppEnv {
        theme,
        width,
        height,
        clock_us: 0,
        settings: &crate::SystemSettings::DEFAULT,
        clipboard: None,
        share_to: None,
        editor: None,
        pointer: None,
        files: Default::default(),
    }
}

fn scene(app: &WebApp, theme: DesktopTheme) -> cw_scene::Scene {
    let e = env(theme, 900, 600);
    let mut p = Painter::themed(theme, 900, 600, 1 << 52);
    app.render(&mut p, &e);
    p.scene
}

fn write_png(path: &std::path::Path, scene: &cw_scene::Scene) {
    let frame = cw_render::Renderer::new().render(scene);
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), frame.width, frame.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&frame.rgba)
        .unwrap();
}

/// Content stills of the native and the web Notes side by side, per platform, for a
/// person to compare: `CW_STILLS_DIR=/tmp/x cargo test -p cw-applications stills -- --ignored`.
#[test]
#[ignore]
fn stills() {
    let dir = std::path::PathBuf::from(std::env::var("CW_STILLS_DIR").expect("CW_STILLS_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let names = vec![
        "groceries.txt".to_owned(),
        "ideas.txt".into(),
        "plans.txt".into(),
    ];
    for theme in [
        DesktopTheme::Macos,
        DesktopTheme::Windows,
        DesktopTheme::Ubuntu,
        DesktopTheme::Ios,
        DesktopTheme::Android,
    ] {
        let (w, h) = if theme.mobile() {
            (390, 760)
        } else {
            (760, 480)
        };
        let e = env(theme, w, h);
        let (mut native, _) = crate::apps::notes::Notes::launch("/home/alice/Notes", 1, 0);
        native.listed(names.clone());
        let (mut web, _) = WebApp::launch("notes", "/home/alice/Notes", 1, 0, theme).unwrap();
        web.tree_listed(1, "/home/alice/Notes", Ok(names.clone()))
            .unwrap();
        let shoot = |label: &str, native: &crate::apps::notes::Notes, web: &WebApp| {
            for (which, state) in [
                (
                    "native",
                    crate::AppState::Native(crate::NativeApp::Notes(native.clone())),
                ),
                (
                    "web",
                    crate::AppState::Native(crate::NativeApp::Web(web.clone())),
                ),
            ] {
                let scene = crate::desktop_scene::app_content_with(&state, &e);
                let name = format!("{}-{label}-{which}.png", theme.platform());
                write_png(&dir.join(name), &scene);
            }
        };
        shoot("list", &native, &web);
        native.click(1, "notes:open:ideas.txt", 0).unwrap();
        native.loaded("Buy a lamp for the desk.\nCall the landlord about the heating.".into());
        web.click(1, "notes:open:ideas.txt", 0).unwrap();
        web.files_read(
            1,
            "web:2",
            vec![(
                "/home/alice/Notes/ideas.txt".into(),
                Ok("Buy a lamp for the desk.\nCall the landlord about the heating.".into()),
            )],
        )
        .unwrap();
        shoot("open", &native, &web);
        native.text(" More").unwrap();
        web.text_effects(1, " More").unwrap();
        shoot("dirty", &native, &web);
        eprintln!("{} console {:?}", theme.platform(), web.console());
    }
}

/// A machine's filesystem, just enough to answer both Notes' effects the way the
/// environment does.
#[derive(Default)]
struct Disk {
    files: std::collections::BTreeMap<String, String>,
    folders: std::collections::BTreeSet<String>,
}
impl Disk {
    fn listing(&self, path: &str) -> Result<Vec<String>, String> {
        if !self.folders.contains(path) {
            return Err("folder not found".into());
        }
        let prefix = format!("{path}/");
        let mut names: Vec<String> = self
            .files
            .keys()
            .filter_map(|f| f.strip_prefix(&prefix))
            .filter(|rest| !rest.contains('/'))
            .map(str::to_owned)
            .collect();
        names.sort();
        Ok(names)
    }
    /// Runs `effects` for the native Notes, delivering what they answer.
    fn native(
        &mut self,
        app: &mut crate::apps::notes::Notes,
        effects: Vec<AppEffect>,
    ) -> Result<(), String> {
        for effect in effects {
            match effect {
                AppEffect::ListDirectory { path, .. } => match self.listing(&path) {
                    Ok(names) => app.listed(names),
                    Err(reason) => app.offline("listing", &reason),
                },
                AppEffect::ReadFile { path, .. } => {
                    app.loaded(self.files.get(&path).cloned().ok_or("file not found")?)
                }
                AppEffect::WriteFile { path, content, .. } => {
                    self.files.insert(path, content);
                }
                AppEffect::CreateDirectory { path, .. } => {
                    self.folders.insert(path);
                }
                other => panic!("native Notes asked for {other:?}"),
            }
        }
        Ok(())
    }
    /// Runs `effects` for the web Notes, delivering what they answer, until none are left.
    fn web(&mut self, app: &mut WebApp, effects: Vec<AppEffect>) -> Result<(), String> {
        let mut pending: std::collections::VecDeque<AppEffect> = effects.into();
        while let Some(effect) = pending.pop_front() {
            let more = match effect {
                AppEffect::ListTree { path, .. } => {
                    let result = self.listing(&path);
                    app.tree_listed(1, &path, result)?
                }
                AppEffect::ReadFiles { tag, paths, .. } => {
                    let files = paths
                        .into_iter()
                        .map(|p| {
                            let r = self
                                .files
                                .get(&p)
                                .cloned()
                                .ok_or_else(|| "file not found".to_owned());
                            (p, r)
                        })
                        .collect();
                    app.files_read(1, &tag, files)?
                }
                AppEffect::WriteFile { path, content, .. } => {
                    self.files.insert(path.clone(), content);
                    app.written(1, &path)?
                }
                AppEffect::CreateDirectory { path, .. } => {
                    self.folders.insert(path);
                    vec![]
                }
                other => panic!("web Notes asked for {other:?}"),
            };
            pending.extend(more);
        }
        Ok(())
    }
}

const FOLDER: &str = "/home/alice/Notes";

fn disk_with(names: &[&str]) -> Disk {
    let mut disk = Disk::default();
    disk.folders.insert(FOLDER.into());
    for name in names {
        disk.files
            .insert(format!("{FOLDER}/{name}"), format!("text of {name}"));
    }
    disk
}

fn web_notes(disk: &mut Disk, theme: DesktopTheme) -> WebApp {
    let (mut app, effects) = WebApp::launch("notes", FOLDER, 1, 0, theme).unwrap();
    disk.web(&mut app, effects).unwrap();
    app
}

#[test]
fn notes_are_real_files_read_and_written_through_the_filesystem() {
    let mut disk = disk_with(&["ideas.txt"]);
    let (mut app, effects) = WebApp::launch("notes", FOLDER, 1, 0, DesktopTheme::Macos).unwrap();
    assert_eq!(
        effects,
        vec![AppEffect::ListTree {
            window: 1,
            path: FOLDER.into(),
            depth: 1
        }]
    );
    disk.web(&mut app, effects).unwrap();
    assert_eq!(app.state()["entries"], serde_json::json!(["ideas.txt"]));
    let effects = app.click(1, "notes:open:ideas.txt", 0).unwrap();
    assert!(
        matches!(&effects[..], [AppEffect::ReadFiles { paths, .. }] if paths[0] == "/home/alice/Notes/ideas.txt")
    );
    disk.web(&mut app, effects).unwrap();
    assert_eq!(app.state()["text"], "text of ideas.txt");
    assert_eq!(app.state()["dirty"], false);
    assert_eq!(app.text_field().as_deref(), Some("notes:body"));
    let effects = app.text_effects(1, " more").unwrap();
    assert!(effects.is_empty());
    assert_eq!(app.state()["dirty"], true);
    assert!(app.modified());
    let effects = app.click(1, "notes:save", 0).unwrap();
    // The folder is created first: a machine that has never taken a note has none.
    assert!(matches!(&effects[0], AppEffect::CreateDirectory { path, .. } if path == FOLDER));
    assert!(
        matches!(&effects[1], AppEffect::WriteFile { path, content, .. }
        if path == "/home/alice/Notes/ideas.txt" && content == "text of ideas.txt more")
    );
    disk.web(&mut app, effects).unwrap();
    assert_eq!(
        disk.files["/home/alice/Notes/ideas.txt"],
        "text of ideas.txt more"
    );
    assert_eq!(app.state()["dirty"], false);
    assert_eq!(app.document(), "/home/alice/Notes/ideas.txt");
    assert_eq!(app.caption(), "ideas.txt");
}

#[test]
fn editing_without_an_open_note_is_refused() {
    let mut disk = disk_with(&[]);
    let mut app = web_notes(&mut disk, DesktopTheme::Macos);
    assert!(app.text_effects(1, "x").is_err());
    assert!(
        app.click(1, "notes:save", 0).is_err(),
        "no Save without a note"
    );
    assert!(app.click(1, "notes:open:missing.txt", 0).is_err());
    assert!(app.click(1, "not-mine", 0).is_err());
    assert!(app.key(1, "Backspace", 0).is_err());
    assert_eq!(app.text_field(), None);
}

#[test]
fn a_new_note_is_named_from_simulation_time() {
    let mut disk = disk_with(&[]);
    let mut app = web_notes(&mut disk, DesktopTheme::Macos);
    app.click(1, "notes:new", 90_000_000).unwrap();
    assert_eq!(app.state()["open"], "note-90.txt");
    assert_eq!(app.state()["editing"], true);
}

#[test]
fn a_missing_folder_is_shown_not_thrown() {
    let mut disk = Disk::default();
    let app = web_notes(&mut disk, DesktopTheme::Macos);
    assert_eq!(app.state()["problem"], "folder not found");
    let mut page = cw_protocol::Page::new("Notes");
    app.page(&mut page);
    assert!(page.elements.iter().any(|e| matches!(e,
        cw_protocol::PageElement::Text { id, text } if id == "notes-problem" && text == "folder not found")));
}

/// The controls a scene offers: its `notes:*` interactions, scroll bars aside.
fn controls(scene: &cw_scene::Scene) -> std::collections::BTreeSet<String> {
    scene
        .nodes
        .iter()
        .filter_map(|n| n.interaction.clone())
        .filter(|i| i.starts_with("notes:"))
        .collect()
}

/// The web Notes and the native one it replaces, driven by the same inputs against
/// the same disk, offer the same controls, declare the same state and accept and
/// refuse the same inputs.
#[test]
fn the_web_notes_behaves_as_the_native_notes() {
    for (seed, theme) in [
        (1_u64, DesktopTheme::Macos),
        (2, DesktopTheme::Ios),
        (3, DesktopTheme::Windows),
        (4, DesktopTheme::Android),
        (5, DesktopTheme::Ubuntu),
    ] {
        let (w, h) = if theme.mobile() {
            (390, 760)
        } else {
            (900, 600)
        };
        let e = env(theme, w, h);
        let mut native_disk = disk_with(&["a.txt", "b.txt"]);
        let mut web_disk = disk_with(&["a.txt", "b.txt"]);
        let (mut native, effects) = crate::apps::notes::Notes::launch(FOLDER, 1, 0);
        native_disk.native(&mut native, effects).unwrap();
        let mut web = web_notes(&mut web_disk, theme);
        let mut x = seed;
        let mut next = move |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        let mut log: Vec<String> = vec![];
        for step in 0..150 {
            let clock = step * 1_500_000;
            let native_scene = crate::desktop_scene::app_content_with(
                &crate::AppState::Native(crate::NativeApp::Notes(native.clone())),
                &e,
            );
            let web_scene = crate::desktop_scene::app_content_with(
                &crate::AppState::Native(crate::NativeApp::Web(web.clone())),
                &e,
            );
            let offered = controls(&native_scene);
            assert_eq!(
                offered,
                controls(&web_scene),
                "{theme:?} step {step}; log {log:?}"
            );
            let (native_ok, web_ok, what) = match next(8) {
                0..=3 => {
                    let targets: Vec<&String> = offered.iter().collect();
                    let target = targets[next(targets.len() as u64) as usize].clone();
                    (
                        native
                            .click(1, &target, clock)
                            .and_then(|e| native_disk.native(&mut native, e))
                            .is_ok(),
                        web.click(1, &target, clock)
                            .and_then(|e| web_disk.web(&mut web, e))
                            .is_ok(),
                        target,
                    )
                }
                4 | 5 => {
                    let text = ["hi", " there", "x", "Ünïcode ✓"][next(4) as usize];
                    (
                        native.text(text).is_ok(),
                        web.text_effects(1, text)
                            .and_then(|e| web_disk.web(&mut web, e))
                            .is_ok(),
                        format!("type {text:?}"),
                    )
                }
                _ => {
                    let key = ["Backspace", "Enter", "Ctrl+s", "Meta+s", "Tab"][next(5) as usize];
                    (
                        native
                            .key(1, key, clock)
                            .and_then(|e| native_disk.native(&mut native, e))
                            .is_ok(),
                        web.key(1, key, clock)
                            .and_then(|e| web_disk.web(&mut web, e))
                            .is_ok(),
                        key.into(),
                    )
                }
            };
            log.push(format!("{step}:{what}:{native_ok}/{web_ok}"));
            let native_state = serde_json::to_value(&native).unwrap();
            let mut native_state = native_state.as_object().unwrap().clone();
            native_state.remove("app");
            assert_eq!(
                (native_ok, serde_json::Value::Object(native_state)),
                (web_ok, web.state().clone()),
                "{theme:?} step {step}: {what}; console {:?}; log {log:?}",
                web.console()
            );
            assert_eq!(
                native_disk.files, web_disk.files,
                "{theme:?} step {step}: {what}"
            );
            let native_field = crate::NativeApp::Notes(native.clone()).text_field(theme.mobile());
            assert_eq!(
                native_field,
                web.text_field(),
                "{theme:?} step {step}: {what}"
            );
        }
    }
}

/// A snapshot keeps the declared state; a restored window boots the same code with it
/// and shows and announces what the live one does.
#[test]
fn a_restored_window_shows_what_the_live_one_does() {
    let mut disk = disk_with(&["a.txt", "b.txt"]);
    let mut app = web_notes(&mut disk, DesktopTheme::Macos);
    let effects = app.click(1, "notes:open:b.txt", 0).unwrap();
    disk.web(&mut app, effects).unwrap();
    app.text_effects(1, " and more").unwrap();
    let json = serde_json::to_string(&app).unwrap();
    let restored: WebApp = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, app);
    let page = |a: &WebApp| {
        let mut page = cw_protocol::Page::new("Notes");
        a.page(&mut page);
        page
    };
    assert_eq!(page(&restored), page(&app));
    let (a, b) = (
        scene(&app, DesktopTheme::Macos),
        scene(&restored, DesktopTheme::Macos),
    );
    assert_eq!(a.nodes, b.nodes);
    assert_eq!(restored.text_field(), app.text_field());
    // And both go on alike.
    let mut restored = restored;
    for a in [&mut app, &mut restored] {
        a.text_effects(1, "!").unwrap();
        a.key(1, "Backspace", 0).unwrap();
        a.key(1, "Backspace", 0).unwrap();
    }
    assert_eq!(restored.state(), app.state());
    assert_eq!(
        scene(&app, DesktopTheme::Macos).nodes,
        scene(&restored, DesktopTheme::Macos).nodes
    );
}

/// A copy taken before the live window moved on keeps what it had, and the live one
/// is not disturbed by the copy being used.
#[test]
fn a_copy_left_behind_keeps_its_own_state() {
    let mut disk = disk_with(&["a.txt"]);
    let mut app = web_notes(&mut disk, DesktopTheme::Macos);
    let before = app.clone();
    let effects = app.click(1, "notes:open:a.txt", 0).unwrap();
    disk.web(&mut app, effects).unwrap();
    assert_eq!(before.state()["open"], serde_json::Value::Null);
    let mut page = cw_protocol::Page::new("Notes");
    before.page(&mut page);
    assert!(!page
        .elements
        .iter()
        .any(|e| matches!(e, cw_protocol::PageElement::Input { .. })));
    let mut page = cw_protocol::Page::new("Notes");
    app.page(&mut page);
    assert!(page.elements.iter().any(
        |e| matches!(e, cw_protocol::PageElement::Input { value, .. } if value == "text of a.txt")
    ));
    app.text_effects(1, "!").unwrap();
    assert_eq!(app.state()["text"], "text of a.txt!");
}

/// A long session is folded back into its declared state without changing anything
/// the window shows.
#[test]
fn a_long_session_is_compacted_from_declared_state() {
    let mut disk = disk_with(&["a.txt"]);
    let mut app = web_notes(&mut disk, DesktopTheme::Macos);
    let effects = app.click(1, "notes:open:a.txt", 0).unwrap();
    disk.web(&mut app, effects).unwrap();
    for _ in 0..(COMPACT_AFTER / 2 + 10) {
        app.text_effects(1, "x").unwrap();
    }
    let weight = {
        let local = lock(&app.local);
        let cell = lock(&local.cell);
        cell.runtime.as_ref().map(|r| r.weight())
    };
    assert!(weight.is_none_or(|w| w < COMPACT_AFTER), "{weight:?}");
    assert_eq!(
        app.state()["text"].as_str().unwrap().len(),
        "text of a.txt".len() + COMPACT_AFTER / 2 + 10
    );
    assert_eq!(app.text_field().as_deref(), Some("notes:body"));
}

/// Scroll containers with ids are the window's panes: their offsets come from the
/// window's `Scroll`, clamped to the content, with the platform's scroll bar.
#[test]
fn a_long_list_is_a_pane_of_the_window() {
    let names: Vec<String> = (0..40).map(|i| format!("note-{i:02}.txt")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut disk = disk_with(&refs);
    let app = web_notes(&mut disk, DesktopTheme::Macos);
    let e = env(DesktopTheme::Macos, 900, 600);
    let mut p = Painter::themed(DesktopTheme::Macos, 900, 600, 1 << 52);
    p.scroll.set("list", 100_000);
    app.render(&mut p, &e);
    let list = p
        .scene
        .scrolls
        .iter()
        .find(|a| a.target == "pane:list")
        .expect("the list is a pane");
    assert_eq!(list.offset, list.max_offset(), "clamped to the end");
    assert!(p.scene.nodes.iter().any(|n| n
        .interaction
        .as_deref()
        .is_some_and(|i| i.starts_with("pane:list:"))));
    let last = p
        .scene
        .nodes
        .iter()
        .find(|n| n.interaction.as_deref() == Some("notes:open:note-39.txt"))
        .expect("the last row is painted");
    let b = last.transform.bounds(last.bounds);
    assert!(b.bottom() <= 600, "{b:?}");
}
