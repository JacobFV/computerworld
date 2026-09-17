//! Ubuntu 24 / GNOME shell. Geometry follows the native 32px panel, 68px
//! Ubuntu Dock and 46px libadwaita headerbar; all controls dispatch Rust actions.
use super::shared::{Painter, ShellContext, WindowView};
use cw_scene::{Color, Rect};

const PANEL: Color = Color::rgb(30, 30, 30);
const INK: Color = Color::rgb(47, 47, 47);
const ORANGE: Color = Color::rgb(233, 84, 32);
const APPS: [(&str, &str); 8] = [
    ("browser", "Web Browser"),
    ("files", "Files"),
    ("terminal", "Terminal"),
    ("editor", "Text Editor"),
    ("mail", "Mail"),
    ("calendar", "Calendar"),
    ("chat", "Chat"),
    ("docs", "Documents"),
];

pub fn background(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.asset(Rect::new(0, 0, ctx.width, ctx.height), "wallpaper/ubuntu");
    if ctx.width > 300 && ctx.height > 250 && ctx.installed("files") {
        let x = ctx.width as i32 - 98;
        p.platform_icon(
            Rect::new(x + 14, 58, 48, 48),
            "ubuntu",
            "files",
            "shell:launch:files",
            "Open home folder",
        );
        p.text(x + 19, 112, 76, "Home", 13, Color::WHITE);
    }
}

pub fn chrome(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = ctx.width as i32;
    let height = ctx.height as i32;
    p.box_(Rect::new(0, 0, ctx.width, 32), PANEL, 0);
    // GNOME 46's Activities control is a workspace pill, not a text menu.
    p.button(
        Rect::new(6, 3, 63, 26),
        if ctx.hovered(Rect::new(6, 3, 63, 26)) {
            Color::rgb(65, 65, 65)
        } else {
            PANEL
        },
        13,
        "shell:launcher",
        "Activities / Show applications",
    );
    p.box_(Rect::new(16, 12, 25, 8), Color::rgb(247, 247, 247), 4);
    p.box_(Rect::new(47, 12, 8, 8), Color::rgb(154, 154, 154), 4);
    let clock = format!("Sep 17  {}", ctx.time());
    p.text((width / 2 - 48).max(75), 5, 140, &clock, 13, Color::WHITE);
    p.region(
        Rect::new((width / 2 - 64).max(72), 0, 154, 32),
        "shell:panel:calendar",
        "Open calendar and notifications",
    );
    if width > 380 {
        tray(p, width - 91, 10);
        p.region(
            Rect::new(width - 98, 0, 98, 32),
            "shell:panel:quick",
            "Open system menu",
        );
    }
    // Ubuntu's fixed full-height dock, with subtly translucent aubergine backing.
    p.box_(
        Rect::new(0, 32, 68, ctx.height.saturating_sub(32)),
        Color(36, 29, 37, 235),
        0,
    );
    p.box_(
        Rect::new(67, 32, 1, ctx.height.saturating_sub(32)),
        Color(255, 255, 255, 19),
        0,
    );
    for (i, (kind, name)) in APPS
        .iter()
        .filter(|(kind, _)| ctx.installed(kind))
        .take(4)
        .enumerate()
    {
        let y = 43 + i as i32 * 59;
        if y + 55 > height - 65 {
            break;
        }
        let running = ctx
            .windows
            .iter()
            .filter(|w| w.kind == *kind)
            .collect::<Vec<_>>();
        let dock_rect = Rect::new(6, y - 2, 56, 56);
        let hovered = ctx.hovered(dock_rect);
        if hovered || running.iter().any(|w| w.focused && !w.minimized) {
            p.box_(
                dock_rect,
                Color(255, 255, 255, if hovered { 48 } else { 28 }),
                8,
            );
        }
        p.platform_icon(
            Rect::new(12, y + 4, 44, 44),
            "ubuntu",
            kind,
            &format!("shell:launch:{kind}"),
            name,
        );
        p.region(dock_rect, &format!("shell:launch:{kind}"), name);
        if hovered && !ctx.launcher_open {
            let tip = Rect::new(78, y + 9, (name.len() as u32 * 7 + 24).max(72), 30);
            p.shadow(tip, 7);
            p.box_(tip, Color::rgb(42, 42, 42), 7);
            p.text(
                tip.x + 12,
                tip.y + 8,
                tip.width - 20,
                name,
                12,
                Color::WHITE,
            );
        }
        for n in 0..running.len().min(3) {
            p.box_(Rect::new(1, y + 23 + n as i32 * 7, 4, 4), ORANGE, 2);
        }
    }
    if height > 190 {
        let y = height - 62;
        p.button(
            Rect::new(6, y, 56, 54),
            if ctx.launcher_open || ctx.hovered(Rect::new(6, y, 56, 54)) {
                Color(255, 255, 255, 30)
            } else {
                Color::TRANSPARENT
            },
            8,
            "shell:launcher",
            "Show applications",
        );
        grid(p, 22, y + 16, 5, 7, Color::rgb(241, 241, 241));
    }
    if ctx.launcher_open || ctx.panel == Some("search") {
        launcher(p, ctx);
    }
    if let Some(panel) = ctx.panel {
        if panel != "search" {
            panel_surface(p, ctx, panel);
        }
    }
}

fn tray(p: &mut Painter, x: i32, y: i32) {
    let white = Color::rgb(240, 240, 240);
    // Crisp symbolic GNOME network, speaker and battery glyphs.
    p.line(
        vec![(x, y + 2), (x + 4, y), (x + 9, y), (x + 13, y + 2)],
        white,
        2,
    );
    p.line(
        vec![(x + 3, y + 6), (x + 6, y + 4), (x + 9, y + 6)],
        white,
        2,
    );
    p.box_(Rect::new(x + 5, y + 9, 3, 3), white, 2);
    p.path(
        vec![
            (x + 25, y + 4),
            (x + 28, y + 4),
            (x + 32, y + 1),
            (x + 32, y + 12),
            (x + 28, y + 9),
            (x + 25, y + 9),
        ],
        white,
    );
    p.line(
        vec![(x + 36, y + 3), (x + 38, y + 6), (x + 36, y + 10)],
        white,
        1,
    );
    p.border(
        Rect::new(x + 49, y + 1, 17, 11),
        Color::TRANSPARENT,
        2,
        white,
    );
    p.box_(Rect::new(x + 51, y + 3, 12, 7), white, 1);
    p.box_(Rect::new(x + 66, y + 4, 2, 5), white, 1);
}

fn grid(p: &mut Painter, x: i32, y: i32, size: u32, gap: i32, c: Color) {
    for row in 0..3 {
        for col in 0..3 {
            p.box_(Rect::new(x + col * gap, y + row * gap, size, size), c, 1);
        }
    }
}

fn launcher(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = ctx.width as i32;
    let height = ctx.height as i32;
    p.box_(
        Rect::new(
            68,
            32,
            ctx.width.saturating_sub(68),
            ctx.height.saturating_sub(32),
        ),
        Color(36, 31, 42, 246),
        0,
    );
    let center = (width + 68) / 2;
    let search_width = (width - 110).clamp(140, 380) as u32;
    let search_x = center - search_width as i32 / 2;
    p.border(
        Rect::new(search_x, 58, search_width, 42),
        Color::rgb(65, 60, 70),
        22,
        Color::rgb(91, 86, 96),
    );
    p.region(
        Rect::new(search_x, 58, search_width, 42),
        "shell:search",
        "Search applications",
    );
    p.text(
        search_x + 48,
        69,
        search_width.saturating_sub(58),
        if ctx.search.is_empty() {
            "Type to search"
        } else {
            ctx.search
        },
        14,
        Color::rgb(224, 222, 226),
    );
    p.box_(
        Rect::new(search_x + 22, 72, 10, 10),
        Color::rgb(193, 190, 198),
        5,
    );
    p.box_(
        Rect::new(search_x + 24, 74, 6, 6),
        Color::rgb(65, 60, 70),
        3,
    );
    p.line(
        vec![(search_x + 30, 80), (search_x + 35, 85)],
        Color::rgb(193, 190, 198),
        2,
    );
    let columns = ((width - 100) / 140).clamp(1, 4);
    let cell = ((width - 100) / columns).min(150);
    let start = center - columns * cell / 2;
    for (i, (kind, name)) in APPS
        .iter()
        .filter(|(kind, name)| {
            ctx.installed(kind)
                && (ctx.search.is_empty()
                    || name.to_lowercase().contains(&ctx.search.to_lowercase()))
        })
        .enumerate()
    {
        let x = start + (i as i32 % columns) * cell;
        let y = 145 + (i as i32 / columns) * 136;
        if y + 94 > height {
            break;
        }
        let tile = Rect::new(x + 4, y - 12, cell.saturating_sub(8) as u32, 112);
        if ctx.hovered(tile) {
            p.box_(tile, Color(255, 255, 255, 24), 12);
        }
        p.region(tile, &format!("shell:launch:{kind}"), name);
        p.platform_icon(
            Rect::new(x + (cell - 68) / 2, y, 68, 68),
            "ubuntu",
            kind,
            &format!("shell:launch:{kind}"),
            name,
        );
        let offset = (cell - (name.chars().count() as i32 * 7)) / 2;
        p.text(x + offset, y + 80, cell as u32, name, 13, Color::WHITE);
    }
    if height > 400 {
        p.box_(
            Rect::new(center - 4, height - 36, 8, 8),
            Color::rgb(240, 239, 242),
            4,
        );
    }
}

fn panel_surface(p: &mut Painter, ctx: &ShellContext<'_>, panel: &str) {
    let width = ctx.width.saturating_sub(90).min(346);
    let x = if panel == "calendar" {
        (ctx.width.saturating_sub(width) / 2) as i32
    } else {
        ctx.width.saturating_sub(width + 12) as i32
    };
    let height = if panel == "calendar" { 326 } else { 238 };
    let r = Rect::new(x, 40, width, height);
    p.region(
        Rect::new(
            68,
            32,
            ctx.width.saturating_sub(68),
            ctx.height.saturating_sub(32),
        ),
        "shell:dismiss",
        "Dismiss system menu",
    );
    p.shadow(r, 16);
    p.border(r, Color::rgb(48, 48, 48), 16, Color::rgb(77, 77, 77));

    let text = Color::rgb(242, 242, 242);
    if panel == "calendar" {
        p.text(
            x + 22,
            60,
            width.saturating_sub(44),
            "Thursday, September 17",
            15,
            text,
        );
        p.text(
            x + 22,
            91,
            width.saturating_sub(44),
            "September 2026",
            14,
            text,
        );
        let cell = width.saturating_sub(40) / 7;
        for (i, day) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
            p.text(
                x + 22 + i as i32 * cell as i32,
                125,
                cell,
                day,
                12,
                Color::rgb(161, 161, 161),
            );
        }
        for day in 1..=30 {
            let index = day; // September 1, 2026 is Tuesday.
            let dx = x + 22 + (index % 7) * cell as i32;
            let dy = 151 + (index / 7) * 28;
            if day == 17 {
                p.box_(Rect::new(dx - 7, dy - 4, 28, 27), ORANGE, 14);
            }
            p.text(dx, dy, cell, &day.to_string(), 13, text);
        }
        p.line(
            vec![(x + 20, 307), (x + width as i32 - 20, 307)],
            Color::rgb(83, 83, 83),
            1,
        );
        if ctx.installed("calendar") {
            p.region(
                Rect::new(x + 16, 316, width.saturating_sub(32), 36),
                "shell:launch:calendar",
                "Open calendar application",
            );
            p.text(
                x + 24,
                323,
                width.saturating_sub(48),
                "Open Calendar",
                13,
                text,
            );
        } else {
            p.text(
                x + 24,
                323,
                width.saturating_sub(48),
                "No notifications",
                13,
                Color::rgb(183, 183, 183),
            );
        }
    } else {
        p.text(
            x + 22,
            60,
            width.saturating_sub(44),
            if panel == "settings" {
                "System"
            } else {
                "Quick Settings"
            },
            17,
            text,
        );
        p.text(
            x + 22,
            93,
            width.saturating_sub(44),
            "Ubuntu 24.04 LTS",
            14,
            Color::rgb(185, 185, 185),
        );
        for (i, (label, action)) in [
            ("Applications", "shell:launcher"),
            ("System information", "shell:settings"),
            ("Open Terminal", "shell:launch:terminal"),
        ]
        .iter()
        .enumerate()
        {
            let y = 124 + i as i32 * 44;
            p.button(
                Rect::new(x + 16, y, width.saturating_sub(32), 36),
                Color::rgb(64, 64, 64),
                9,
                action,
                label,
            );
            p.text(x + 29, y + 10, width.saturating_sub(58), label, 13, text);
        }
    }
}

pub fn window_frame(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let r = w.rect;
    let radius = if w.maximized { 0 } else { 12 };
    if !w.maximized {
        p.shadow(r, radius);
    }
    let edge = if w.focused {
        Color::rgb(117, 111, 116)
    } else {
        Color::rgb(145, 140, 145)
    };
    p.border(r, Color::rgb(250, 250, 250), radius, edge);
    let header = if w.focused {
        Color::rgb(235, 235, 235)
    } else {
        Color::rgb(243, 243, 243)
    };
    p.box_(
        Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(2), 45),
        header,
        radius.saturating_sub(1),
    );
    p.box_(
        Rect::new(r.x + 1, r.y + 24, r.width.saturating_sub(2), 22),
        header,
        0,
    );
    p.line(
        vec![(r.x + 1, r.y + 45), (r.x + r.width as i32 - 2, r.y + 45)],
        Color::rgb(209, 209, 209),
        1,
    );
    p.region(
        Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(2), 44),
        &w.action("drag"),
        &format!("Move {}", w.title),
    );
    // GNOME app menu at the left and title centered in the remaining header.
    let title_width = r.width.saturating_sub(158);
    let title_px = (w.title.chars().count() as u32 * 7).min(title_width);
    let title_x = r.x + (r.width.saturating_sub(title_px) / 2) as i32 - 16;
    p.text(
        title_x.max(r.x + 15),
        r.y + 14,
        title_width,
        &w.title,
        14,
        if w.focused {
            INK
        } else {
            Color::rgb(120, 120, 120)
        },
    );
    let x = r.x + r.width as i32;
    for (offset, action, label) in [
        (104, "minimize", "Minimize window"),
        (70, "maximize", "Maximize or restore window"),
        (36, "close", "Close window"),
    ] {
        let cx = x - offset;
        p.button(
            Rect::new(cx, r.y + 9, 28, 28),
            if ctx.hovered(Rect::new(cx, r.y + 9, 28, 28)) {
                Color::rgb(199, 199, 199)
            } else if action == "close" {
                Color::rgb(217, 217, 217)
            } else {
                header
            },
            14,
            &w.action(action),
            label,
        );
        match action {
            "minimize" => p.line(vec![(cx + 9, r.y + 24), (cx + 19, r.y + 24)], INK, 1),
            "maximize" => {
                p.border(Rect::new(cx + 9, r.y + 18, 10, 9), header, 1, INK);
                if w.maximized {
                    p.line(
                        vec![
                            (cx + 12, r.y + 15),
                            (cx + 21, r.y + 15),
                            (cx + 21, r.y + 24),
                        ],
                        INK,
                        1,
                    );
                }
            }
            _ => {
                p.line(vec![(cx + 10, r.y + 19), (cx + 18, r.y + 27)], INK, 1);
                p.line(vec![(cx + 18, r.y + 19), (cx + 10, r.y + 27)], INK, 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_scene::DesktopTheme;

    #[test]
    fn titlebar_controls_do_not_get_captured_by_drag_region() {
        let window = WindowView {
            id: 42,
            title: "Files".into(),
            kind: "files".into(),
            rect: Rect::new(120, 90, 700, 430),
            focused: true,
            maximized: false,
            minimized: false,
            content: None,
        };
        let windows = [window];
        let ctx = ShellContext {
            theme: DesktopTheme::Ubuntu,
            width: 1024,
            height: 768,
            clock_us: 0,
            title: "Files",
            launcher_open: false,
            active: true,
            windows: &windows,
            installed_apps: &[],
            panel: None,
            search: "",
            hover: None,
        };
        let mut painter = Painter::new(1024, 768);
        window_frame(&mut painter, &ctx, &windows[0]);
        for (x, expected) in [
            (400, "drag"),
            (730, "minimize"),
            (764, "maximize"),
            (798, "close"),
        ] {
            assert_eq!(
                painter
                    .scene
                    .hit_test(x, 113)
                    .and_then(|n| n.interaction.as_deref()),
                Some(format!("window:42:{expected}").as_str())
            );
        }
    }
}
