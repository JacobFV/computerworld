use cw_applications::{
    desktop_scene::{app_content, DesktopTheme},
    AppState,
};
use cw_scene::{Primitive, Rect};

#[test]
fn editor_scrolls_to_cursor_and_exposes_real_controls() {
    let text = (0..90).map(|n| format!("line {n}\n")).collect::<String>();
    let state = AppState::Editor {
        path: "/note".into(),
        cursor: text.len(),
        text,
        dirty: true,
    };
    let scene = app_content(&state, DesktopTheme::Macos, 480, 240);
    assert!(scene
        .nodes
        .iter()
        .any(|n| matches!(&n.primitive, Primitive::Text { text, .. } if text == "line 89")));
    assert!(!scene
        .nodes
        .iter()
        .any(|n| matches!(&n.primitive, Primitive::Text { text, .. } if text == "line 0")));
    assert!(scene
        .nodes
        .iter()
        .any(|n| n.interaction.as_deref() == Some("editor-save")));
    let body = scene.hit_test(100, 90).unwrap();
    assert_eq!(body.interaction.as_deref(), Some("editor-text"));
    assert_eq!(body.semantic.as_ref().unwrap().role, "textbox");
}

#[test]
fn projection_tolerates_unaligned_restored_cursor_and_clips_small_views() {
    let state = AppState::Editor {
        path: "/note".into(),
        cursor: 1,
        text: "🦀".into(),
        dirty: false,
    };
    let scene = app_content(&state, DesktopTheme::Android, 80, 60);
    assert!(scene
        .nodes
        .iter()
        .all(|n| n.clip == Some(Rect::new(0, 0, 80, 60))));
    assert_eq!(scene, app_content(&state, DesktopTheme::Android, 80, 60));
}

#[test]
fn file_browser_actions_are_backed_by_visible_entries() {
    let state = AppState::Files {
        path: "/Documents".into(),
        entries: vec!["notes.txt".into(), "projects/".into()],
    };
    let scene = app_content(&state, DesktopTheme::Windows, 700, 400);
    for action in ["files-up", "files-root", "open:0", "open:1"] {
        assert!(
            scene
                .nodes
                .iter()
                .any(|n| n.interaction.as_deref() == Some(action)),
            "missing {action}"
        );
    }
}
