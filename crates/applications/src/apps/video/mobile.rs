//! The phone editors: iMovie on iOS and the Android video editor. Both put the viewer
//! across the top, the playback row under it, the timeline in the middle and a tool bar
//! along the bottom whose tools change with what is selected — a clip's tools (split,
//! speed, volume…) when one is, and the things to add when none is.
use super::widgets::{self, chip, slider, target, text_button, tool, tool_off, Skin};
use super::{Editor, Product, Prop};
use crate::desktop_scene::{shared::Align, Painter};
use crate::AppEnv;
use cw_scene::{Color, Rect};
use cw_video::{MediaKind, Source, TrackKind, TransitionKind};

pub fn render(ed: &Editor, p: &mut Painter, env: &AppEnv<'_>) {
    let s = widgets::skin(ed.product, true);
    let (w, h) = (env.width, env.height);
    p.scene.background = s.bg;
    p.box_(Rect::new(0, 0, w, h), s.bg, 0);
    let ios = ed.product == Product::Imovie;
    // Top bar.
    let top = 50u32;
    if ios {
        text_button(p, &s, Rect::new(10, 10, 64, 30), "Done", "save", false);
    } else {
        tool(
            p,
            &s,
            Rect::new(8, 10, 32, 30),
            "folder",
            "Open project",
            "open",
            false,
            false,
        );
        tool(
            p,
            &s,
            Rect::new(44, 10, 32, 30),
            "download",
            "Save project",
            "save",
            false,
            false,
        );
    }
    p.label(
        80,
        16,
        w.saturating_sub(220),
        &ed.project.name,
        14,
        s.text,
        true,
        Align::Center,
    );
    let undo = Rect::new(w as i32 - 116, 10, 32, 30);
    if ed.undo.is_empty() {
        tool_off(p, &s, undo, "undo", "Nothing to undo");
    } else {
        tool(p, &s, undo, "undo", "Undo", "undo", false, false);
    }
    let export = Rect::new(w as i32 - 78, 10, 68, 30);
    if ed.project.duration() == 0 || ed.export.is_some() {
        widgets::text_button_off(
            p,
            &s,
            export,
            if ios { "Share" } else { "Export" },
            if ed.export.is_some() {
                "An export is running"
            } else {
                "The movie is empty"
            },
        );
    } else {
        text_button(
            p,
            &s,
            export,
            if ios { "Share" } else { "Export" },
            "export",
            true,
        );
    }
    // Viewer.
    let vh = w * 9 / 16;
    let viewer = Rect::new(0, top as i32, w, vh);
    widgets::monitor(p, ed, env, viewer, &s);
    // Playback row: time, play, add media.
    let row_y = viewer.y + vh as i32;
    let row = Rect::new(0, row_y, w, 48);
    p.box_(row, s.bar, 0);
    p.left(
        12,
        row_y + 15,
        130,
        &widgets::clock_text(ed, env),
        12,
        s.text,
    );
    let playing = ed.play.is_some();
    let play = Rect::new(w as i32 / 2 - 18, row_y + 6, 36, 36);
    if ed.project.duration() == 0 {
        tool_off(p, &s, play, "play", "The movie is empty");
    } else {
        tool(
            p,
            &s,
            play,
            if playing { "pause" } else { "play" },
            if playing { "Pause" } else { "Play" },
            "play",
            false,
            false,
        );
    }
    let add = Rect::new(w as i32 - 46, row_y + 7, 34, 34);
    p.button(add, s.raised, 17, &target("import"), "Add media");
    p.symbol("plus", add.x + 9, add.y + 9, 16, s.accent);
    // Bottom tool bar, and the tool page above it.
    let bar_h = 72u32;
    let bar_y = h as i32 - bar_h as i32;
    let tl_y = row_y + 48;
    let clip_page = ed.selected.is_some() && !ed.inspector.is_empty();
    let page_h = if clip_page || is_add_tab(ed) {
        (h / 3).min(260)
    } else {
        0
    };
    let tl_h = (bar_y - tl_y - page_h as i32).max(60) as u32;
    let lanes_r = Rect::new(0, tl_y, w, tl_h);
    let g = widgets::lanes(ed, env, lanes_r, 0, 18, 44);
    widgets::timeline(p, ed, env, lanes_r, &s, g, 0);
    if page_h > 0 {
        let page = Rect::new(0, bar_y - page_h as i32, w, page_h);
        p.box_(page, s.panel, 0);
        p.hline(0, page.y, w, s.line);
        tool_page(p, ed, env, &s, page, g);
    }
    let bar = Rect::new(0, bar_y, w, bar_h);
    p.box_(bar, s.bar, 0);
    p.hline(0, bar_y, w, s.line);
    let tools = tools(ed);
    let tw = w / tools.len().max(1) as u32;
    for (i, (id, symbol, label, command)) in tools.iter().enumerate() {
        let r = Rect::new(i as i32 * tw as i32, bar_y + 6, tw, bar_h - 12);
        let on = ed.inspector == *id || ed.tab == *id;
        p.button(
            r,
            if on { s.raised } else { Color::TRANSPARENT },
            s.radius,
            &target(command),
            label,
        );
        p.symbol(
            symbol,
            r.x + (tw as i32 - 22) / 2,
            r.y + 8,
            22,
            if on { s.accent } else { s.text },
        );
        p.label(r.x, r.y + 36, tw, label, 10, s.text, false, Align::Center);
    }
    if !ed.status.is_empty() {
        p.label(
            0,
            tl_y + tl_h as i32 - 18,
            w,
            &ed.status,
            10,
            s.muted,
            false,
            Align::Center,
        );
    }
    widgets::progress(p, ed, &s, Rect::new(10, top as i32 + 8, w - 20, 40));
    widgets::sheet(p, ed, env, &s);
}

fn is_add_tab(ed: &Editor) -> bool {
    ed.selected.is_none()
        && matches!(
            ed.tab.as_str(),
            "titles" | "transitions" | "backgrounds" | "overlay"
        )
}

/// The bottom bar's tools: a clip's when one is selected, else what can be added.
fn tools(ed: &Editor) -> Vec<(&'static str, &'static str, &'static str, String)> {
    let ios = ed.product == Product::Imovie;
    let t = |id: &'static str, symbol: &'static str, label: &'static str, command: String| {
        (id, symbol, label, command)
    };
    match ed.selected_clip() {
        Some(c) => {
            let audio = ed.project.track(c.track).map(|t| t.kind) == Some(TrackKind::Audio);
            let title = matches!(c.source, Source::Title(_));
            let mut v = vec![t(
                "actions",
                "scissors",
                if ios { "Actions" } else { "Split" },
                "inspector:actions".into(),
            )];
            if !title {
                v.push(t("speed", "dial", "Speed", "inspector:speed".into()));
            }
            if audio {
                v.push(t("audio", "volume", "Volume", "inspector:audio".into()));
            }
            if title {
                v.push(t(
                    "title",
                    "text-tool",
                    if ios { "Titles" } else { "Text" },
                    "inspector:title".into(),
                ));
            }
            if !ios && !audio {
                v.push(t(
                    "transform",
                    "move",
                    "Animation",
                    "inspector:transform".into(),
                ));
                v.push(t("color", "sliders", "Adjust", "inspector:color".into()));
            }
            v.push(t("fade", "contrast", "Fade", "inspector:fade".into()));
            v.push(t("done", "check", "Done", "deselect".into()));
            v
        }
        None => {
            let mut v = vec![
                t(
                    "titles",
                    "text-tool",
                    if ios { "Titles" } else { "Text" },
                    "tab:titles".into(),
                ),
                t(
                    "transitions",
                    "layers",
                    "Transitions",
                    "tab:transitions".into(),
                ),
                t(
                    "backgrounds",
                    "palette",
                    "Backgrounds",
                    "tab:backgrounds".into(),
                ),
            ];
            if !ios {
                v.push(t("overlay", "image", "Overlay", "tab:overlay".into()));
            }
            v.push(t("open", "folder", "Projects", "open".into()));
            v
        }
    }
}

fn tool_page(p: &mut Painter, ed: &Editor, env: &AppEnv<'_>, s: &Skin, r: Rect, g: widgets::Lanes) {
    let x = r.x + 16;
    let w = r.width - 32;
    let mut y = r.y + 12;
    if ed.selected.is_none() {
        match ed.tab.as_str() {
            "titles" => {
                for (style, label) in [
                    ("plain", "Standard"),
                    ("headline", "Headline"),
                    ("lower", "Lower third"),
                    ("credits", "Ending"),
                ] {
                    text_button(
                        p,
                        s,
                        Rect::new(x, y, w, 36),
                        label,
                        &format!("add-title:{style}"),
                        false,
                    );
                    y += 42;
                }
            }
            "transitions" => {
                let site = ed.transition_site().is_ok();
                if !site {
                    p.left(
                        x,
                        y,
                        w,
                        "Tap between two clips that touch to add a transition",
                        12,
                        s.muted,
                    );
                    return;
                }
                let kinds: &[TransitionKind] = if ed.product == Product::Imovie {
                    &[
                        TransitionKind::CrossDissolve,
                        TransitionKind::Slide,
                        TransitionKind::Wipe,
                        TransitionKind::DipToBlack,
                    ]
                } else {
                    &TransitionKind::ALL
                };
                for kind in kinds {
                    text_button(
                        p,
                        s,
                        Rect::new(x, y, w, 34),
                        ed.product.transition_name(*kind),
                        &format!("add-transition:{}", kind.id()),
                        false,
                    );
                    y += 40;
                }
            }
            "backgrounds" => {
                let mut bx = x;
                for hex in ["000000", "ffffff", "1e3a8a", "0f766e", "b91c1c", "f59e0b"] {
                    let color = cw_raster::parse_hex(hex)
                        .map(widgets::rgba)
                        .unwrap_or(Color::BLACK);
                    p.button(
                        Rect::new(bx, y, 48, 48),
                        color,
                        10,
                        &target(&format!("add-color:{hex}")),
                        &format!("Add #{hex} background"),
                    );
                    bx += 56;
                }
            }
            "overlay" => {
                let pictures: Vec<_> = ed
                    .project
                    .media
                    .iter()
                    .filter(|m| m.kind != MediaKind::Audio && ed.library.contains_key(&m.id))
                    .collect();
                if pictures.is_empty() {
                    p.left(
                        x,
                        y,
                        w,
                        "Add a clip with + first; overlays lay it over the movie",
                        12,
                        s.muted,
                    );
                    return;
                }
                for m in pictures.into_iter().take(5) {
                    let name = m.path.rsplit('/').next().unwrap_or(&m.path);
                    text_button(
                        p,
                        s,
                        Rect::new(x, y, w, 34),
                        &format!("Overlay {name}"),
                        &format!("overlay:{}", m.id),
                        false,
                    );
                    y += 40;
                }
            }
            _ => {}
        }
        let _ = g;
        return;
    }
    match ed.inspector.as_str() {
        "actions" => {
            let now = ed.now(env.clock_us);
            let can_split = ed
                .selected_clip()
                .is_some_and(|c| c.start < now && now < c.end());
            let mut bx = x;
            for (label, cmd, ok) in [("Split", "split", can_split), ("Delete", "delete", true)] {
                let r2 = Rect::new(bx, y, 100, 36);
                if ok {
                    text_button(p, s, r2, label, cmd, false);
                } else {
                    widgets::text_button_off(p, s, r2, label, "Move the playhead inside the clip");
                }
                bx += 108;
            }
        }
        "speed" => {
            y = slider(p, ed, env, s, x, y, w, "Speed", Prop::Speed, "");
            if ed.product != Product::Imovie {
                let reversed = ed.selected_clip().is_some_and(|c| c.reverse);
                chip(
                    p,
                    s,
                    Rect::new(x, y, 100, 28),
                    "Reverse",
                    "reverse",
                    reversed,
                );
            }
        }
        "audio" => {
            y = slider(p, ed, env, s, x, y, w, "Volume", Prop::Volume, "%");
            let muted = ed.value(Prop::Volume, env.clock_us) == Some(0);
            chip(
                p,
                s,
                Rect::new(x, y, 100, 28),
                if muted { "Unmute" } else { "Mute" },
                "mute-clip",
                muted,
            );
        }
        "title" => {
            widgets::title_controls(p, ed, env, s, x, y, w);
        }
        "transform" => {
            y = slider(p, ed, env, s, x, y, w, "Scale", Prop::Scale, "");
            y = slider(p, ed, env, s, x, y, w, "Position X", Prop::X, "");
            let _ = slider(p, ed, env, s, x, y, w, "Opacity", Prop::Opacity, "");
        }
        "color" => {
            y = slider(p, ed, env, s, x, y, w, "Brightness", Prop::Brightness, "");
            y = slider(p, ed, env, s, x, y, w, "Contrast", Prop::Contrast, "");
            let _ = slider(p, ed, env, s, x, y, w, "Saturation", Prop::Saturation, "");
        }
        "fade" => {
            y = slider(p, ed, env, s, x, y, w, "Fade in", Prop::FadeIn, "");
            let _ = slider(p, ed, env, s, x, y, w, "Fade out", Prop::FadeOut, "");
        }
        _ => {}
    }
}
