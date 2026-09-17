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
#[test]
fn files_project_real_names_and_only_supported_interactions() {
    for theme in THEMES {
        let scene = app_content(
            &AppState::Files {
                path: "/work".into(),
                entries: vec!["invoices/".into(), "budget.txt".into()],
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
        assert!(actions.contains(&"files-root"));
        assert!(actions.contains(&"files-up"));
        assert!(actions.contains(&"open:0"));
        assert!(actions.contains(&"open:1"));
        assert!(actions
            .iter()
            .all(|a| matches!(*a, "files-root" | "files-up" | "open:0" | "open:1")));
        for n in &scene.nodes {
            if let Some(s) = &n.semantic {
                if s.disabled {
                    assert!(n.interaction.is_none());
                    assert!(!s.focusable);
                }
            }
        }
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
        assert!(scene
            .nodes
            .iter()
            .any(|n| n.interaction.as_deref() == Some("editor-save")));
        assert!(scene
            .nodes
            .iter()
            .any(|n| n.interaction.as_deref() == Some("editor-text")
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
        let output = (0..100).map(|i| format!("line {i}\n")).collect::<String>();
        let scene = app_content(
            &AppState::Terminal {
                input: "pwd".into(),
                output,
                history: Vec::new(),
            },
            theme,
            800,
            300,
        );
        assert!(scene
            .nodes
            .iter()
            .any(|n| matches!(&n.primitive,Primitive::Text{text,..}if text=="line 99")));
        assert!(!scene
            .nodes
            .iter()
            .any(|n| matches!(&n.primitive,Primitive::Text{text,..}if text=="line 0")));
        assert_eq!(
            scene.hit_test(150, 100).unwrap().interaction.as_deref(),
            Some("terminal-input")
        );
    }
}
