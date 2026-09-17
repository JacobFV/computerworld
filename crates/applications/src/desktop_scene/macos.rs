//! macOS geometry recovered from the predecessor MacChrome and macos-web-next:
//! a 28 px menu bar, 12 px traffic lights, restrained chrome and a floating Dock.
use super::shared::{Painter, ShellContext, WindowView};
use cw_scene::{Color, Primitive, Rect};

const INK: Color = Color(35, 35, 38, 255);
const CLEAR: Color = Color(0, 0, 0, 0);

pub fn background(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.asset(Rect::new(0, 0, ctx.width, ctx.height), "wallpaper/macos");
    // Desktop items are intentionally real entry points, not unrelated artwork.
    if ctx.width >= 640 {
        let x = ctx.width as i32 - 94;
        p.platform_icon(
            Rect::new(x + 9, 55, 52, 52),
            "macos",
            "files",
            "shell:launch:files",
            "Open Macintosh HD",
        );
        desktop_label(p, x - 10, 111, "Macintosh HD", 104);
        p.platform_icon(
            Rect::new(x + 9, 157, 52, 52),
            "macos",
            "editor",
            "shell:launch:editor",
            "Open TextEdit",
        );
        desktop_label(p, x - 2, 213, "TextEdit", 90);
    }
}

fn desktop_label(p: &mut Painter, x: i32, y: i32, text: &str, width: u32) {
    p.text(x + 1, y + 1, width, text, 12, Color(0, 0, 0, 155));
    p.text(x, y, width, text, 12, Color::WHITE);
}

pub fn window_frame(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let r = w.rect;
    let radius = if w.maximized { 0 } else { 10 };
    if !w.maximized {
        p.shadow(r, radius);
    }
    p.border(r, Color::rgb(250, 250, 251), radius, Color(55, 55, 65, 100));
    p.box_(
        Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(2), 41),
        if w.focused {
            Color::rgb(242, 241, 242)
        } else {
            Color::rgb(246, 246, 246)
        },
        radius,
    );
    // Square only the bottom edge; preserve the two rounded top corners.
    p.box_(
        Rect::new(r.x + 1, r.y + 25, r.width.saturating_sub(2), 17),
        Color::rgb(242, 241, 242),
        0,
    );
    p.line(
        vec![(r.x + 1, r.y + 41), (r.x + r.width as i32 - 1, r.y + 41)],
        Color::rgb(218, 217, 219),
        1,
    );
    p.region(
        Rect::new(r.x, r.y, r.width, 42),
        &w.action("drag"),
        &format!("Move {}", w.title),
    );
    let title_width = r.width.saturating_sub(190);
    let title = match w.kind.as_str() {
        "files" => "Finder",
        "terminal" => "Terminal",
        "editor" => "TextEdit",
        "browser" => "Safari",
        _ => &w.title,
    };
    let approximate_width = (title.chars().count() as u32 * 7).min(title_width);
    bold(
        p,
        r.x + ((r.width - approximate_width) / 2) as i32,
        r.y + 13,
        title_width,
        title,
        13,
        if w.focused {
            INK
        } else {
            Color::rgb(139, 139, 144)
        },
    );
    for (i, (action, label, color, edge)) in [
        (
            "close",
            "Close window",
            Color::rgb(255, 95, 87),
            Color::rgb(224, 68, 62),
        ),
        (
            "minimize",
            "Minimize window",
            Color::rgb(254, 188, 46),
            Color::rgb(222, 161, 35),
        ),
        (
            "maximize",
            "Zoom window",
            Color::rgb(40, 200, 64),
            Color::rgb(26, 171, 41),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let x = r.x + 16 + i as i32 * 20;
        p.border(
            Rect::new(x, r.y + 15, 12, 12),
            if w.focused {
                color
            } else {
                Color::rgb(210, 209, 211)
            },
            6,
            if w.focused {
                edge
            } else {
                Color::rgb(190, 189, 191)
            },
        );
        if ctx.hovered(Rect::new(r.x + 10, r.y + 10, 70, 24)) {
            let cy = r.y + 21;
            let ink = Color::rgb(90, 58, 38);
            match action {
                "close" => {
                    p.line(vec![(x + 4, cy - 2), (x + 8, cy + 2)], ink, 1);
                    p.line(vec![(x + 4, cy + 2), (x + 8, cy - 2)], ink, 1);
                }
                "minimize" => p.line(vec![(x + 3, cy), (x + 9, cy)], ink, 1),
                _ => {
                    p.path(
                        vec![(x + 3, cy - 3), (x + 8, cy - 3), (x + 3, cy + 2)],
                        Color::rgb(16, 95, 35),
                    );
                    p.path(
                        vec![(x + 9, cy + 3), (x + 4, cy + 3), (x + 9, cy - 2)],
                        Color::rgb(16, 95, 35),
                    );
                }
            }
        }
        p.region(Rect::new(x - 3, r.y + 11, 18, 20), &w.action(action), label);
    }
}

pub fn chrome(p: &mut Painter, ctx: &ShellContext<'_>) {
    menu_bar(p, ctx);
    dock(p, ctx);
    if ctx.launcher_open {
        launchpad(p, ctx);
    }
    if let Some(panel) = ctx.panel {
        panel_surface(p, ctx, panel);
    }
}

fn menu_bar(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.box_(Rect::new(0, 0, ctx.width, 28), Color(248, 244, 250, 215), 0);
    p.line(
        vec![(0, 27), (ctx.width as i32, 27)],
        Color(255, 255, 255, 75),
        1,
    );
    // This small original silhouette deliberately lives in vector geometry rather
    // than relying on the private-use Apple font character on the host machine.
    p.path(
        vec![
            (20, 9),
            (17, 8),
            (14, 10),
            (13, 14),
            (14, 19),
            (17, 23),
            (20, 22),
            (23, 23),
            (26, 20),
            (27, 17),
            (24, 15),
            (24, 12),
            (26, 10),
            (23, 8),
        ],
        INK,
    );
    p.path(vec![(20, 8), (21, 4), (25, 3), (24, 6)], INK);
    p.region(Rect::new(6, 0, 35, 28), "shell:panel:apple", "Apple menu");
    let front = ctx.windows.iter().find(|w| w.focused);
    let app = match front.map(|w| w.kind.as_str()) {
        Some("browser") => "Safari",
        Some("terminal") => "Terminal",
        Some("editor") => "TextEdit",
        _ => "Finder",
    };
    bold(p, 48, 7, 85, app, 13, INK);
    let mut x = 48 + (app.len() as i32 * 7) + 24;
    for label in ["File", "Edit", "View", "Window", "Help"] {
        if x + 55 > ctx.width as i32 - 290 {
            break;
        }
        let menu_r = Rect::new(x - 5, 0, (label.len() * 7 + 18) as u32, 28);
        if ctx.hovered(menu_r) || ctx.panel == Some(label.to_lowercase().as_str()) {
            p.box_(menu_r, Color(70, 65, 85, 35), 5);
        }
        p.text(x, 7, 62, label, 13, INK);
        p.region(
            Rect::new(x - 5, 0, (label.len() * 7 + 18) as u32, 28),
            &format!("shell:panel:{}", label.to_lowercase()),
            &format!("{label} menu"),
        );
        x += label.len() as i32 * 7 + 24;
    }
    if ctx.width > 720 {
        let right = ctx.width as i32;
        // Battery, Wi-Fi, Spotlight and Control Center have hand-tuned 1 px strokes.
        p.border(Rect::new(right - 272, 9, 22, 10), CLEAR, 2, INK);
        p.box_(Rect::new(right - 270, 11, 16, 6), INK, 1);
        p.box_(Rect::new(right - 249, 12, 2, 4), INK, 1);
        let wx = right - 222;
        p.line(
            vec![
                (wx - 8, 11),
                (wx - 4, 8),
                (wx, 7),
                (wx + 4, 8),
                (wx + 8, 11),
            ],
            INK,
            2,
        );
        p.line(vec![(wx - 5, 14), (wx, 11), (wx + 5, 14)], INK, 2);
        p.box_(Rect::new(wx - 1, 16, 3, 3), INK, 2);
        p.border(Rect::new(right - 195, 8, 8, 8), CLEAR, 5, INK);
        p.line(vec![(right - 188, 15), (right - 184, 19)], INK, 1);
        p.region(
            Rect::new(right - 204, 0, 29, 28),
            "shell:panel:spotlight",
            "Spotlight Search",
        );
        for (dy, knob) in [(8, 0), (15, 7)] {
            p.border(Rect::new(right - 163, dy, 16, 5), CLEAR, 3, INK);
            p.box_(Rect::new(right - 162 + knob, dy, 5, 5), INK, 3);
        }
        p.region(
            Rect::new(right - 172, 0, 33, 28),
            "shell:panel:control",
            "Control Center",
        );
        p.text(
            right - 131,
            7,
            128,
            &format!("Thu Sep 17  {}", ctx.time()),
            12,
            INK,
        );
        p.region(
            Rect::new(right - 136, 0, 136, 28),
            "shell:panel:notifications",
            "Notification Center",
        );
    } else {
        p.text(ctx.width as i32 - 52, 7, 48, &ctx.time(), 12, INK);
    }
}

fn dock(p: &mut Painter, ctx: &ShellContext<'_>) {
    let kinds = [
        ("files", "Finder"),
        ("browser", "Safari"),
        ("mail", "Mail"),
        ("calendar", "Calendar"),
        ("editor", "TextEdit"),
        ("terminal", "Terminal"),
    ];
    let kinds: Vec<_> = kinds
        .into_iter()
        .filter(|(kind, _)| ctx.installed(kind))
        .collect();
    let icon = if ctx.width < 600 { 38 } else { 48 };
    let gap = 8;
    let width = kinds.len() as u32 * (icon + gap) + 26;
    let x = (ctx.width.saturating_sub(width) / 2) as i32;
    let y = ctx.height as i32 - icon as i32 - 28;
    let tray = Rect::new(x, y, width, icon + 20);
    p.shadow(tray, 17);
    p.border(
        tray,
        Color(238, 238, 247, 145),
        17,
        Color(255, 255, 255, 160),
    );
    p.box_(
        Rect::new(x + 12, y + 1, width - 24, 1),
        Color(255, 255, 255, 145),
        0,
    );
    for (i, (kind, label)) in kinds.iter().enumerate() {
        let ix = x + 13 + i as i32 * (icon + gap) as i32;
        let existing = ctx.windows.iter().rev().find(|w| w.kind == *kind);
        let action = existing
            .map(|w| w.action("focus"))
            .unwrap_or_else(|| format!("shell:launch:{kind}"));
        let hovered = ctx.hovered(Rect::new(ix - 3, y, icon + 6, icon + 20));
        if hovered {
            p.box_(
                Rect::new(ix - 4, y + 2, icon + 8, icon + 12),
                Color(255, 255, 255, 55),
                10,
            );
            let tw = label.len() as u32 * 7 + 20;
            let tx = ix + (icon as i32 - tw as i32) / 2;
            p.shadow(Rect::new(tx, y - 36, tw, 25), 6);
            p.border(
                Rect::new(tx, y - 36, tw, 25),
                Color(243, 241, 245, 245),
                6,
                Color(255, 255, 255, 180),
            );
            p.text(tx + 10, y - 30, tw - 16, label, 12, INK);
        }
        p.platform_icon(
            Rect::new(ix, y + if hovered { 1 } else { 6 }, icon, icon),
            "macos",
            kind,
            &action,
            label,
        );
        if existing.is_some() {
            p.box_(
                Rect::new(ix + icon as i32 / 2 - 2, y + icon as i32 + 10, 4, 4),
                Color(35, 35, 38, 210),
                2,
            );
        }
    }
}

fn launchpad(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = ctx.width.saturating_sub(64).min(610);
    let height = ctx.height.saturating_sub(135).min(310);
    let x = (ctx.width - width) as i32 / 2;
    let y = 58;
    p.shadow(Rect::new(x, y, width, height), 18);
    p.border(
        Rect::new(x, y, width, height),
        Color(245, 245, 249, 246),
        18,
        Color(255, 255, 255, 200),
    );
    p.text(x + 24, y + 20, width - 48, "Applications", 20, INK);
    p.region(
        Rect::new(x + width as i32 - 42, y + 12, 28, 28),
        "shell:launcher",
        "Close Applications",
    );
    p.text(
        x + width as i32 - 34,
        y + 18,
        22,
        "×",
        20,
        Color::rgb(100, 100, 107),
    );
    let apps = [
        ("files", "Finder"),
        ("browser", "Safari"),
        ("mail", "Mail"),
        ("calendar", "Calendar"),
        ("editor", "TextEdit"),
        ("terminal", "Terminal"),
    ];
    let apps: Vec<_> = apps
        .into_iter()
        .filter(|(kind, _)| ctx.installed(kind))
        .collect();
    let columns = if width >= 480 { 6 } else { 3 };
    let cell = width / columns;
    for (i, (kind, label)) in apps.iter().enumerate() {
        let ax = x + (i as u32 % columns * cell) as i32 + (cell as i32 - 50) / 2;
        let ay = y + 73 + (i as u32 / columns * 104) as i32;
        if ay + 73 > y + height as i32 {
            break;
        }
        p.platform_icon(
            Rect::new(ax, ay, 50, 50),
            "macos",
            kind,
            &format!("shell:launch:{kind}"),
            label,
        );
        p.text(ax - 7, ay + 58, cell.min(92), label, 12, INK);
    }
}

fn panel_surface(p: &mut Painter, ctx: &ShellContext<'_>, panel: &str) {
    let front = ctx.windows.iter().find(|w| w.focused);
    if panel == "spotlight" || panel == "search" {
        let width = ctx.width.saturating_sub(48).min(600);
        let x = (ctx.width - width) as i32 / 2;
        let y = (ctx.height / 5).max(45) as i32;
        let query = ctx.search.to_lowercase();
        let apps: Vec<_> = [
            ("files", "Finder"),
            ("browser", "Safari"),
            ("editor", "TextEdit"),
            ("terminal", "Terminal"),
            ("mail", "Mail"),
            ("calendar", "Calendar"),
        ]
        .into_iter()
        .filter(|(k, n)| {
            ctx.installed(k)
                && (query.is_empty() || n.to_lowercase().contains(&query) || k.contains(&query))
        })
        .collect();
        let h = 64 + apps.len() as u32 * 44;
        p.shadow(Rect::new(x, y, width, h), 12);
        p.border(
            Rect::new(x, y, width, h),
            Color(249, 248, 250, 250),
            12,
            Color(255, 255, 255, 220),
        );
        p.border(
            Rect::new(x + 21, y + 21, 16, 16),
            CLEAR,
            9,
            Color::rgb(100, 100, 108),
        );
        p.line(
            vec![(x + 35, y + 35), (x + 43, y + 43)],
            Color::rgb(100, 100, 108),
            2,
        );
        p.text(
            x + 59,
            y + 20,
            width - 85,
            if ctx.search.is_empty() {
                "Spotlight Search"
            } else {
                ctx.search
            },
            24,
            if ctx.search.is_empty() {
                Color::rgb(144, 144, 150)
            } else {
                INK
            },
        );
        p.region(
            Rect::new(x, y, width, 62),
            "shell:noop",
            "Spotlight search field",
        );
        for (i, (kind, name)) in apps.iter().enumerate() {
            let ay = y + 65 + i as i32 * 44;
            if i == 0 {
                p.box_(
                    Rect::new(x + 8, ay - 2, width - 16, 40),
                    Color::rgb(45, 116, 222),
                    6,
                );
            }
            p.asset(
                Rect::new(x + 19, ay + 2, 30, 30),
                &format!("icon/macos/{kind}"),
            );
            p.text(
                x + 61,
                ay + 8,
                width - 83,
                name,
                14,
                if i == 0 { Color::WHITE } else { INK },
            );
            p.region(
                Rect::new(x + 8, ay - 2, width - 16, 40),
                &format!("shell:launch:{kind}"),
                name,
            );
        }
        return;
    }
    if panel == "settings" {
        let width = ctx.width.saturating_sub(48).min(480);
        let x = (ctx.width - width) as i32 / 2;
        p.shadow(Rect::new(x, 60, width, 260), 12);
        p.border(
            Rect::new(x, 60, width, 260),
            Color::rgb(246, 245, 247),
            12,
            Color::rgb(220, 218, 224),
        );
        bold(p, x + 24, 82, width - 48, "System Settings", 21, INK);
        p.text(x + 24, 125, width - 48, "macOS Golden Gate", 15, INK);
        p.text(
            x + 24,
            156,
            width - 48,
            &format!("Display: {} × {}", ctx.width, ctx.height),
            13,
            INK,
        );
        p.text(
            x + 24,
            181,
            width - 48,
            &format!("Open windows: {}", ctx.windows.len()),
            13,
            INK,
        );
        p.text(
            x + 24,
            206,
            width - 48,
            "Applications are provided by this computer's world.",
            12,
            Color::rgb(108, 106, 114),
        );
        p.button(
            Rect::new(x + 24, 256, 160, 32),
            Color::rgb(226, 225, 230),
            6,
            "shell:launcher",
            "Show Applications",
        );
        p.text(x + 37, 265, 140, "Show Applications", 13, INK);
        p.button(
            Rect::new(x + width as i32 - 98, 256, 74, 32),
            Color::rgb(36, 113, 223),
            6,
            "shell:dismiss",
            "Done",
        );
        p.text(x + width as i32 - 77, 265, 54, "Done", 13, Color::WHITE);
        return;
    }
    if panel == "quick" || panel == "control" {
        control_center(p, ctx);
        return;
    }
    if panel == "notifications" {
        let width = 320.min(ctx.width.saturating_sub(32));
        let x = ctx.width as i32 - width as i32 - 16;
        p.shadow(Rect::new(x, 40, width, 164), 14);
        p.border(
            Rect::new(x, 40, width, 164),
            Color(247, 245, 249, 245),
            14,
            Color(255, 255, 255, 190),
        );
        p.text(x + 20, 58, width - 40, "Notification Center", 18, INK);
        p.text(
            x + 20,
            105,
            width - 40,
            "No new notifications",
            13,
            Color::rgb(119, 117, 125),
        );
        p.region(
            Rect::new(x + 16, 149, width - 32, 32),
            "shell:dismiss",
            "Close Notification Center",
        );
        p.text(
            x + 20,
            155,
            width - 40,
            "Dismiss",
            13,
            Color::rgb(40, 105, 211),
        );
        return;
    }
    let mut entries: Vec<(String, String, &str)> = match panel {
        "apple" => vec![
            ("Applications…".into(), "shell:launcher".into(), ""),
            ("System Settings…".into(), "shell:settings".into(), ""),
            ("Show Desktop".into(), "shell:home".into(), ""),
        ],
        "file" => vec![
            ("New Window".into(), "shell:new".into(), "⌘N"),
            ("Save".into(), "shell:save".into(), "⌘S"),
            (
                "New Finder Window".into(),
                "shell:launch:files".into(),
                "⌘N",
            ),
            ("New Text Document".into(), "shell:launch:editor".into(), ""),
        ]
        .into_iter()
        .chain(front.map(|w| ("Close Window".into(), w.action("close"), "⌘W")))
        .collect(),
        "window" => front
            .map(|w| {
                vec![
                    ("Minimize".into(), w.action("minimize"), "⌘M"),
                    ("Zoom".into(), w.action("maximize"), ""),
                ]
            })
            .unwrap_or_default()
            .into_iter()
            .chain(
                ctx.windows
                    .iter()
                    .map(|w| (w.title.clone(), w.action("focus"), "")),
            )
            .collect(),
        "view" => vec![
            ("Show Desktop".into(), "shell:home".into(), ""),
            ("Show Applications".into(), "shell:launcher".into(), ""),
        ],
        "edit" => vec![(
            "Find Applications…".into(),
            "shell:panel:spotlight".into(),
            "",
        )],
        "help" => vec![
            (
                "Search Applications".into(),
                "shell:panel:spotlight".into(),
                "",
            ),
            ("System Settings".into(), "shell:settings".into(), ""),
        ],
        _ => vec![
            ("System Settings".into(), "shell:settings".into(), ""),
            (
                "Search Applications".into(),
                "shell:panel:spotlight".into(),
                "",
            ),
        ],
    };
    entries.retain(|(_, action, _)| {
        action != "shell:save" || front.is_some_and(|w| w.kind == "editor")
    });
    let desired_x = match panel {
        "apple" => 8,
        "file" => 114,
        "edit" => 164,
        "view" => 219,
        "window" => 274,
        "help" => 349,
        _ => ctx.width as i32 - 260,
    };
    let width = 248.min(ctx.width.saturating_sub(16));
    let x = desired_x
        .max(8)
        .min(ctx.width.saturating_sub(width + 8) as i32);
    let h = entries.len() as u32 * 28 + 12;
    p.shadow(Rect::new(x, 29, width, h), 7);
    p.border(
        Rect::new(x, 29, width, h),
        Color(245, 243, 248, 247),
        7,
        Color(255, 255, 255, 190),
    );
    for (i, (label, action, shortcut)) in entries.iter().enumerate() {
        let y = 35 + i as i32 * 28;
        let hover = ctx.hovered(Rect::new(x + 5, y, width - 10, 27));
        if hover {
            p.box_(
                Rect::new(x + 5, y, width - 10, 27),
                Color::rgb(37, 111, 221),
                4,
            );
        }
        p.text(
            x + 14,
            y + 5,
            width - 58,
            label,
            13,
            if hover { Color::WHITE } else { INK },
        );
        if !shortcut.is_empty() {
            p.text(
                x + width as i32 - 42,
                y + 5,
                38,
                shortcut,
                12,
                Color::rgb(100, 100, 106),
            );
        }
        p.region(Rect::new(x + 5, y, width - 10, 27), action, label);
    }
}

fn control_center(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = 310.min(ctx.width.saturating_sub(32));
    let x = ctx.width as i32 - width as i32 - 16;
    let y = 39;
    p.shadow(Rect::new(x, y, width, 224), 14);
    p.border(
        Rect::new(x, y, width, 224),
        Color(236, 235, 241, 238),
        14,
        Color(255, 255, 255, 190),
    );
    p.text(x + 16, y + 14, width - 32, "Control Center", 15, INK);
    let cell = (width - 36) / 2;
    for (i, (label, detail, icon, action)) in [
        (
            "Settings",
            "System preferences",
            "settings",
            "shell:settings",
        ),
        (
            "Notifications",
            "Notification Center",
            "calendar",
            "shell:panel:notifications",
        ),
        (
            "Applications",
            "Open an application",
            "files",
            "shell:launcher",
        ),
        (
            "Spotlight",
            "Search applications",
            "browser",
            "shell:search",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let cx = x + 12 + (i % 2) as i32 * (cell as i32 + 12);
        let cy = y + 44 + (i / 2) as i32 * 79;
        p.border(
            Rect::new(cx, cy, cell, 70),
            Color(255, 255, 255, 205),
            10,
            Color(255, 255, 255, 155),
        );
        p.asset(
            Rect::new(cx + 11, cy + 10, 26, 26),
            &format!("icon/macos/{icon}"),
        );
        p.text(cx + 10, cy + 40, cell - 16, label, 12, INK);
        p.region(Rect::new(cx, cy, cell, 70), action, detail);
    }
}

#[allow(clippy::too_many_arguments)]
fn bold(p: &mut Painter, x: i32, y: i32, width: u32, text: &str, size: u16, color: Color) {
    p.node(
        Rect::new(x, y, width, u32::from(size) + 9),
        Primitive::UiTextBold {
            text: text.into(),
            size,
            color,
        },
        None,
    );
}
