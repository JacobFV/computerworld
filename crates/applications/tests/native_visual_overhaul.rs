use cw_applications::{
    desktop_scene::{app_content, DesktopTheme},
    AppState,
};
use cw_scene::Primitive;
const THEMES: [DesktopTheme; 5] = [
    DesktopTheme::Macos,
    DesktopTheme::Windows,
    DesktopTheme::Ubuntu,
    DesktopTheme::Ios,
    DesktopTheme::Android,
];
/// The whole no-fake-controls invariant, over every state the file manager can paint:
/// nothing selected, something selected with a clipboard behind it, sorted the other
/// way, filtered, in a grid, and in Recents. Every id painted in any of them has to be
/// one `DesktopState::click` accepts against a desktop in that same state.
#[test]
fn files_project_real_names_and_only_supported_interactions() {
    let mut tab = cw_applications::FileTab::new("/work");
    tab.entries = vec!["invoices/".into(), "budget.txt".into()];
    let clipboard = cw_applications::Clipboard::new(vec!["/work/budget.txt".into()], false);
    let states: Vec<(&str, cw_applications::FileTab)> = vec![
        ("plain", tab.clone()),
        ("selected", {
            let mut t = tab.clone();
            t.selected = Some(1);
            t
        }),
        ("sorted", {
            let mut t = tab.clone();
            t.sort = cw_applications::SortKey::Kind;
            t.descending = true;
            t
        }),
        ("filtered", {
            let mut t = tab.clone();
            t.query = "bud".into();
            t.searching = true;
            t
        }),
        ("grid", {
            let mut t = tab.clone();
            t.view = cw_applications::FileView::Grid;
            t
        }),
        ("renaming", {
            let mut t = tab.clone();
            t.selected = Some(1);
            t.rename = Some(cw_applications::Rename {
                from: "budget.txt".into(),
                name: "budget".into(),
            });
            t
        }),
        ("recents", {
            let mut t = tab.clone();
            t.scope = cw_applications::FileScope::Recents;
            t.entries = vec!["/work/budget.txt".into()];
            t
        }),
    ];
    for theme in THEMES {
        for (name, tab) in &states {
            let state = AppState::Files {
                tabs: vec![tab.clone()],
                active: 0,
            };
            let scene = cw_applications::desktop_scene::app_content_with(
                &state,
                &cw_applications::AppEnv {
                    theme,
                    width: 800,
                    height: 500,
                    clock_us: 0,
                    settings: &cw_applications::SystemSettings::DEFAULT,
                    clipboard: Some(&clipboard),
                    share_to: None,
                    files: Default::default(),
                },
            );
            let actions: Vec<_> = scene
                .nodes
                .iter()
                .filter_map(|n| n.interaction.as_deref())
                .collect();
            for action in &actions {
                // A fresh desktop put into exactly the state that was painted: the
                // control has to be real there, not merely real somewhere.
                let mut desktop = cw_applications::DesktopState::default();
                desktop.launch("files", "/work").unwrap();
                desktop.clipboard = Some(clipboard.clone());
                if let Some(target) = desktop
                    .windows
                    .values_mut()
                    .next()
                    .and_then(|w| w.state.file_tab_mut())
                {
                    *target = tab.clone();
                }
                assert!(
                    desktop.click(action).is_ok(),
                    "{theme:?}/{name} paints unhandled control {action}"
                );
            }
            for n in &scene.nodes {
                if let Some(s) = &n.semantic {
                    if s.disabled {
                        assert!(n.interaction.is_none());
                        assert!(!s.focusable);
                    }
                }
            }
        }
        let scene = app_content(
            &AppState::Files {
                tabs: vec![tab.clone()],
                active: 0,
            },
            theme,
            800,
            500,
        );
        let labels: Vec<_> = scene
            .nodes
            .iter()
            .filter_map(|n| n.semantic.as_ref().map(|s| s.label.as_str()))
            .collect();
        assert!(labels.contains(&"budget.txt"));
        let actions: Vec<_> = scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.as_deref())
            .collect();
        // iOS reaches the folder through its Browse tab, which returns to the folder
        // the tab was in rather than jumping to the root; every other shell has a
        // root row in its sidebar or breadcrumb.
        assert!(actions.contains(&"files-root") || actions.contains(&"files-browse"));
        // Finder climbs from its Go menu and Files from its path bar, both drawn by
        // the shell; every other client area carries its own Up.
        assert!(
            actions.contains(&"files-up")
                || matches!(theme, DesktopTheme::Macos | DesktopTheme::Ubuntu)
        );
        assert!(actions.contains(&"open:0"));
        assert!(actions.contains(&"open:1"));
        let row = scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some("open:1"))
            .unwrap();
        assert_eq!(
            scene
                .hit_test(row.bounds.x + 3, row.bounds.y + 3)
                .unwrap()
                .interaction
                .as_deref(),
            Some("open:1")
        );
    }
}

/// The question sorting raises: `open:<i>` is a row on screen, so reversing the order
/// must move what a click selects. Anything else opens the file next to the one the
/// pointer was over, which is the one bug a file manager may never have.
#[test]
fn click_targets_follow_the_displayed_order_not_the_listing() {
    let mut desktop = cw_applications::DesktopState::default();
    desktop.launch("files", "/work").unwrap();
    let id = *desktop.windows.keys().next().unwrap();
    desktop
        .directory_loaded(
            id,
            0,
            vec!["alpha.txt".into(), "beta.txt".into(), "gamma.txt".into()],
        )
        .unwrap();
    let selection = |d: &cw_applications::DesktopState| {
        d.windows[&id]
            .state
            .file_tab()
            .unwrap()
            .selection()
            .cloned()
            .unwrap()
    };
    desktop.click("open:0").unwrap();
    assert_eq!(selection(&desktop), "alpha.txt");
    // Same row, reversed order: the row is now the last entry of the raw listing.
    desktop.click("files-sort:name").unwrap();
    desktop.click("open:0").unwrap();
    assert_eq!(selection(&desktop), "gamma.txt");
    // And the painted row at that index agrees with what the click selected.
    let scene = app_content(&desktop.windows[&id].state, DesktopTheme::Windows, 800, 500);
    let row = scene
        .nodes
        .iter()
        .find(|n| n.interaction.as_deref() == Some("open:0"))
        .unwrap();
    assert_eq!(row.semantic.as_ref().unwrap().label, "gamma.txt");
    // A filter renumbers the rows the same way.
    desktop.click("files-sort:name").unwrap();
    desktop.click("files-search").unwrap();
    desktop.text("bet").unwrap();
    desktop.click("open:0").unwrap();
    assert_eq!(selection(&desktop), "beta.txt");
    assert!(desktop.click("open:1").is_err());
}
#[test]
fn editors_are_deterministic_handle_unicode_and_expose_save() {
    let state = AppState::Editor {
        path: "/work/notes.txt".into(),
        text: "Résumé\nαβγ\nNotes".into(),
        cursor: 2,
        dirty: true,
    };
    for theme in THEMES {
        let scene = app_content(&state, theme, 800, 500);
        assert_eq!(scene, app_content(&state, theme, 800, 500));
        // Phones paint Save; desktop editors keep it in their menus.
        assert_eq!(
            scene
                .nodes
                .iter()
                .any(|n| n.interaction.as_deref() == Some("editor-save")),
            theme.mobile(),
            "{theme:?}"
        );
        assert!(scene.nodes.iter().any(|n| n
            .interaction
            .as_deref()
            .is_some_and(|i| i.starts_with("editor-text"))
            && n.semantic.as_ref().unwrap().role == "textbox"));
        assert!(scene
            .nodes
            .iter()
            .any(|n| matches!(&n.primitive,Primitive::Text{text,..}if text=="Résumé")));
        // Narrow clients and stale cursor offsets must remain safe to project.
        app_content(
            &AppState::Editor {
                path: String::new(),
                text: "é".into(),
                cursor: usize::MAX,
                dirty: false,
            },
            theme,
            1,
            1,
        );
    }
}
#[test]
fn terminal_keeps_latest_real_output_and_focusable_input() {
    for theme in THEMES {
        let mut transcript = (0..100)
            .map(|i| {
                cw_applications::TerminalEntry::new(
                    "alice@box:/home/alice$",
                    "echo",
                    &format!("line {i}"),
                    "",
                    0,
                )
            })
            .collect::<Vec<_>>();
        transcript.push(cw_applications::TerminalEntry::new(
            "alice@box:/home/alice$",
            "nope",
            "",
            "nope: not found",
            127,
        ));
        let scene = app_content(
            &AppState::Terminal {
                input: "pwd".into(),
                prompt: "alice@box:/home/alice$".into(),
                transcript,
                history: Vec::new(),
                cursor: 3,
                scroll: 0,
            },
            theme,
            800,
            300,
        );
        let text = |want: &str| {
            scene
                .nodes
                .iter()
                .any(|n| matches!(&n.primitive,Primitive::Text{text,..}if text==want))
        };
        assert!(text("line 99"));
        // The command is echoed and the failure is a literal, not prose to regex over.
        assert!(text("alice@box:/home/alice$ nope"));
        assert!(text("[exit 127]"));
        // The pending prompt is the machine's, drawn with a trailing space before input.
        assert!(text("alice@box:/home/alice$ "));
        assert!(!text("line 0"));
        assert_eq!(
            scene.hit_test(150, 100).unwrap().interaction.as_deref(),
            Some("terminal-input")
        );
    }
}
