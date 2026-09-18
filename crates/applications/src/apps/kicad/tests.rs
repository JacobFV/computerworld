use super::*;
use crate::desktop_scene::app_content_with;
use crate::AppState;

fn env(theme: DesktopTheme) -> crate::AppEnv<'static> {
    crate::AppEnv {
        theme,
        width: 1400,
        height: 900,
        clock_us: 0,
        settings: &crate::SystemSettings::DEFAULT,
        clipboard: None,
        share_to: None,
        files: Default::default(),
        editor: None,
        pointer: None,
    }
}
fn targets(app: &Kicad, theme: DesktopTheme) -> Vec<String> {
    let scene = app_content_with(
        &AppState::Native(crate::NativeApp::Kicad(app.clone())),
        &env(theme),
    );
    scene
        .nodes
        .iter()
        .filter_map(|n| n.interaction.clone())
        .collect()
}

#[test]
fn every_frame_paints_on_every_desktop_and_its_controls_dispatch() {
    for frame in [
        "",
        "sch|/home/u/Documents/KiCad/p/p.kicad_pro",
        "pcb|/home/u/Documents/KiCad/p/p.kicad_pro",
        "sim|/home/u/Documents/KiCad/p/p.kicad_pro",
    ] {
        let (mut app, _) = Kicad::launch(frame, 1, 0);
        app.files_read(1, "open", vec![]);
        for theme in [
            DesktopTheme::Macos,
            DesktopTheme::Windows,
            DesktopTheme::Ubuntu,
        ] {
            let t = targets(&app, theme);
            assert!(!t.is_empty(), "{frame} paints no controls");
            for target in t {
                assert!(
                    target.starts_with("kicad:"),
                    "{target} is not a KiCad target"
                );
                // Every painted control is a command the app knows (it may refuse for
                // want of a selection, but it must not be unknown).
                let mut probe = app.clone();
                if let Err(e) = probe.click(1, &target, 0) {
                    assert!(!e.contains("unknown"), "{target}: {e}");
                }
            }
        }
    }
}
