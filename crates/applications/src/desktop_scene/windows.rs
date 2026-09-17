//! Windows 11 shell geometry follows the predecessor's Fluent tokens, not its DOM.
use super::shared::{Painter, ShellContext, WindowView};
use cw_scene::{Color, Primitive, Rect};
const INK: Color = Color(28, 28, 30, 255);
const MUTED: Color = Color(96, 96, 103, 255);
const ACCENT: Color = Color(0, 103, 192, 255);
const PINNED: [(&str, &str); 4] = [
    ("browser", "Browser"),
    ("files", "File Explorer"),
    ("terminal", "Terminal"),
    ("editor", "Notepad"),
];

const APPS: [(&str, &str); 8] = [
    ("browser", "Browser"),
    ("files", "File Explorer"),
    ("terminal", "Terminal"),
    ("editor", "Notepad"),
    ("mail", "Mail"),
    ("calendar", "Calendar"),
    ("chat", "Chat"),
    ("docs", "Documents"),
];

pub fn background(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.asset(Rect::new(0, 0, ctx.width, ctx.height), "wallpaper/windows");
    if !ctx.installed("files") {
        return;
    }
    // The desktop shortcut opens the real file manager. No inert recycle-bin fixture.
    p.platform_icon(
        Rect::new(26, 26, 42, 42),
        "windows",
        "files",
        "shell:launch:files",
        "Open File Explorer",
    );
    p.text(12, 73, 94, "File Explorer", 12, Color(0, 0, 0, 130));
    p.text(11, 72, 94, "File Explorer", 12, Color::WHITE);
}

fn heading(p: &mut Painter, x: i32, y: i32, width: u32, text: &str, size: u16) {
    p.node(
        Rect::new(x, y, width, u32::from(size) + 9),
        Primitive::UiTextBold {
            text: text.into(),
            color: INK,
            size,
        },
        None,
    );
}

fn start_mark(p: &mut Painter, x: i32, y: i32) {
    for dy in [0, 13] {
        for dx in [0, 13] {
            p.box_(Rect::new(x + dx, y + dy, 11, 11), ACCENT, 1);
        }
    }
}
fn search_mark(p: &mut Painter, x: i32, y: i32) {
    p.border(Rect::new(x, y, 12, 12), Color(0, 0, 0, 0), 6, INK);
    p.line(vec![(x + 10, y + 10), (x + 15, y + 15)], INK, 1);
}

pub fn chrome(p: &mut Painter, ctx: &ShellContext<'_>) {
    let y = ctx.height.saturating_sub(48) as i32;
    p.border(
        Rect::new(0, y, ctx.width, 48),
        Color(235, 243, 252, 247),
        0,
        Color(255, 255, 255, 180),
    );
    let search_width = if ctx.width >= 1100 { 180 } else { 100 };
    let pinned: Vec<_> = PINNED
        .iter()
        .filter(|(kind, _)| ctx.installed(kind))
        .collect();
    let total = 48 + search_width + 48 + pinned.len() as i32 * 44;
    let left = ((ctx.width as i32 - total) / 2).max(4);
    p.button(
        Rect::new(left, y + 4, 40, 40),
        if ctx.launcher_open {
            Color(255, 255, 255, 175)
        } else {
            Color(0, 0, 0, 0)
        },
        4,
        "shell:launcher",
        "Start",
    );
    start_mark(p, left + 8, y + 12);
    p.button(
        Rect::new(left + 46, y + 8, search_width as u32, 32),
        Color(255, 255, 255, 185),
        16,
        "shell:search",
        "Search installed applications",
    );
    search_mark(p, left + 58, y + 16);
    p.text(
        left + 80,
        y + 14,
        (search_width - 40) as u32,
        "Search",
        14,
        MUTED,
    );
    let view_x = left + 52 + search_width;
    p.region(
        Rect::new(view_x, y + 4, 40, 40),
        "shell:overview",
        "Task view",
    );
    p.border(
        Rect::new(view_x + 8, y + 13, 15, 20),
        Color(195, 206, 218, 255),
        2,
        Color(119, 132, 150, 255),
    );
    p.border(
        Rect::new(view_x + 18, y + 17, 15, 16),
        Color(246, 251, 255, 255),
        2,
        Color(153, 172, 191, 255),
    );
    for (index, (kind, label)) in pinned.iter().enumerate() {
        let x = view_x + 46 + index as i32 * 44;
        let running = ctx.windows.iter().rev().find(|w| w.kind == *kind);
        let focused = running.is_some_and(|w| w.focused && !w.minimized);
        if focused {
            p.border(
                Rect::new(x, y + 4, 40, 40),
                Color(255, 255, 255, 195),
                4,
                Color(255, 255, 255, 110),
            );
        }
        let action = running
            .map(|w| w.action(if focused { "minimize" } else { "focus" }))
            .unwrap_or_else(|| format!("shell:launch:{kind}"));
        p.platform_icon(
            Rect::new(x + 8, y + 10, 24, 24),
            "windows",
            kind,
            &action,
            label,
        );
        p.region(Rect::new(x, y + 4, 40, 40), &action, label);
        if running.is_some() {
            p.box_(
                Rect::new(
                    x + if focused { 12 } else { 17 },
                    y + 40,
                    if focused { 16 } else { 6 },
                    3,
                ),
                if focused { ACCENT } else { MUTED },
                2,
            );
        }
    }
    // System tray is compact enough not to collide with centered launch controls.
    if ctx.width >= 760 {
        let tx = ctx.width as i32 - 158;
        p.region(
            Rect::new(tx, y + 4, 61, 40),
            "shell:panel:quick",
            "Network and volume",
        );
        p.line(
            vec![(tx + 4, y + 24), (tx + 8, y + 20), (tx + 12, y + 24)],
            INK,
            1,
        );
        p.line(
            vec![
                (tx + 22, y + 20),
                (tx + 26, y + 18),
                (tx + 30, y + 18),
                (tx + 34, y + 20),
            ],
            INK,
            1,
        );
        p.line(
            vec![(tx + 24, y + 24), (tx + 28, y + 22), (tx + 32, y + 24)],
            INK,
            1,
        );
        p.box_(Rect::new(tx + 27, y + 27, 3, 3), INK, 2);
        p.path(
            vec![
                (tx + 41, y + 22),
                (tx + 45, y + 22),
                (tx + 50, y + 18),
                (tx + 50, y + 30),
                (tx + 45, y + 26),
                (tx + 41, y + 26),
            ],
            INK,
        );
        p.line(
            vec![(tx + 53, y + 20), (tx + 56, y + 24), (tx + 53, y + 28)],
            INK,
            1,
        );
        let minute = (ctx.clock_us / 60_000_000) % 60;
        let hours = (9 + ctx.clock_us / 3_600_000_000) % 24;
        let time = format!(
            "{}:{minute:02} {}",
            if hours.is_multiple_of(12) {
                12
            } else {
                hours % 12
            },
            if hours < 12 { "AM" } else { "PM" }
        );
        p.region(
            Rect::new(tx + 64, y + 3, 88, 42),
            "shell:panel:calendar",
            "Date and time",
        );
        p.text(tx + 69, y + 6, 85, &time, 12, INK);
        let (year, month, day, _) = date(ctx.clock_us);
        p.text(
            tx + 69,
            y + 24,
            85,
            &format!("{month}/{day}/{year}"),
            12,
            INK,
        );
        p.region(
            Rect::new(ctx.width as i32 - 5, y, 5, 48),
            "shell:desktop",
            "Show desktop",
        );
    }
    if ctx.launcher_open {
        start_menu(p, ctx);
    }
    if let Some(panel) = ctx.panel {
        panel_surface(p, ctx, panel);
    }
}

fn start_menu(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = 600.min(ctx.width.saturating_sub(24));
    let height = 552.min(ctx.height.saturating_sub(76));
    let x = (ctx.width - width) as i32 / 2;
    let y = ctx.height as i32 - 60 - height as i32;
    let panel = Rect::new(x, y, width, height);
    p.shadow(panel, 8);
    p.border(
        panel,
        Color(243, 247, 252, 252),
        8,
        Color(255, 255, 255, 225),
    );
    let inset = 32;
    p.border(
        Rect::new(x + inset, y + 28, width.saturating_sub(64), 34),
        Color(255, 255, 255, 235),
        17,
        Color(210, 217, 226, 255),
    );
    p.region(
        Rect::new(x + inset, y + 28, width.saturating_sub(64), 34),
        "shell:search",
        "Search apps",
    );
    search_mark(p, x + 47, y + 37);
    p.text(
        x + 74,
        y + 36,
        width.saturating_sub(110),
        "Search for apps",
        14,
        MUTED,
    );
    heading(p, x + 48, y + 92, 180, "Pinned", 14);
    let columns = if width < 460 { 4 } else { 6 };
    let stride = (width.saturating_sub(64) / columns) as i32;
    for (index, (kind, label)) in APPS
        .iter()
        .filter(|(kind, _)| ctx.installed(kind))
        .enumerate()
    {
        let ix = x + 32 + index as i32 % columns as i32 * stride;
        let iy = y + 130 + (index as i32 / columns as i32) * 88;
        let action = format!("shell:launch:{kind}");
        p.platform_icon(
            Rect::new(ix + (stride - 32) / 2, iy, 32, 32),
            "windows",
            kind,
            &action,
            label,
        );
        p.region(Rect::new(ix, iy - 7, stride as u32, 77), &action, label);
        p.text(
            ix + 3,
            iy + 39,
            stride.saturating_sub(4) as u32,
            label,
            12,
            INK,
        );
    }
    if height > 430 {
        let recent_y = y + 324;
        heading(
            p,
            x + 48,
            recent_y,
            width.saturating_sub(96),
            "Recommended",
            14,
        );
        if ctx.windows.is_empty() {
            p.text(
                x + 48,
                recent_y + 35,
                width.saturating_sub(96),
                "Your open applications will appear here.",
                13,
                MUTED,
            );
        } else {
            let rows = ((height as i32 - 60 - 324 - 34) / 57).clamp(0, 2) as usize;
            for (index, w) in ctx.windows.iter().rev().take(rows * 2).enumerate() {
                let rx = x + 40 + (index % 2) as i32 * ((width as i32 - 80) / 2);
                let ry = recent_y + 34 + (index / 2) as i32 * 57;
                p.platform_icon(
                    Rect::new(rx + 8, ry + 4, 28, 28),
                    "windows",
                    &w.kind,
                    &w.action("focus"),
                    &w.title,
                );
                p.region(
                    Rect::new(rx, ry, width.saturating_sub(80) / 2, 49),
                    &w.action("focus"),
                    &format!("Switch to {}", w.title),
                );
                p.text(
                    rx + 47,
                    ry + 3,
                    width.saturating_sub(180) / 2,
                    &w.title,
                    12,
                    INK,
                );
                p.text(
                    rx + 47,
                    ry + 22,
                    width.saturating_sub(180) / 2,
                    if w.minimized {
                        "Minimized"
                    } else {
                        "Open application"
                    },
                    11,
                    MUTED,
                );
            }
        }
    }
    let foot = y + height as i32 - 60;
    p.box_(
        Rect::new(x + 1, foot, width.saturating_sub(2), 59),
        Color(227, 235, 245, 255),
        7,
    );
    p.box_(
        Rect::new(x + 1, foot, width.saturating_sub(2), 12),
        Color(227, 235, 245, 255),
        0,
    );
    p.asset(Rect::new(x + 47, foot + 17, 25, 25), "icon/windows/files");
    p.text(
        x + 85,
        foot + 21,
        width.saturating_sub(130),
        "This PC",
        12,
        INK,
    );
}

pub fn window_frame(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let r = w.rect;
    let radius = if w.maximized { 0 } else { 8 };
    if !w.maximized {
        p.shadow(r, radius);
    }
    p.border(
        r,
        Color(250, 250, 250, 255),
        radius,
        if w.focused {
            Color(156, 168, 182, 255)
        } else {
            Color(188, 195, 205, 255)
        },
    );
    p.box_(
        Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(2), 37),
        if w.focused {
            Color(237, 242, 250, 255)
        } else {
            Color(244, 245, 248, 255)
        },
        radius,
    );
    p.box_(
        Rect::new(r.x + 1, r.y + 29, r.width.saturating_sub(2), 9),
        Color(237, 242, 250, 255),
        0,
    );
    p.region(
        Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(139), 37),
        &w.action("drag"),
        &format!("Move {}", w.title),
    );
    p.asset(
        Rect::new(r.x + 12, r.y + 10, 18, 18),
        &format!("icon/windows/{}", w.kind),
    );
    p.text(
        r.x + 39,
        r.y + 10,
        r.width.saturating_sub(192),
        &w.title,
        12,
        if w.focused { INK } else { MUTED },
    );
    let right = r.x + r.width as i32;
    for (i, (verb, label)) in [
        ("minimize", "Minimize"),
        (
            "maximize",
            if w.maximized {
                "Restore down"
            } else {
                "Maximize"
            },
        ),
        ("close", "Close"),
    ]
    .iter()
    .enumerate()
    {
        let x = right - 138 + i as i32 * 46;
        let hit = Rect::new(x, r.y + 1, 46, 36);
        let hovered = ctx.hover.is_some_and(|(px, py)| hit.contains(px, py));
        if hovered {
            p.box_(
                hit,
                if *verb == "close" {
                    Color(196, 43, 28, 255)
                } else {
                    Color(220, 227, 236, 255)
                },
                0,
            );
        }
        let glyph = if hovered && *verb == "close" {
            Color::WHITE
        } else {
            INK
        };
        p.region(hit, &w.action(verb), label);
        let gx = x + 18;
        let gy = r.y + 14;
        match *verb {
            "minimize" => p.line(vec![(gx, gy + 5), (gx + 10, gy + 5)], glyph, 1),
            "maximize" => {
                if w.maximized {
                    p.border(Rect::new(gx + 3, gy - 1, 9, 9), Color(0, 0, 0, 0), 1, INK);
                }
                p.border(
                    Rect::new(gx, gy + if w.maximized { 2 } else { 0 }, 10, 10),
                    Color(237, 242, 250, 255),
                    1,
                    INK,
                );
            }
            _ => {
                p.line(vec![(gx, gy), (gx + 9, gy + 9)], glyph, 1);
                p.line(vec![(gx + 9, gy), (gx, gy + 9)], glyph, 1);
            }
        }
    }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
fn month_days(year: u64, month: u64) -> u64 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}
fn date(clock_us: u64) -> (u64, u64, u64, u64) {
    let elapsed = clock_us / 86_400_000_000;
    let (mut year, mut month, mut day) = (2026, 9, 17 + elapsed);
    while day > month_days(year, month) {
        day -= month_days(year, month);
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }
    let weekday = (4 + elapsed) % 7;
    (year, month, day, (weekday + 7 - (day - 1) % 7) % 7)
}

fn panel_surface(p: &mut Painter, ctx: &ShellContext<'_>, panel: &str) {
    let width = if panel == "overview" { 720 } else { 540 }.min(ctx.width.saturating_sub(24));
    let height = 410.min(ctx.height.saturating_sub(80));
    let x = (ctx.width - width) as i32 / 2;
    let y = ctx.height as i32 - 60 - height as i32;
    let r = Rect::new(x, y, width, height);
    p.region(
        Rect::new(0, 0, ctx.width, ctx.height.saturating_sub(48)),
        "shell:dismiss",
        "Dismiss panel",
    );
    p.shadow(r, 8);
    p.border(r, Color(244, 247, 252, 252), 8, Color(255, 255, 255, 220));
    // Absorb clicks inside the flyout instead of activating an underlying window.
    p.region(r, "shell:noop", "Panel");
    match panel {
        "search" | "spotlight" => {
            p.border(
                Rect::new(x + 24, y + 24, width.saturating_sub(48), 38),
                Color::WHITE,
                18,
                Color(191, 204, 221, 255),
            );
            search_mark(p, x + 39, y + 35);
            p.text(
                x + 66,
                y + 34,
                width.saturating_sub(95),
                if ctx.search.is_empty() {
                    "Type to search"
                } else {
                    ctx.search
                },
                14,
                INK,
            );
            p.text(x + 30, y + 84, width.saturating_sub(60), "Apps", 14, INK);
            let query = ctx.search.to_lowercase();
            for (index, (kind, label)) in APPS
                .iter()
                .filter(|(kind, label)| {
                    ctx.installed(kind)
                        && (query.is_empty()
                            || label.to_lowercase().contains(&query)
                            || kind.contains(&query))
                })
                .take(5)
                .enumerate()
            {
                let ry = y + 115 + index as i32 * 49;
                let action = format!("shell:launch:{kind}");
                p.platform_icon(
                    Rect::new(x + 37, ry, 28, 28),
                    "windows",
                    kind,
                    &action,
                    label,
                );
                p.region(
                    Rect::new(x + 24, ry - 7, width.saturating_sub(48), 45),
                    &action,
                    label,
                );
                p.text(x + 83, ry + 4, width.saturating_sub(115), label, 14, INK);
            }
        }
        "overview" | "taskview" => {
            p.text(
                x + 28,
                y + 23,
                width.saturating_sub(56),
                "Task view",
                18,
                INK,
            );
            if ctx.windows.is_empty() {
                p.text(
                    x + 28,
                    y + 75,
                    width.saturating_sub(56),
                    "No open windows",
                    14,
                    MUTED,
                );
            }
            let card_width = (width.saturating_sub(72) / 2).max(1);
            for (index, w) in ctx.windows.iter().rev().take(4).enumerate() {
                let cx = x + 24 + (index % 2) as i32 * (card_width as i32 + 24);
                let cy = y + 66 + (index / 2) as i32 * 147;
                p.border(
                    Rect::new(cx, cy, card_width, 127),
                    Color(255, 255, 255, 220),
                    7,
                    if w.focused {
                        ACCENT
                    } else {
                        Color(204, 214, 227, 255)
                    },
                );
                p.region(
                    Rect::new(cx, cy, card_width, 127),
                    &w.action("focus"),
                    &format!("Switch to {}", w.title),
                );
                p.asset(
                    Rect::new(cx + 14, cy + 13, 23, 23),
                    &format!("icon/windows/{}", w.kind),
                );
                p.text(
                    cx + 46,
                    cy + 16,
                    card_width.saturating_sub(63),
                    &w.title,
                    12,
                    INK,
                );
                p.asset(
                    Rect::new(cx + (card_width as i32 - 46) / 2, cy + 55, 46, 46),
                    &format!("icon/windows/{}", w.kind),
                );
            }
        }
        "calendar" => {
            p.text(
                x + 28,
                y + 23,
                width.saturating_sub(56),
                &format!(
                    "{}, {} {}",
                    [
                        "Sunday",
                        "Monday",
                        "Tuesday",
                        "Wednesday",
                        "Thursday",
                        "Friday",
                        "Saturday"
                    ][(4 + ctx.clock_us / 86_400_000_000) as usize % 7],
                    MONTHS[(date(ctx.clock_us).1 - 1) as usize],
                    date(ctx.clock_us).2
                ),
                18,
                INK,
            );
            p.text(
                x + 28,
                y + 67,
                width.saturating_sub(56),
                &format!(
                    "{} {}",
                    MONTHS[(date(ctx.clock_us).1 - 1) as usize],
                    date(ctx.clock_us).0
                ),
                14,
                INK,
            );
            let col = (width.saturating_sub(48) / 7) as i32;
            for (i, day) in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
                .iter()
                .enumerate()
            {
                p.text(x + 31 + i as i32 * col, y + 107, col as u32, day, 12, MUTED);
            }
            let (year, month, today, offset) = date(ctx.clock_us);
            for day in 1..=month_days(year, month) as i32 {
                let cell = day - 1 + offset as i32;
                let cx = x + 24 + (cell % 7) * col;
                let cy = y + 143 + (cell / 7) * 35;
                if day == today as i32 {
                    p.box_(Rect::new(cx + 1, cy - 6, 34, 34), ACCENT, 17);
                }
                p.text(
                    cx + 10,
                    cy,
                    col as u32,
                    &day.to_string(),
                    13,
                    if day == today as i32 {
                        Color::WHITE
                    } else {
                        INK
                    },
                );
            }
            if ctx.installed("calendar") {
                p.region(
                    Rect::new(x + 24, y + height as i32 - 43, width.saturating_sub(48), 32),
                    "shell:launch:calendar",
                    "Open calendar",
                );
                p.text(
                    x + 31,
                    y + height as i32 - 37,
                    width.saturating_sub(62),
                    "Open calendar",
                    13,
                    ACCENT,
                );
            }
        }
        _ => {
            p.text(
                x + 28,
                y + 25,
                width.saturating_sub(56),
                "Quick settings",
                18,
                INK,
            );
            p.text(
                x + 28,
                y + 74,
                width.saturating_sub(56),
                "Network and system",
                14,
                MUTED,
            );
            for (index, (kind, label)) in [
                ("terminal", "Open Terminal"),
                ("files", "Open File Explorer"),
                ("browser", "Open Browser"),
            ]
            .iter()
            .filter(|(kind, _)| ctx.installed(kind))
            .enumerate()
            {
                let ry = y + 122 + index as i32 * 69;
                let action = format!("shell:launch:{kind}");
                p.border(
                    Rect::new(x + 25, ry - 10, width.saturating_sub(50), 55),
                    Color(255, 255, 255, 230),
                    5,
                    Color(221, 227, 235, 255),
                );
                p.platform_icon(
                    Rect::new(x + 39, ry, 28, 28),
                    "windows",
                    kind,
                    &action,
                    label,
                );
                p.region(
                    Rect::new(x + 25, ry - 10, width.saturating_sub(50), 55),
                    &action,
                    label,
                );
                p.text(x + 86, ry + 4, width.saturating_sub(115), label, 14, INK);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_scene::DesktopTheme;
    fn context<'a>(windows: &'a [WindowView], apps: &'a [String]) -> ShellContext<'a> {
        ShellContext {
            theme: DesktopTheme::Windows,
            width: 1280,
            height: 800,
            clock_us: 0,
            title: "",
            launcher_open: false,
            active: true,
            windows,
            installed_apps: apps,
            panel: None,
            search: "",
            hover: None,
        }
    }
    #[test]
    fn caption_and_taskbar_target_the_correct_window() {
        let windows = vec![WindowView {
            id: 42,
            title: "Notes".into(),
            kind: "editor".into(),
            rect: Rect::new(100, 100, 700, 500),
            focused: true,
            maximized: false,
            minimized: false,
            content: None,
        }];
        let ctx = context(&windows, &[]);
        let mut p = Painter::new(1280, 800);
        window_frame(&mut p, &ctx, &windows[0]);
        for (x, action) in [
            (150, "drag"),
            (670, "minimize"),
            (720, "maximize"),
            (770, "close"),
        ] {
            assert_eq!(
                p.scene
                    .hit_test(x, 115)
                    .and_then(|n| n.interaction.as_deref()),
                Some(format!("window:42:{action}").as_str())
            );
        }
        chrome(&mut p, &ctx);
        assert!(p
            .scene
            .nodes
            .iter()
            .any(|n| n.interaction.as_deref() == Some("window:42:minimize") && n.bounds.y >= 752));
    }
    #[test]
    fn start_menu_only_offers_installed_apps() {
        let apps = vec!["terminal".into()];
        let mut ctx = context(&[], &apps);
        ctx.launcher_open = true;
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        let launches: Vec<_> = p
            .scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.as_deref())
            .filter(|a| a.starts_with("shell:launch:"))
            .collect();
        assert!(!launches.is_empty());
        assert!(launches.iter().all(|a| *a == "shell:launch:terminal"));
    }
}
