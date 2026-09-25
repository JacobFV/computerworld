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

/// Content stills of Notes on every platform, for a person to look at:
/// `CW_STILLS_DIR=/tmp/x cargo test -p cw-applications stills -- --ignored`.
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
        let (mut web, _) = WebApp::launch("notes", "/home/alice/Notes", 1, 0, theme).unwrap();
        web.tree_listed(1, "/home/alice/Notes", Ok(names.clone()))
            .unwrap();
        let shoot = |label: &str, web: &WebApp| {
            let state = crate::AppState::Native(crate::NativeApp::Web(web.clone()));
            let scene = crate::desktop_scene::app_content_with(&state, &e);
            write_png(
                &dir.join(format!("{}-{label}.png", theme.platform())),
                &scene,
            );
        };
        shoot("list", &web);
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
        shoot("open", &web);
        web.text_effects(1, " More").unwrap();
        shoot("dirty", &web);
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
    let (mut app, effects) =
        WebApp::launch(react_notes(), FOLDER, 1, 0, DesktopTheme::Macos).unwrap();
    disk.web(&mut app, effects).unwrap();
    let effects = app.click(1, "notes:open:a.txt", 0).unwrap();
    disk.web(&mut app, effects).unwrap();
    let weight = |app: &WebApp| {
        let local = lock(&app.local);
        let cell = lock(&local.cell);
        cell.runtime.as_ref().map(|r| r.weight())
    };
    let mut folded = false;
    let typed = COMPACT_AFTER / 2;
    for _ in 0..typed {
        app.text_effects(1, "x").unwrap();
        let w = weight(&app);
        assert!(w.is_none_or(|w| w <= COMPACT_AFTER + 64), "{w:?}");
        folded |= w.is_none();
    }
    assert!(folded, "the journal was folded into declared state");
    assert_eq!(
        app.state()["text"].as_str().unwrap().len(),
        "text of a.txt".len() + typed
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

/// One recorded input to Notes and what the native Notes did with it.
#[derive(serde::Serialize, serde::Deserialize)]
struct TraceStep {
    /// `click <id>`, `type <text>` or `key <key>`.
    input: String,
    clock: u64,
    ok: bool,
    state: serde_json::Value,
    files: std::collections::BTreeMap<String, String>,
    controls: std::collections::BTreeSet<String>,
    field: Option<String>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Trace {
    platform: String,
    steps: Vec<TraceStep>,
}

/// The platforms `painter-trace.json` was recorded on.
const TRACE_THEMES: [DesktopTheme; 5] = [
    DesktopTheme::Macos,
    DesktopTheme::Ios,
    DesktopTheme::Windows,
    DesktopTheme::Android,
    DesktopTheme::Ubuntu,
];

fn theme_named(platform: &str) -> DesktopTheme {
    TRACE_THEMES
        .into_iter()
        .find(|t| t.platform() == platform)
        .unwrap()
}

/// The web Notes does with every recorded input what the Painter Notes it replaced
/// did: the same controls on screen, the same outcome, declared state, files on disk
/// and text focus. `painter-trace.json` was recorded from the Painter Notes (80 random
/// inputs on each platform, against a disk holding `a.txt` and `b.txt`) by a recorder
/// removed with it; see the commit that made Notes a web application.
#[test]
fn the_web_notes_does_what_the_painter_notes_did() {
    let traces: Vec<Trace> =
        serde_json::from_str(include_str!("../../web/notes/painter-trace.json")).unwrap();
    assert_eq!(traces.len(), TRACE_THEMES.len());
    for trace in traces {
        let theme = theme_named(&trace.platform);
        let (w, h) = if theme.mobile() {
            (390, 760)
        } else {
            (900, 600)
        };
        let e = env(theme, w, h);
        let mut disk = disk_with(&["a.txt", "b.txt"]);
        let mut web = web_notes(&mut disk, theme);
        for (index, step) in trace.steps.iter().enumerate() {
            let scene = crate::desktop_scene::app_content_with(
                &crate::AppState::Native(crate::NativeApp::Web(web.clone())),
                &e,
            );
            let before = controls(&scene);
            let expected_before = match index {
                0 => None,
                i => Some(&trace.steps[i - 1].controls),
            };
            if let Some(expected) = expected_before {
                assert_eq!(&before, expected, "{} step {index}", trace.platform);
            }
            let (kind, arg) = step.input.split_once(' ').unwrap();
            let ok = match kind {
                "click" => web
                    .click(1, arg, step.clock)
                    .and_then(|e| disk.web(&mut web, e))
                    .is_ok(),
                "type" => web
                    .text_effects(1, arg)
                    .and_then(|e| disk.web(&mut web, e))
                    .is_ok(),
                _ => web
                    .key(1, arg, step.clock)
                    .and_then(|e| disk.web(&mut web, e))
                    .is_ok(),
            };
            let at = format!("{} step {index}: {}", trace.platform, step.input);
            assert_eq!(ok, step.ok, "{at}; console {:?}", web.console());
            assert_eq!(web.state(), &step.state, "{at}");
            assert_eq!(disk.files, step.files, "{at}");
            assert_eq!(web.text_field(), step.field, "{at}");
        }
    }
}

/// The desktop launches Notes for its own platform and routes what Notes asks for,
/// and the answers, through the window.
#[test]
fn the_desktop_runs_notes_as_a_web_application() {
    let mut d = crate::DesktopState {
        theme: Some(DesktopTheme::Ios),
        ..Default::default()
    };
    let (id, effects) = d.launch("notes", "/Users/alice/Notes").unwrap();
    assert_eq!(
        effects,
        vec![AppEffect::ListTree {
            window: id,
            path: "/Users/alice/Notes".into(),
            depth: 1
        }]
    );
    d.tree_listed(id, "/Users/alice/Notes", 1, Err("folder not found".into()))
        .unwrap();
    // A tap on a phone is one click, whatever it changes.
    d.activate("notes:new").unwrap();
    let crate::AppState::Native(app) = &d.windows[&id].state else {
        panic!("Notes is an application window");
    };
    let state = app.web().unwrap().state();
    assert_eq!(state["open"], "note-0.txt");
    assert_eq!(state["problem"], "folder not found");
    assert_eq!(
        app.phone_back(DesktopTheme::Ios).as_deref(),
        Some("notes:close")
    );
    // The body is not focused on a phone until it is tapped.
    assert_eq!(
        app.text_field(true),
        Some("notes:body".into()),
        "a new note is being edited"
    );
    let effects = d.key("Ctrl+s").unwrap();
    assert!(
        matches!(&effects[1], AppEffect::WriteFile { path, .. } if path == "/Users/alice/Notes/note-0.txt")
    );
    let more = d
        .file_written(id, "/Users/alice/Notes/note-0.txt", "")
        .unwrap();
    assert!(matches!(&more[..], [AppEffect::ListTree { .. }]));
}

fn compiled_counter() -> &'static str {
    define(cw_sdk::WebApplication {
        kind: "compiled-counter".into(),
        version: 1,
        titles: Default::default(),
        source: cw_sdk::WebSource::Compiled {
            ir: include_str!("fixtures/counter.ui.json").into(),
            script: include_str!("fixtures/counter.js").into(),
            style: String::new(),
        },
    })
    .unwrap();
    "compiled-counter"
}

fn page_of(app: &WebApp) -> Vec<cw_protocol::PageElement> {
    let mut page = cw_protocol::Page::new("t");
    app.page(&mut page);
    page.elements
}

fn text_of(app: &WebApp, id: &str) -> String {
    page_of(app)
        .into_iter()
        .find_map(|e| match e {
            cw_protocol::PageElement::Text { id: i, text } if i == id => Some(text),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text {id}"))
}

/// An app inside the compiled subset runs on cw-ui, behind the same host: clicks
/// by id, typing into the focused field, the semantic page, and a snapshot that is
/// cw-ui's own state, so a restored window is exactly the one it was taken from.
#[test]
fn a_compiled_app_runs_on_cw_ui_and_restores_exactly() {
    let kind = compiled_counter();
    let (mut app, effects) = WebApp::launch(kind, "", 1, 0, DesktopTheme::Macos).unwrap();
    assert!(effects.is_empty());
    {
        let local = lock(&app.local);
        let cell = lock(&local.cell);
        assert_eq!(
            cell.runtime.as_ref().unwrap().weight(),
            0,
            "on cw-ui, not the VM"
        );
    }
    assert_eq!(text_of(&app, "counter-value"), "0");
    app.click(1, "counter:add", 0).unwrap();
    app.click(1, "counter:add", 0).unwrap();
    assert_eq!(text_of(&app, "counter-value"), "2");
    app.click(1, "counter:name", 0).unwrap();
    assert_eq!(app.text_field().as_deref(), Some("counter:name"));
    app.text_effects(1, "Ada").unwrap();
    assert_eq!(text_of(&app, "counter-greeting"), "Hello Ada");
    // The window's state is cw-ui's snapshot of the app.
    let json = serde_json::to_string(&app).unwrap();
    let restored: WebApp = serde_json::from_str(&json).unwrap();
    assert_eq!(page_of(&restored), page_of(&app));
    assert_eq!(restored.text_field(), app.text_field());
    let (a, b) = (
        scene(&app, DesktopTheme::Macos),
        scene(&restored, DesktopTheme::Macos),
    );
    assert_eq!(a.nodes, b.nodes);
    let mut restored = restored;
    restored.click(1, "counter:add", 0).unwrap();
    assert_eq!(text_of(&restored, "counter-value"), "3");
    assert_eq!(text_of(&app, "counter-value"), "2");
}

/// A restored window boots before it knows the platform it is on; once painted
/// there it has the focus the live window has, which on a phone means the body of a
/// note that was opened but not tapped does not take the keyboard.
#[test]
fn a_restored_phone_window_takes_the_focus_the_live_one_has() {
    let mut disk = disk_with(&["a.txt"]);
    let mut app = web_notes(&mut disk, DesktopTheme::Ios);
    let e = env(DesktopTheme::Ios, 390, 760);
    let mut p = Painter::themed(DesktopTheme::Ios, 390, 760, 1 << 52);
    app.render(&mut p, &e);
    let effects = app.click(1, "notes:open:a.txt", 0).unwrap();
    disk.web(&mut app, effects).unwrap();
    assert_eq!(app.text_field(), None, "not tapped yet");
    let restored: WebApp = serde_json::from_str(&serde_json::to_string(&app).unwrap()).unwrap();
    let mut p = Painter::themed(DesktopTheme::Ios, 390, 760, 1 << 52);
    restored.render(&mut p, &e);
    assert_eq!(restored.text_field(), None);
    let mut restored = restored;
    restored.click(1, "notes:body", 0).unwrap();
    assert_eq!(restored.text_field().as_deref(), Some("notes:body"));
}

/// Notes is inside cw-tsx's compiled subset and its IR mounts on cw-ui, so the
/// desktop runs it with no VM; the React fallback is only for a cw-ui that cannot
/// load it, and would otherwise hide a Notes that stopped compiling.
#[test]
fn notes_runs_on_cw_ui() {
    let entry = catalog::get("notes").unwrap();
    let WebSource::Compiled { ir, style, .. } = &entry.app.source else {
        panic!("Notes ships without its IR");
    };
    let e = env_for(DesktopTheme::Macos, 900, 600);
    let boot = Boot {
        kind: "notes",
        argument: FOLDER,
        state: None,
        env: &e,
    };
    let mut runtime = UiRuntime::boot(ir, style, &boot, None, 0).unwrap();
    let out = runtime.drain();
    assert!(
        matches!(&out.requests[..], [(1, Request::List { path })] if path == FOLDER),
        "{:?}",
        out.requests
    );
    assert_eq!(out.state.unwrap()["folder"], FOLDER);
}

/// Notes on React on the VM, from the fallback script cw-tsx builds.
fn react_notes() -> &'static str {
    let entry = catalog::get("notes").unwrap();
    let WebSource::Compiled { script, style, .. } = &entry.app.source else {
        panic!("Notes ships its IR and fallback");
    };
    define(cw_sdk::WebApplication {
        kind: "notes-on-react".into(),
        version: 1,
        titles: Default::default(),
        source: WebSource::Script {
            script: script.clone(),
            style: style.clone(),
            react: true,
        },
    })
    .unwrap();
    "notes-on-react"
}

/// A window saved while a request it made is outstanding keeps the request and the
/// code awaiting it: the restored window takes the machine's answer where the live
/// one would have, on either backend.
#[test]
fn a_window_saved_mid_request_takes_the_answer_after_a_restore() {
    for kind in ["notes", react_notes()] {
        let mut disk = disk_with(&["a.txt", "b.txt"]);
        let (mut app, effects) = WebApp::launch(kind, FOLDER, 1, 0, DesktopTheme::Macos).unwrap();
        disk.web(&mut app, effects).unwrap();
        let quiet = serde_json::to_value(&app).unwrap();
        assert!(
            quiet.get("inflight").is_none(),
            "{kind}: nothing outstanding"
        );
        let effects = app.click(1, "notes:open:b.txt", 0).unwrap();
        let [AppEffect::ReadFiles { tag, .. }] = &effects[..] else {
            panic!("{kind}: {effects:?}");
        };
        // Saved with the read outstanding, and restored twice.
        let json = serde_json::to_string(&app).unwrap();
        assert!(json.contains("\"inflight\""), "{kind}");
        let mut copies: Vec<WebApp> = (0..2)
            .map(|_| serde_json::from_str(&json).unwrap())
            .collect();
        copies.push(app);
        for copy in &mut copies {
            let more = copy
                .files_read(
                    1,
                    tag,
                    vec![(format!("{FOLDER}/b.txt"), Ok("text of b".into()))],
                )
                .unwrap_or_else(|e| panic!("{kind}: {e}"));
            assert!(more.is_empty());
            assert_eq!(copy.state()["text"], "text of b", "{kind}");
            assert_eq!(copy.text_field().as_deref(), Some("notes:body"), "{kind}");
            let saved = serde_json::to_value(&*copy).unwrap();
            assert!(saved.get("inflight").is_none(), "{kind}: answered");
        }
        assert_eq!(page_of(&copies[0]), page_of(&copies[2]), "{kind}");
        // A clone taken mid-request and left behind keeps its own.
        let effects = copies[0].click(1, "notes:open:a.txt", 0).unwrap();
        let [AppEffect::ReadFiles { tag, .. }] = &effects[..] else {
            panic!("{kind}: {effects:?}");
        };
        let behind = copies[0].clone();
        copies[0]
            .files_read(
                1,
                tag,
                vec![(format!("{FOLDER}/a.txt"), Ok("first".into()))],
            )
            .unwrap();
        let mut behind = behind;
        behind
            .files_read(
                1,
                tag,
                vec![(format!("{FOLDER}/a.txt"), Ok("second".into()))],
            )
            .unwrap();
        assert_eq!(copies[0].state()["text"], "first", "{kind}");
        assert_eq!(behind.state()["text"], "second", "{kind}");
    }
}
