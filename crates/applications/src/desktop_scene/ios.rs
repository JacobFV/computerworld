//! iOS 18 presentation: safe areas, SpringBoard grid, dock, and full-screen apps.
//! Geometry follows Apple's iOS 18 Home Screen reference, while every icon launches
//! an actual simulator application. The clock uses simulation time exclusively.
use super::shared::{Painter, ShellContext, WindowView};
use cw_scene::{Color, Primitive, Rect};

const BLUE: Color = Color::rgb(0, 122, 255);
const INK: Color = Color::rgb(22, 22, 24);
const APPS: [(&str, &str, &str); 8] = [
    ("mail", "mail", "Mail"),
    ("calendar", "calendar", "Calendar"),
    ("docs", "editor", "Documents"),
    ("chat", "messages", "Messages"),
    ("files", "files", "Files"),
    ("editor", "editor", "Notes"),
    ("terminal", "terminal", "Terminal"),
    ("browser", "browser", "Safari"),
];

fn label(p: &mut Painter, x: i32, y: i32, width: u32, text: &str, size: u16, color: Color) {
    // UI font is proportional. Center short icon labels using conservative advances.
    let advance = text
        .chars()
        .map(|c| {
            if matches!(c, 'i' | 'l' | 'I' | 't') {
                0.30
            } else if matches!(c, 'm' | 'w' | 'M' | 'W') {
                0.82
            } else {
                0.53
            }
        })
        .sum::<f32>();
    let tw = (advance * f32::from(size)).round() as i32;
    p.text(
        x + ((width as i32 - tw) / 2).max(0),
        y,
        width,
        text,
        size,
        color,
    );
}
#[allow(clippy::too_many_arguments)]
fn app(
    p: &mut Painter,
    x: i32,
    y: i32,
    size: u32,
    kind: &str,
    asset: &str,
    name: &str,
    show_label: bool,
) {
    let r = Rect::new(x, y, size, size);
    p.box_(
        Rect::new(x + 1, y + 3, size - 2, size),
        Color(0, 0, 0, 28),
        size / 4,
    );
    p.platform_icon(r, "ios", asset, &format!("shell:launch:{kind}"), name);
    if show_label {
        label(
            p,
            x - 12,
            y + size as i32 + 6,
            size + 24,
            name,
            12,
            Color(0, 0, 0, 55),
        );
        label(
            p,
            x - 12,
            y + size as i32 + 5,
            size + 24,
            name,
            12,
            Color::WHITE,
        );
    }
}

pub fn background(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.asset(Rect::new(0, 0, ctx.width, ctx.height), "wallpaper/ios");
    if ctx.active {
        return;
    }
    let width = ctx.width as i32;
    let height = ctx.height as i32;
    let size = ((ctx.width - 100) / 4).clamp(45, 62);
    let cell = (width - 38) / 4;
    let x_for = |column: i32| 19 + column * cell + (cell - size as i32) / 2;
    let top = 92.min((height / 8).max(66));
    // A clock widget reflects the same deterministic clock as the status bar.
    let widget = Rect::new(23, top, (cell * 2 - 9) as u32, 154);
    p.shadow(widget, 24);
    p.border(
        widget,
        Color(251, 251, 253, 240),
        24,
        Color(255, 255, 255, 105),
    );
    p.text(
        widget.x + 17,
        top + 14,
        widget.width - 25,
        "CLOCK",
        11,
        Color::rgb(105, 105, 110),
    );
    label(p, widget.x, top + 40, widget.width, &ctx.time(), 37, INK);
    label(
        p,
        widget.x,
        top + 105,
        widget.width,
        "Local time",
        12,
        Color::rgb(109, 109, 115),
    );
    p.region(widget, "shell:launcher", "Open app library");
    for (i, (kind, asset, name)) in APPS[..4]
        .iter()
        .filter(|(kind, _, _)| ctx.installed(kind))
        .enumerate()
    {
        app(
            p,
            x_for(2 + (i % 2) as i32),
            top + (i / 2) as i32 * 91,
            size,
            kind,
            asset,
            name,
            true,
        );
    }
    for (i, (kind, asset, name)) in APPS[4..]
        .iter()
        .filter(|(kind, _, _)| ctx.installed(kind))
        .enumerate()
    {
        app(p, x_for(i as i32), top + 190, size, kind, asset, name, true);
    }
    // Search opens the functional search panel backed by application catalog state.
    let sy = (height - 171).max(top + 300);
    p.button(
        Rect::new(width / 2 - 43, sy, 86, 29),
        Color(255, 255, 255, 45),
        15,
        "shell:search",
        "Search installed applications",
    );
    p.border(
        Rect::new(width / 2 - 25, sy + 9, 8, 8),
        Color::TRANSPARENT,
        4,
        Color::WHITE,
    );
    p.line(
        vec![(width / 2 - 18, sy + 16), (width / 2 - 15, sy + 19)],
        Color::WHITE,
        1,
    );
    p.text(width / 2 - 8, sy + 6, 49, "Search", 12, Color::WHITE);
    let dock = Rect::new(15, height - 126, ctx.width - 30, 96);
    p.shadow(dock, 32);
    p.border(
        dock,
        Color(235, 230, 242, 110),
        32,
        Color(255, 255, 255, 36),
    );
    for (i, (kind, asset, name)) in [APPS[0], APPS[7], APPS[3], APPS[5]]
        .iter()
        .filter(|(kind, _, _)| ctx.installed(kind))
        .enumerate()
    {
        app(
            p,
            x_for(i as i32),
            height - 110,
            size,
            kind,
            asset,
            name,
            false,
        );
    }
}

fn status(p: &mut Painter, ctx: &ShellContext<'_>, color: Color) {
    let w = ctx.width as i32;
    p.node(
        Rect::new(30, 18, 68, 25),
        Primitive::UiTextBold {
            text: ctx.time(),
            size: 16,
            color,
        },
        None,
    );
    p.box_(Rect::new(w / 2 - 49, 12, 98, 29), Color::rgb(3, 3, 5), 15);
    p.box_(Rect::new(w / 2 + 27, 20, 12, 12), Color::rgb(11, 14, 23), 6);
    p.box_(Rect::new(w / 2 + 30, 23, 5, 5), Color::rgb(21, 30, 51), 3);
    for i in 0..4 {
        p.box_(
            Rect::new(w - 87 + i * 4, 29 - i * 2, 3, (4 + i * 2) as u32),
            color,
            1,
        );
    }
    // Wi-Fi is drawn as three nested chevrons and a point, preserving crisp small geometry.
    p.line(
        vec![(w - 66, 25), (w - 62, 23), (w - 58, 23), (w - 54, 25)],
        color,
        2,
    );
    p.line(vec![(w - 64, 28), (w - 60, 26), (w - 56, 28)], color, 2);
    p.box_(Rect::new(w - 61, 30, 3, 3), color, 2);
    p.border(
        Rect::new(w - 47, 23, 24, 11),
        Color::TRANSPARENT,
        3,
        Color(color.0, color.1, color.2, 130),
    );
    p.box_(Rect::new(w - 45, 25, 19, 7), color, 1);
    p.box_(
        Rect::new(w - 22, 26, 2, 5),
        Color(color.0, color.1, color.2, 150),
        1,
    );
}

fn panel(p: &mut Painter, ctx: &ShellContext<'_>, name: &str) {
    let w = ctx.width as i32;
    let h = ctx.height as i32;
    p.box_(
        Rect::new(0, 0, ctx.width, ctx.height),
        Color(28, 28, 40, 244),
        0,
    );
    p.region(
        Rect::new(0, 50, ctx.width, ctx.height.saturating_sub(85)),
        "shell:dismiss",
        "Dismiss panel",
    );
    if name == "overview" {
        p.text(26, 77, ctx.width - 52, "App Switcher", 27, Color::WHITE);
        if ctx.windows.is_empty() {
            p.text(
                27,
                143,
                ctx.width - 54,
                "No open applications",
                16,
                Color(255, 255, 255, 180),
            );
        }
        for (i, window) in ctx.windows.iter().rev().take(4).enumerate() {
            let y = 139 + i as i32 * 132;
            if y + 120 > h - 50 {
                break;
            }
            p.button(
                Rect::new(24, y, ctx.width - 48, 113),
                Color::rgb(245, 245, 249),
                20,
                &window.action("focus"),
                &format!("Switch to {}", window.title),
            );
            p.asset(
                Rect::new(42, y + 19, 47, 47),
                &format!("icon/ios/{}", window.kind),
            );
            p.text(102, y + 27, ctx.width - 149, &window.title, 16, INK);
            p.text(
                43,
                y + 78,
                ctx.width - 90,
                if window.minimized {
                    "Suspended · Tap to return"
                } else {
                    "Open · Tap to return"
                },
                12,
                Color::rgb(112, 112, 119),
            );
            p.region(
                Rect::new(w - 67, y + 65, 31, 34),
                &window.action("close"),
                "Close application",
            );
            p.line(
                vec![(w - 56, y + 77), (w - 47, y + 86)],
                Color::rgb(130, 130, 137),
                2,
            );
            p.line(
                vec![(w - 47, y + 77), (w - 56, y + 86)],
                Color::rgb(130, 130, 137),
                2,
            );
        }
        return;
    }
    let search = name == "search";
    p.text(
        25,
        75,
        ctx.width - 50,
        if search {
            "Search"
        } else if name == "calendar" {
            "Today"
        } else {
            "Control Center"
        },
        29,
        Color::WHITE,
    );
    if search {
        p.button(
            Rect::new(20, 126, ctx.width - 40, 48),
            Color(255, 255, 255, 28),
            13,
            "shell:search",
            "Search applications",
        );
        p.text(
            37,
            140,
            ctx.width - 75,
            if ctx.search.is_empty() {
                "Search apps"
            } else {
                ctx.search
            },
            17,
            if ctx.search.is_empty() {
                Color(255, 255, 255, 140)
            } else {
                Color::WHITE
            },
        );
    } else {
        p.text(28, 130, ctx.width - 56, &ctx.time(), 43, Color::WHITE);
        p.text(
            29,
            187,
            ctx.width - 58,
            "Local time",
            13,
            Color(255, 255, 255, 170),
        );
    }
    let query = ctx.search.to_lowercase();
    let mut count = 0;
    for (kind, asset, label) in APPS.iter().filter(|(kind, _, label)| {
        ctx.installed(kind)
            && (!search || kind.contains(&query) || label.to_lowercase().contains(&query))
    }) {
        let y = if search { 196 } else { 229 } + count * 62;
        if y + 54 > h - 45 {
            break;
        }
        p.button(
            Rect::new(20, y, ctx.width - 40, 55),
            Color(255, 255, 255, 20),
            12,
            &format!("shell:launch:{kind}"),
            label,
        );
        p.asset(Rect::new(31, y + 8, 39, 39), &format!("icon/ios/{asset}"));
        p.text(86, y + 17, ctx.width - 117, label, 16, Color::WHITE);
        count += 1;
    }
    if count == 0 {
        p.text(
            30,
            206,
            ctx.width - 60,
            "No matching applications",
            15,
            Color(255, 255, 255, 170),
        );
    }
}

pub fn chrome(p: &mut Painter, ctx: &ShellContext<'_>) {
    let w = ctx.width as i32;
    let h = ctx.height as i32;
    if ctx.launcher_open {
        p.box_(
            Rect::new(0, 0, ctx.width, ctx.height),
            Color(24, 24, 35, 235),
            0,
        );
        p.button(
            Rect::new(22, 69, ctx.width - 44, 44),
            Color(255, 255, 255, 30),
            13,
            "shell:launcher",
            "Close app library",
        );
        p.text(44, 82, ctx.width - 80, "App Library", 17, Color::WHITE);
        let size = ((ctx.width - 100) / 4).clamp(45, 62);
        let cell = (w - 34) / 4;
        for (i, (kind, asset, name)) in APPS
            .iter()
            .filter(|(kind, _, _)| ctx.installed(kind))
            .enumerate()
        {
            app(
                p,
                17 + (i % 4) as i32 * cell + (cell - size as i32) / 2,
                151 + (i / 4) as i32 * 106,
                size,
                kind,
                asset,
                name,
                true,
            );
        }
        if !ctx.windows.is_empty() {
            p.text(
                25,
                383,
                ctx.width - 50,
                "Open applications",
                19,
                Color::WHITE,
            );
            for (i, window) in ctx.windows.iter().rev().take(4).enumerate() {
                let y = 425 + i as i32 * 57;
                if y + 50 > h - 55 {
                    break;
                }
                p.button(
                    Rect::new(21, y, ctx.width - 42, 49),
                    Color(255, 255, 255, 22),
                    12,
                    &window.action("focus"),
                    &format!("Switch to {}", window.title),
                );
                p.asset(
                    Rect::new(31, y + 8, 33, 33),
                    &format!("icon/ios/{}", window.kind),
                );
                p.text(78, y + 14, ctx.width - 110, &window.title, 14, Color::WHITE);
            }
        }
    }
    if let Some(name) = ctx.panel {
        panel(p, ctx, name);
    }
    let fg = if ctx.active && !ctx.launcher_open && ctx.panel.is_none() {
        INK
    } else {
        Color::WHITE
    };
    status(p, ctx, fg);
    p.region(Rect::new(20, 10, 80, 35), "shell:panel:calendar", "Today");
    p.region(
        Rect::new(w - 96, 10, 88, 35),
        "shell:panel:quick",
        "Control Center",
    );
    p.box_(Rect::new(w / 2 - 64, h - 13, 128, 5), fg, 3);
    p.region(
        Rect::new(w / 2 - 100, h - 32, 200, 32),
        "shell:home",
        "Home",
    );
}

pub fn window_frame(p: &mut Painter, ctx: &ShellContext<'_>, window: &WindowView) {
    p.box_(
        Rect::new(0, 0, ctx.width, ctx.height),
        Color::rgb(250, 250, 252),
        0,
    );
    p.box_(Rect::new(0, 94, ctx.width, 1), Color::rgb(224, 224, 229), 0);
    p.region(Rect::new(10, 51, 72, 42), "shell:home", "Home");
    p.line(vec![(23, 65), (16, 72), (23, 79)], BLUE, 2);
    p.text(29, 64, 58, "Home", 16, BLUE);
    let title = match window.kind.as_str() {
        "browser" => "Safari",
        "editor" => "Notes",
        "chat" => "Messages",
        "docs" => "Documents",
        _ => window.title.as_str(),
    };
    label(p, 85, 64, ctx.width.saturating_sub(170), title, 16, INK);
    p.region(
        Rect::new(ctx.width as i32 - 57, 51, 47, 42),
        "shell:launcher",
        "App library",
    );
    for row in 0..2 {
        for col in 0..2 {
            p.border(
                Rect::new(ctx.width as i32 - 43 + col * 9, 65 + row * 9, 6, 6),
                Color::TRANSPARENT,
                1,
                BLUE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_scene::DesktopTheme;

    #[test]
    fn home_only_exposes_installed_apps_and_keeps_home_hit_target() {
        let installed = vec!["files".to_owned(), "browser".to_owned()];
        let ctx = ShellContext {
            theme: DesktopTheme::Ios,
            width: 390,
            height: 844,
            clock_us: 0,
            title: "",
            launcher_open: false,
            active: false,
            windows: &[],
            installed_apps: &installed,
            panel: None,
            search: "",
            hover: None,
        };
        let mut p = Painter::new(390, 844);
        background(&mut p, &ctx);
        chrome(&mut p, &ctx);
        for node in &p.scene.nodes {
            if let Some(action) = node.interaction.as_deref() {
                if let Some(id) = action.strip_prefix("shell:launch:") {
                    assert!(installed.iter().any(|app| app == id));
                }
            }
        }
        assert_eq!(
            p.scene.hit_test(195, 830).unwrap().interaction.as_deref(),
            Some("shell:home")
        );
        assert!(p.scene.nodes.iter().any(|node| matches!(&node.primitive, cw_scene::Primitive::UiText {text,..} if text == "09:00")));
    }
}
