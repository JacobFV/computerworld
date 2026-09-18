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
    // TextEdit saves from its menu bar; the document area paints no Save.
    assert!(!scene
        .nodes
        .iter()
        .any(|n| n.interaction.as_deref() == Some("editor-save")));
    let body = scene.hit_test(100, 90).unwrap();
    // The target carries the first visible line, so a click on a scrolled document
    // still resolves to the character under the pointer.
    assert_eq!(body.interaction.as_deref(), Some("editor-text:79"));
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
    let mut tab = cw_applications::FileTab::new("/Documents");
    tab.entries = vec!["notes.txt".into(), "projects/".into()];
    let state = AppState::Files {
        tabs: vec![tab],
        active: 0,
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
