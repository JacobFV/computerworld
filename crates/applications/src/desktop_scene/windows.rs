//! Windows 11 presentation: Mica taskbar and flyouts, tabbed title bars for the inbox
//! applications, 46 px caption buttons and 8 px window corners. Geometry follows the
//! Fluent tokens; every control that accepts input dispatches a simulator action.
use super::shared::{Align, Painter, ShellContext, WindowView};
use cw_scene::{Color, Rect};

const INK: Color = Color::rgb(27, 27, 28);
const MUTED: Color = Color::rgb(96, 96, 100);
const FAINT: Color = Color::rgb(140, 140, 146);
const ACCENT: Color = Color::rgb(0, 95, 184);
const STROKE: Color = Color(0, 0, 0, 22);
const CAPTION: Color = Color::rgb(232, 238, 246);
const CAPTION_INACTIVE: Color = Color::rgb(243, 243, 243);
const TAB: Color = Color::rgb(249, 250, 252);
/// Taskbar pins: the handful Windows keeps out of Start.
const PINNED: [(&str, &str); 5] = [
    ("browser", "Microsoft Edge"),
    ("files", "File Explorer"),
    ("mail", "Outlook"),
    ("terminal", "Terminal"),
    ("editor", "Notepad"),
];
/// Every application Start can present, in its pinned-grid order.
const APPS: [(&str, &str); 17] = [
    ("browser", "Edge"),
    ("files", "File Explorer"),
    ("terminal", "Terminal"),
    ("editor", "Notepad"),
    ("mail", "Outlook"),
    ("calendar", "Calendar"),
    ("chat", "Teams"),
    ("docs", "Word"),
    ("notes", "Sticky Notes"),
    ("contacts", "People"),
    ("photos", "Photos"),
    ("music", "Media Player"),
    ("maps", "Maps"),
    ("weather", "Weather"),
    ("calculator", "Calculator"),
    ("clock", "Clock"),
    ("settings", "Settings"),
];

fn app_name(kind: &str) -> &str {
    APPS.iter()
        .find(|(k, _)| *k == kind)
        .map_or(kind, |(_, n)| *n)
}
fn basename(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("This PC")
}
fn shadowed_label(p: &mut Painter, x: i32, y: i32, width: u32, text: &str) {
    for (dx, dy, alpha) in [(1, 1, 170), (0, 2, 70), (-1, 1, 60)] {
        p.label(
            x + dx,
            y + dy,
            width,
            text,
            12,
            Color(0, 0, 0, alpha),
            false,
            Align::Center,
        );
    }
    p.label(x, y, width, text, 12, Color::WHITE, false, Align::Center);
}

pub fn background(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.asset(Rect::new(0, 0, ctx.width, ctx.height), "wallpaper/windows");
    if !ctx.installed("files") {
        return;
    }
    // The desktop shortcut opens the real file manager. No inert recycle-bin fixture.
    let slot = Rect::new(4, 8, 76, 84);
    if ctx.selected("files") {
        p.border(slot, Color(255, 255, 255, 60), 3, Color(255, 255, 255, 110));
    } else if ctx.hovered(slot) {
        p.border(slot, Color(255, 255, 255, 36), 3, Color(255, 255, 255, 60));
    }
    p.platform_icon(
        Rect::new(18, 14, 48, 48),
        "windows",
        "files",
        "shell:open:files",
        "Open File Explorer",
    );
    shadowed_label(p, 4, 66, 76, "File Explorer");
}

fn start_mark(p: &mut Painter, x: i32, y: i32) {
    for (dx, dy, color) in [
        (0, 0, Color::rgb(56, 178, 250)),
        (12, 0, Color::rgb(26, 151, 240)),
        (0, 12, Color::rgb(26, 151, 240)),
        (12, 12, Color::rgb(0, 120, 215)),
    ] {
        p.box_(Rect::new(x + dx, y + dy, 11, 11), color, 1);
    }
}
/// Mica: heavily blurred wallpaper under a light, nearly opaque tint.
fn mica(p: &mut Painter, r: Rect, radius: u32) {
    p.glass(r, radius, 40, Color(243, 246, 250, 222), None);
}
fn flyout(p: &mut Painter, r: Rect) {
    p.drop_shadow(r, 8, 28, 90, 12);
    mica(p, r, 8);
    p.border(r, Color::TRANSPARENT, 8, Color(0, 0, 0, 40));
    // Absorb clicks inside the flyout instead of activating an underlying window.
    p.region(r, "shell:noop", "Panel");
}

pub fn chrome(p: &mut Painter, ctx: &ShellContext<'_>) {
    // A locked or powered-off display owns the whole screen; nothing under it is reachable.
    if !ctx.awake() {
        locked_screen(p, ctx);
        return;
    }
    if ctx.panel == Some("overview") {
        task_view(p, ctx);
    }
    taskbar(p, ctx);
    // Start is painted before the panels so a flyout opened from it — the power menu —
    // sits over the menu it came from whenever both are open at once.
    if ctx.launcher_open {
        start_menu(p, ctx);
    }
    match ctx.panel {
        Some("overview") | None => {}
        Some(panel) => panel_surface(p, ctx, panel),
    }
}

/// Lock and power-off screens. There is no authentication model, so the surface is one
/// real `shell:power:wake` target rather than a fabricated password prompt.
fn locked_screen(p: &mut Painter, ctx: &ShellContext<'_>) {
    let full = Rect::new(0, 0, ctx.width, ctx.height);
    let locked = ctx.screen == crate::ScreenState::Locked;
    if locked {
        p.asset(full, "wallpaper/windows");
        p.box_(full, Color(10, 14, 26, 130), 0);
    } else {
        p.box_(full, Color::rgb(3, 4, 6), 0);
    }
    p.button(
        full,
        Color::TRANSPARENT,
        0,
        "shell:power:wake",
        if locked {
            "Sign in as alice"
        } else {
            "Turn on the display"
        },
    );
    let mid = ctx.height as i32 / 2;
    let centre = ctx.width as i32 / 2;
    if !locked {
        let button = Rect::new(centre - 84, mid - 22, 168, 44);
        p.button(
            button,
            Color(255, 255, 255, 16),
            6,
            "shell:power:wake",
            "Turn on the display",
        );
        p.border(button, Color::TRANSPARENT, 6, Color(255, 255, 255, 60));
        p.symbol(
            "power",
            button.x + 24,
            button.y + 14,
            16,
            Color(255, 255, 255, 190),
        );
        p.left(
            button.x + 54,
            button.y + 13,
            100,
            "Turn on",
            14,
            Color(255, 255, 255, 190),
        );
        return;
    }
    let date = ctx.date();
    p.label(
        0,
        mid - 170,
        ctx.width,
        &ctx.time12(),
        56,
        Color::WHITE,
        true,
        Align::Center,
    );
    p.label(
        0,
        mid - 80,
        ctx.width,
        &format!(
            "{}, {} {}",
            date.weekday_name(),
            date.month_name(),
            date.day
        ),
        20,
        Color(255, 255, 255, 220),
        false,
        Align::Center,
    );
    p.circle(centre, mid + 60, 40, Color(255, 255, 255, 46));
    p.symbol(
        "person",
        centre - 22,
        mid + 38,
        44,
        Color(255, 255, 255, 210),
    );
    p.label(
        0,
        mid + 116,
        ctx.width,
        "alice",
        18,
        Color::WHITE,
        true,
        Align::Center,
    );
    p.symbol("lock", centre - 8, mid + 152, 16, Color(255, 255, 255, 160));
    p.label(
        0,
        mid + 178,
        ctx.width,
        "Click anywhere to sign in",
        14,
        Color(255, 255, 255, 190),
        false,
        Align::Center,
    );
}

fn taskbar(p: &mut Painter, ctx: &ShellContext<'_>) {
    let y = ctx.height.saturating_sub(48) as i32;
    mica(p, Rect::new(0, y, ctx.width, 48), 0);
    p.hline(0, y, ctx.width, Color(0, 0, 0, 26));
    let plate = Color(255, 255, 255, 150);
    let search_width: i32 = if ctx.width >= 1100 { 200 } else { 104 };
    let pinned: Vec<_> = PINNED
        .iter()
        .filter(|(kind, _)| ctx.installed(kind))
        .collect();
    let total = 44 + search_width + 8 + 44 + pinned.len() as i32 * 44;
    let left = ((ctx.width as i32 - total) / 2).max(4);
    let start = Rect::new(left, y + 4, 40, 40);
    p.button(
        start,
        if ctx.launcher_open || ctx.hovered(start) {
            plate
        } else {
            Color::TRANSPARENT
        },
        4,
        "shell:launcher",
        "Start",
    );
    start_mark(p, left + 9, y + 13);
    let search = Rect::new(left + 46, y + 8, search_width as u32, 32);
    p.button(
        search,
        Color(255, 255, 255, 200),
        16,
        "shell:search",
        "Search installed applications",
    );
    p.border(search, Color::TRANSPARENT, 16, Color(0, 0, 0, 30));
    p.symbol("search", search.x + 12, search.y + 8, 15, INK);
    p.left(
        search.x + 36,
        search.y + 7,
        search.width - 44,
        "Search",
        13,
        MUTED,
    );
    let view_x = left + 52 + search_width;
    let view = Rect::new(view_x, y + 4, 40, 40);
    if ctx.hovered(view) || ctx.panel == Some("overview") {
        p.box_(view, plate, 4);
    }
    p.region(view, "shell:overview", "Task view");
    p.border(
        Rect::new(view_x + 9, y + 15, 14, 14),
        Color(60, 60, 66, 255),
        3,
        Color(30, 30, 34, 255),
    );
    p.border(
        Rect::new(view_x + 16, y + 19, 15, 14),
        Color::rgb(250, 250, 252),
        3,
        Color(30, 30, 34, 255),
    );
    for (index, (kind, label)) in pinned.iter().enumerate() {
        let x = view_x + 44 + index as i32 * 44;
        let slot = Rect::new(x, y + 4, 40, 40);
        let running = ctx.windows.iter().rev().find(|w| w.kind == *kind);
        let focused = running.is_some_and(|w| w.focused && !w.minimized);
        let hovered = ctx.hovered(slot);
        if focused || hovered {
            p.border(
                slot,
                if focused {
                    plate
                } else {
                    Color(255, 255, 255, 100)
                },
                4,
                Color(0, 0, 0, 14),
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
        p.region(slot, &action, label);
        if running.is_some() {
            let width = if focused { 16 } else { 6 };
            p.box_(
                Rect::new(x + 20 - width / 2, y + 41, width as u32, 3),
                if focused { ACCENT } else { FAINT },
                2,
            );
        }
        if hovered && ctx.panel.is_none() && !ctx.launcher_open {
            let width = p.measure(label, 12, false) + 20;
            let tip = Rect::new(x + 20 - width as i32 / 2, y - 36, width, 28);
            p.drop_shadow(tip, 4, 10, 60, 3);
            p.border(tip, Color::rgb(249, 249, 249), 4, Color(0, 0, 0, 38));
            p.center(tip.x, tip.y + 6, width, label, 12, INK);
        }
    }
    // System tray is compact enough not to collide with centred launch controls.
    if ctx.width >= 760 {
        let right = ctx.width as i32;
        let date = ctx.date();
        let clock = Rect::new(right - 106, y + 4, 92, 40);
        if ctx.hovered(clock) || ctx.panel == Some("calendar") {
            p.box_(clock, plate, 4);
        }
        p.right(clock.x, y + 7, 84, &ctx.time12(), 12, INK);
        p.right(
            clock.x,
            y + 24,
            84,
            &format!("{}/{}/{}", date.month, date.day, date.year),
            12,
            INK,
        );
        p.region(clock, "shell:panel:calendar", "Date and time");
        let quick = Rect::new(right - 190, y + 4, 80, 40);
        if ctx.hovered(quick) || ctx.panel == Some("quick") {
            p.box_(quick, plate, 4);
        }
        // The tray reports the switches and levels the machine really holds.
        let radio = if ctx.switch("airplane_mode") {
            "airplane"
        } else if ctx.switch("wifi") {
            "wifi-fill"
        } else {
            "wifi"
        };
        let sound = if ctx.level("volume") == 0 {
            "volume-mute"
        } else {
            "volume"
        };
        for (i, symbol) in [radio, sound, "battery"].iter().enumerate() {
            p.symbol(
                symbol,
                quick.x + 8 + i as i32 * 24,
                y + 16,
                if i == 2 { 18 } else { 16 },
                if i == 0 && !ctx.switch("wifi") && !ctx.switch("airplane_mode") {
                    FAINT
                } else {
                    INK
                },
            );
        }
        p.region(quick, "shell:panel:quick", "Network, volume and battery");
        // Still greyed, deliberately: a tray overflow needs a model of which status
        // icons are promoted and which are hidden, and nothing else in the machine
        // wants one. The tray shows every icon it has, so none are hidden.
        p.symbol("chevron-up", right - 214, y + 18, 12, FAINT);
        p.disabled("Hidden icons");
        p.region(
            Rect::new(right - 6, y, 6, 48),
            "shell:desktop",
            "Show desktop",
        );
    }
}

fn start_menu(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = 642.min(ctx.width.saturating_sub(24));
    let height = 640.min(ctx.height.saturating_sub(72));
    let x = (ctx.width - width) as i32 / 2;
    let y = ctx.height as i32 - 60 - height as i32;
    flyout(p, Rect::new(x, y, width, height));
    let field = Rect::new(x + 32, y + 28, width.saturating_sub(64), 36);
    p.border(field, Color(255, 255, 255, 235), 18, Color(0, 0, 0, 34));
    p.region(field, "shell:search", "Search apps");
    p.symbol("search", field.x + 14, field.y + 10, 16, MUTED);
    p.left(
        field.x + 42,
        field.y + 9,
        field.width - 60,
        "Search for apps, settings, and documents",
        13,
        MUTED,
    );
    p.strong(x + 56, y + 92, 180, "Pinned", 14, INK);
    if width > 420 {
        // There is no separate all-apps page; the search flyout is that full list.
        let all = Rect::new(x + width as i32 - 136, y + 86, 84, 28);
        p.button(
            all,
            if ctx.hovered(all) {
                Color(255, 255, 255, 235)
            } else {
                Color(255, 255, 255, 190)
            },
            4,
            "shell:search",
            "All apps",
        );
        p.border(all, Color::TRANSPARENT, 4, STROKE);
        p.left(all.x + 10, all.y + 6, 60, "All apps", 12, INK);
        p.symbol("chevron-right", all.x + 66, all.y + 9, 10, INK);
    }
    let columns: u32 = if width < 460 { 4 } else { 6 };
    let stride = (width.saturating_sub(64) / columns) as i32;
    for (index, (kind, label)) in APPS
        .iter()
        .filter(|(kind, _)| ctx.installed(kind))
        .enumerate()
    {
        let ix = x + 32 + index as i32 % columns as i32 * stride;
        let iy = y + 128 + (index as i32 / columns as i32) * 84;
        // The account row owns the foot of the menu; search carries the overflow.
        if iy + 80 > y + height as i32 - 64 {
            break;
        }
        let action = format!("shell:launch:{kind}");
        let slot = Rect::new(ix + 2, iy, stride as u32 - 4, 80);
        if ctx.hovered(slot) {
            p.border(slot, Color(255, 255, 255, 170), 4, STROKE);
        }
        p.platform_icon(
            Rect::new(ix + (stride - 36) / 2, iy + 10, 36, 36),
            "windows",
            kind,
            &action,
            label,
        );
        p.region(slot, &action, label);
        p.center(ix, iy + 53, stride as u32, label, 12, INK);
    }
    if height > 430 {
        let recent_y = y + 318;
        p.strong(
            x + 56,
            recent_y,
            width.saturating_sub(112),
            "Recommended",
            14,
            INK,
        );
        if ctx.windows.is_empty() {
            p.paragraph(
                x + 56,
                recent_y + 36,
                width.saturating_sub(112),
                "Your open applications will appear here.",
                13,
                MUTED,
            );
        } else {
            let rows = ((height as i32 - 64 - 318 - 40) / 56).clamp(0, 3) as usize;
            let column = (width as i32 - 80) / 2;
            for (index, w) in ctx.windows.iter().rev().take(rows * 2).enumerate() {
                let rx = x + 40 + (index % 2) as i32 * column;
                let ry = recent_y + 36 + (index / 2) as i32 * 56;
                let row = Rect::new(rx, ry, column as u32, 52);
                if ctx.hovered(row) {
                    p.border(row, Color(255, 255, 255, 170), 4, STROKE);
                }
                p.platform_icon(
                    Rect::new(rx + 14, ry + 10, 32, 32),
                    "windows",
                    &w.kind,
                    &w.action("focus"),
                    &w.title,
                );
                p.region(row, &w.action("focus"), &format!("Switch to {}", w.title));
                p.left(rx + 58, ry + 9, column as u32 - 70, &tab_title(w), 12, INK);
                p.left(
                    rx + 58,
                    ry + 27,
                    column as u32 - 70,
                    if w.minimized { "Minimized" } else { "Open now" },
                    12,
                    MUTED,
                );
            }
        }
    }
    let foot = y + height as i32 - 64;
    p.box_(Rect::new(x + 1, foot, width - 2, 63), Color(0, 0, 0, 10), 0);
    p.hline(x + 1, foot, width - 2, STROKE);
    let user = Rect::new(x + 52, foot + 12, 168, 40);
    if ctx.hovered(user) {
        p.border(user, Color(255, 255, 255, 170), 4, STROKE);
    }
    p.circle(x + 72, foot + 32, 16, Color::rgb(203, 214, 228));
    p.symbol("person", x + 63, foot + 23, 18, Color::rgb(80, 96, 116));
    p.left(x + 98, foot + 24, 110, "alice", 12, INK);
    p.region(user, "shell:settings", "Account settings for alice");
    let power = Rect::new(x + width as i32 - 92, foot + 12, 40, 40);
    if ctx.hovered(power) {
        p.border(power, Color(255, 255, 255, 170), 4, STROKE);
    }
    p.symbol("power", power.x + 12, power.y + 12, 16, INK);
    p.region(power, POWER_MENU, "Power");
}

/// Tab caption of an inbox application window.
fn tab_title(w: &WindowView) -> String {
    match w.kind.as_str() {
        "files" => basename(&w.document).to_owned(),
        "editor" if w.document.is_empty() => "Untitled".into(),
        "editor" => basename(&w.document).to_owned(),
        "terminal" => "Windows PowerShell".into(),
        "browser" if w.caption.is_empty() => "New tab".into(),
        "browser" => w.caption.clone(),
        _ => w.title.clone(),
    }
}

pub fn window_frame(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let r = w.rect;
    let radius = super::corner_radius(ctx.theme, w.maximized);
    if !w.maximized {
        if w.focused {
            p.drop_shadow(r, radius, 30, 95, 14);
        } else {
            p.drop_shadow(r, radius, 16, 60, 6);
        }
    }
    let caption = if w.focused { CAPTION } else { CAPTION_INACTIVE };
    p.border(
        r,
        Color::rgb(249, 249, 249),
        radius,
        if w.focused {
            Color(0, 0, 0, 92)
        } else {
            Color(0, 0, 0, 60)
        },
    );
    let inner = Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(2), 37);
    p.box_(inner, caption, radius.saturating_sub(1));
    p.box_(Rect::new(inner.x, r.y + 20, inner.width, 18), caption, 0);
    p.region(
        Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(139), 37),
        &w.action("drag"),
        &format!("Move {}", w.title),
    );
    let ink = if w.focused { INK } else { FAINT };
    if !w.tabs.is_empty() {
        // Explorer and Edge both place their tab strip in the caption. Every tab, its
        // close button and the new-tab button drive the application's real tab list.
        let room = r.width.saturating_sub(200);
        let width = (240.min(room / w.tabs.len().max(1) as u32))
            .max(96)
            .min(room);
        let mut x = r.x + 8;
        for (index, label) in w.tabs.iter().enumerate() {
            if x + width as i32 > r.x + r.width as i32 - 170 {
                break;
            }
            let tab = Rect::new(x, r.y + 6, width, 32);
            let selected = index == w.active_tab;
            p.button(
                tab,
                if selected { TAB } else { Color::TRANSPARENT },
                7,
                &w.tab_select(index),
                label,
            );
            if selected {
                p.box_(Rect::new(tab.x, tab.y + 16, tab.width, 16), TAB, 0);
            }
            p.asset(
                Rect::new(tab.x + 10, tab.y + 8, 16, 16),
                &format!("icon/windows/{}", w.kind),
            );
            p.left(
                tab.x + 34,
                tab.y + 8,
                width.saturating_sub(64),
                label,
                12,
                ink,
            );
            if selected && w.modified {
                p.circle(tab.x + width as i32 - 17, tab.y + 16, 4, ink);
            } else {
                let close = Rect::new(tab.x + width as i32 - 28, tab.y + 5, 22, 22);
                p.button(
                    close,
                    Color::TRANSPARENT,
                    4,
                    &w.tab_close(index),
                    "Close tab",
                );
                p.symbol("close", close.x + 6, close.y + 6, 11, ink);
            }
            x += width as i32 + 2;
        }
        let plus = Rect::new(x + 4, r.y + 6, 26, 26);
        p.button(plus, Color::TRANSPARENT, 4, &w.tab_new(), "New tab");
        p.symbol("plus", plus.x + 7, plus.y + 7, 13, ink);
    } else {
        p.asset(
            Rect::new(r.x + 12, r.y + 11, 16, 16),
            &format!("icon/windows/{}", w.kind),
        );
        p.left(
            r.x + 38,
            r.y + 11,
            r.width.saturating_sub(192),
            &w.title,
            12,
            ink,
        );
    }
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
        let hovered = ctx.hovered(hit);
        if hovered {
            if *verb == "close" {
                // The close plate follows the window's rounded top-right corner.
                p.box_(hit, Color::rgb(196, 43, 28), radius.saturating_sub(1));
                p.box_(Rect::new(x, r.y + 1, 30, 36), Color::rgb(196, 43, 28), 0);
                p.box_(Rect::new(x, r.y + 17, 46, 20), Color::rgb(196, 43, 28), 0);
            } else {
                p.box_(hit, Color(0, 0, 0, 20), 0);
            }
        }
        let glyph = if hovered && *verb == "close" {
            Color::WHITE
        } else {
            ink
        };
        p.region(hit, &w.action(verb), label);
        let gx = x + 18;
        let gy = r.y + 14;
        match *verb {
            "minimize" => p.line(vec![(gx, gy + 5), (gx + 10, gy + 5)], glyph, 1),
            "maximize" => {
                if w.maximized {
                    p.border(
                        Rect::new(gx + 2, gy - 1, 9, 9),
                        Color::TRANSPARENT,
                        2,
                        glyph,
                    );
                    p.box_(
                        Rect::new(gx, gy + 1, 9, 9),
                        if hovered {
                            Color::rgb(212, 217, 224)
                        } else {
                            caption
                        },
                        2,
                    );
                    p.border(Rect::new(gx, gy + 1, 9, 9), Color::TRANSPARENT, 2, glyph);
                } else {
                    p.border(Rect::new(gx, gy, 10, 10), Color::TRANSPARENT, 2, glyph);
                }
            }
            _ => {
                p.line(vec![(gx, gy), (gx + 10, gy + 10)], glyph, 1);
                p.line(vec![(gx + 10, gy), (gx, gy + 10)], glyph, 1);
            }
        }
    }
}

/// Edge toolbar beneath the caption's tab strip.
pub fn browser_chrome(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let r = super::window_content_rect(ctx.theme, w.rect);
    let ink = if w.focused { INK } else { FAINT };
    p.box_(Rect::new(r.x, r.y, r.width, 40), TAB, 0);
    p.hline(r.x, r.y + 39, r.width, STROKE);
    // History that does not exist is greyed rather than painted as a live control:
    // Back and Forward follow the tab's own history, Refresh needs a loaded page.
    for (i, (symbol, action, label, ready)) in [
        ("arrow-left", "back", "Back", w.can_go_back),
        ("arrow-right", "forward", "Forward", w.can_go_forward),
        ("reload", "reload", "Refresh", !w.document.is_empty()),
    ]
    .into_iter()
    .enumerate()
    {
        let hit = Rect::new(r.x + 6 + i as i32 * 36, r.y + 4, 34, 32);
        if ready && ctx.hovered(hit) {
            p.box_(hit, Color(0, 0, 0, 14), 4);
        }
        p.symbol(
            symbol,
            hit.x + 9,
            hit.y + 8,
            16,
            if ready { ink } else { FAINT },
        );
        if ready {
            p.region(hit, &w.action(&format!("content:shell:{action}")), label);
        } else {
            p.disabled(label);
        }
    }
    let trailing = if r.width > 520 { 84 } else { 12 };
    let field = Rect::new(
        r.x + 118,
        r.y + 5,
        r.width.saturating_sub(118 + trailing),
        30,
    );
    p.button(
        field,
        if w.editing {
            Color::WHITE
        } else {
            Color::rgb(238, 240, 244)
        },
        15,
        &w.action("content:shell:address"),
        "Address and search",
    );
    if w.editing {
        p.border(field, Color::TRANSPARENT, 15, ACCENT);
    }
    let typed = w
        .title
        .split_once(" — ")
        .map_or(w.title.as_str(), |(_, a)| a);
    let inner = field.width.saturating_sub(74);
    if typed.is_empty() {
        p.symbol("search", field.x + 12, field.y + 8, 14, MUTED);
        p.left(
            field.x + 36,
            field.y + 7,
            inner,
            "Search or enter web address",
            13,
            MUTED,
        );
    } else {
        p.symbol(
            if w.editing { "search" } else { "lock" },
            field.x + 12,
            field.y + 8,
            13,
            MUTED,
        );
        let end = p.left(field.x + 36, field.y + 7, inner, typed, 13, INK);
        if w.editing {
            p.box_(
                Rect::new(field.x + 37 + end as i32, field.y + 7, 1, 16),
                INK,
                0,
            );
        }
    }
    // Favourites are the machine's own bookmark list, shared by every browser window.
    // The star paints the position it reads back; an empty tab has no page to save.
    let star = Rect::new(field.x + field.width as i32 - 32, field.y + 3, 24, 24);
    let saved = ctx.bookmarked && w.focused;
    if w.document.is_empty() {
        p.symbol(
            "star-outline",
            star.x + 4,
            star.y + 4,
            15,
            Color(140, 140, 146, 150),
        );
        p.disabled("Add to favourites");
    } else {
        if ctx.hovered(star) {
            p.box_(star, Color(0, 0, 0, 14), 4);
        }
        p.symbol(
            if saved { "star" } else { "star-outline" },
            star.x + 4,
            star.y + 4,
            15,
            if saved {
                Color::rgb(232, 155, 20)
            } else {
                MUTED
            },
        );
        p.region(
            star,
            "shell:bookmark",
            if saved {
                "Remove from favourites"
            } else {
                "Add to favourites"
            },
        );
    }
    if r.width > 520 {
        let right = r.x + r.width as i32;
        // Edge calls this menu "Settings and more"; it opens the real settings panel.
        let more = Rect::new(right - 84, r.y + 6, 32, 28);
        if ctx.hovered(more) {
            p.box_(more, Color(0, 0, 0, 14), 4);
        }
        p.symbol("more", right - 72, r.y + 12, 16, ink);
        p.region(more, "shell:settings", "Settings and more");
        // There is one account on this machine, and its settings are where Start's own
        // account row leads, so the avatar goes to the same place rather than nowhere.
        let profile = Rect::new(right - 38, r.y + 8, 24, 24);
        if ctx.hovered(profile) {
            p.box_(profile, Color(0, 0, 0, 14), 12);
        }
        p.circle(right - 26, r.y + 20, 12, Color::rgb(216, 222, 231));
        p.symbol(
            "person",
            right - 33,
            r.y + 13,
            14,
            Color::rgb(122, 134, 150),
        );
        p.region(profile, "shell:settings", "Profile, alice");
    }
}

fn anchored(ctx: &ShellContext<'_>, width: u32, height: u32) -> Rect {
    let width = width.min(ctx.width.saturating_sub(24));
    let height = height.min(ctx.height.saturating_sub(72));
    Rect::new(
        ctx.width as i32 - width as i32 - 12,
        ctx.height as i32 - 60 - height as i32,
        width,
        height,
    )
}
fn dismiss_layer(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.region(
        Rect::new(0, 0, ctx.width, ctx.height.saturating_sub(48)),
        "shell:dismiss",
        "Dismiss panel",
    );
}

/// Start's power flyout owns a panel name of its own, and `chrome` paints it over an
/// open Start menu rather than instead of it — see the ordering there. Whether it
/// actually overlays depends on `DesktopState::panel_over_launcher` reaching this shell:
/// the field exists, but nothing sets it and `ShellContext` does not carry it, so today
/// opening the panel still clears `launcher_open` and the flyout stands alone.
const POWER_PANEL: &str = "power";
const POWER_MENU: &str = "shell:panel:power";

fn panel_surface(p: &mut Painter, ctx: &ShellContext<'_>, panel: &str) {
    dismiss_layer(p, ctx);
    match panel {
        "search" | "spotlight" => search(p, ctx),
        "calendar" | "notifications" => calendar(p, ctx),
        "settings" => settings(p, ctx),
        "context" => context_menu(p, ctx),
        POWER_PANEL => power_menu(p, ctx),
        "file" | "edit" | "view" | "app-settings" => notepad_menu(p, ctx, panel),
        _ => quick_settings(p, ctx),
    }
}

/// Notepad's File, Edit and View menus and its settings flyout, dropped under the
/// control in the Notepad window that opened them. Every entry changes the document,
/// the window or a real machine setting; a menu with no Notepad behind it is empty.
fn notepad_menu(p: &mut Painter, ctx: &ShellContext<'_>, panel: &str) {
    let Some(w) = ctx.windows.iter().find(|w| w.focused && w.kind == "editor") else {
        return;
    };
    let content = crate::desktop_scene::window_content_rect(
        crate::desktop_scene::DesktopTheme::Windows,
        w.rect,
    );
    let on = |value: bool| if value { "On" } else { "Off" };
    let date = ctx.date();
    let stamp = format!("{} {}/{}/{}", ctx.time12(), date.month, date.day, date.year);
    let wrap = ctx.settings.word_wrap;
    // (label, action, trailing text)
    let entries: Vec<(&str, String, String)> = match panel {
        "file" => vec![
            ("New window", "shell:new".into(), "Ctrl+N".into()),
            ("Save", "shell:save".into(), "Ctrl+S".into()),
            ("Close window", w.action("close"), "Ctrl+W".into()),
        ],
        "edit" => vec![("Time/Date", format!("shell:insert:{stamp}"), "F5".into())],
        "view" => vec![(
            "Word wrap",
            "shell:toggle:word_wrap".into(),
            if wrap { "✓".into() } else { String::new() },
        )],
        _ => vec![
            (
                "Word wrap",
                "shell:toggle:word_wrap".into(),
                on(wrap).into(),
            ),
            (
                "Dark theme",
                "shell:toggle:dark_mode".into(),
                on(ctx.settings.dark_mode).into(),
            ),
        ],
    };
    let x = match panel {
        "file" => content.x + 8,
        "edit" => content.x + 54,
        "view" => content.x + 100,
        _ => content.x + content.width as i32 - 252,
    };
    let r = Rect::new(
        x.clamp(4, ctx.width as i32 - 244),
        content.y + 34,
        240,
        entries.len() as u32 * 36 + 8,
    );
    flyout(p, r);
    for (i, (label, action, trailing)) in entries.iter().enumerate() {
        let row = Rect::new(r.x + 4, r.y + 4 + i as i32 * 36, r.width - 8, 36);
        if ctx.hovered(row) {
            p.box_(row, Color(0, 0, 0, 14), 4);
        }
        p.left(row.x + 14, row.y + 9, row.width - 90, label, 13, INK);
        p.right(row.x, row.y + 9, row.width - 14, trailing, 12, MUTED);
        p.region(row, action, label);
    }
}

/// `shell:power:*` really moves the display between Active, Locked and Off, so every
/// entry here changes machine state rather than decorating the menu.
fn power_menu(p: &mut Painter, ctx: &ShellContext<'_>) {
    let entries = [
        ("lock", "Lock", "shell:power:lock"),
        ("reload", "Restart", "shell:power:restart"),
        ("power", "Shut down", "shell:power:off"),
    ];
    let height = entries.len() as u32 * 36 + 8;
    // Anchored under the Start menu's power button, which is what opened it.
    let menu = 642.min(ctx.width.saturating_sub(24)) as i32;
    let anchor = (ctx.width as i32 - menu) / 2 + menu - 72;
    let r = Rect::new(
        (anchor - 108).clamp(4, (ctx.width as i32 - 220).max(4)),
        ctx.height as i32 - 60 - height as i32,
        216,
        height,
    );
    flyout(p, r);
    for (i, (symbol, label, action)) in entries.iter().enumerate() {
        let row = Rect::new(r.x + 4, r.y + 4 + i as i32 * 36, r.width - 8, 36);
        if ctx.hovered(row) {
            p.box_(row, Color(0, 0, 0, 14), 4);
        }
        p.symbol(symbol, row.x + 12, row.y + 10, 16, INK);
        p.left(row.x + 42, row.y + 9, row.width - 54, label, 13, INK);
        p.region(row, action, label);
    }
}

fn search(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = 642.min(ctx.width.saturating_sub(24));
    let height = 520.min(ctx.height.saturating_sub(72));
    let x = (ctx.width - width) as i32 / 2;
    let y = ctx.height as i32 - 60 - height as i32;
    flyout(p, Rect::new(x, y, width, height));
    let field = Rect::new(x + 24, y + 24, width.saturating_sub(48), 38);
    p.border(field, Color::WHITE, 19, Color(0, 0, 0, 34));
    // Without this the query field falls through to the flyout's click absorber.
    p.region(field, "shell:search", "Search box");
    p.box_(
        Rect::new(field.x + 18, field.y + 36, field.width - 36, 2),
        ACCENT,
        1,
    );
    p.symbol("search", field.x + 14, field.y + 11, 16, MUTED);
    if ctx.search.is_empty() {
        p.left(
            field.x + 42,
            field.y + 10,
            field.width - 60,
            "Type here to search",
            14,
            MUTED,
        );
    } else {
        let end = p.left(
            field.x + 42,
            field.y + 10,
            field.width - 60,
            ctx.search,
            14,
            INK,
        );
        p.box_(
            Rect::new(field.x + 43 + end as i32, field.y + 10, 1, 18),
            INK,
            0,
        );
    }
    p.strong(x + 32, y + 84, width.saturating_sub(64), "Apps", 13, INK);
    let query = ctx.search.to_lowercase();
    for (index, (kind, label)) in APPS
        .iter()
        .filter(|(kind, label)| {
            ctx.installed(kind)
                && (query.is_empty()
                    || label.to_lowercase().contains(&query)
                    || kind.contains(&query))
        })
        .take(7)
        .enumerate()
    {
        let ry = y + 112 + index as i32 * 46;
        if ry + 46 > y + height as i32 {
            break;
        }
        let row = Rect::new(x + 24, ry, width.saturating_sub(48), 42);
        let action = format!("shell:launch:{kind}");
        if index == 0 || ctx.hovered(row) {
            p.border(row, Color(255, 255, 255, 190), 4, STROKE);
        }
        p.platform_icon(
            Rect::new(x + 36, ry + 7, 28, 28),
            "windows",
            kind,
            &action,
            label,
        );
        p.region(row, &action, label);
        p.left(x + 78, ry + 5, width.saturating_sub(180), label, 13, INK);
        p.left(x + 78, ry + 22, width.saturating_sub(180), "App", 11, MUTED);
    }
}

fn calendar(p: &mut Painter, ctx: &ShellContext<'_>) {
    let date = ctx.date();
    // The header always reports the world's own day; the grid follows the paged month.
    let shown = ctx.panel_date();
    let cal = anchored(ctx, 336, 388);
    if cal.y > 150 {
        let notes = Rect::new(cal.x, cal.y - 112, cal.width, 100);
        flyout(p, notes);
        p.strong(notes.x + 18, notes.y + 14, 200, "Notifications", 13, INK);
        // Windows keeps do-not-disturb in this header, and it is a real system switch.
        let quiet = ctx.switch("do_not_disturb");
        let bell = Rect::new(notes.x + notes.width as i32 - 46, notes.y + 10, 32, 32);
        p.button(
            bell,
            if quiet {
                ACCENT
            } else {
                Color(255, 255, 255, 190)
            },
            4,
            "shell:toggle:do_not_disturb",
            &format!("Do not disturb, {}", if quiet { "on" } else { "off" }),
        );
        p.border(bell, Color::TRANSPARENT, 4, STROKE);
        p.symbol(
            if quiet { "dnd" } else { "bell" },
            bell.x + 8,
            bell.y + 8,
            16,
            if quiet { Color::WHITE } else { INK },
        );
        // What is listed here is what applications really posted; an empty feed says
        // so, and each row opens the one notice it belongs to.
        if ctx.notifications.is_empty() {
            p.center(
                notes.x,
                notes.y + 52,
                notes.width,
                if quiet {
                    "Notifications are silenced"
                } else {
                    "No new notifications"
                },
                13,
                MUTED,
            );
        } else {
            for (i, notice) in ctx.notifications.iter().take(2).enumerate() {
                let row = Rect::new(
                    notes.x + 12,
                    notes.y + 44 + i as i32 * 30,
                    notes.width - 24,
                    28,
                );
                if ctx.hovered(row) {
                    p.box_(row, Color(0, 0, 0, 14), 4);
                }
                p.asset(
                    Rect::new(row.x + 4, row.y + 5, 18, 18),
                    &format!("icon/windows/{}", notice.app),
                );
                p.left(
                    row.x + 30,
                    row.y + 6,
                    row.width - 90,
                    &notice.title,
                    12,
                    INK,
                );
                // Unread is a real flag on the notice, never inferred from its age.
                if !notice.seen {
                    p.circle(row.x + row.width as i32 - 58, row.y + 14, 3, ACCENT);
                }
                p.region(row, &format!("shell:notice:{i}"), &notice.title);
            }
            let seen = Rect::new(notes.x + notes.width as i32 - 116, notes.y + 10, 62, 32);
            if ctx.hovered(seen) {
                p.box_(seen, Color(0, 0, 0, 14), 4);
            }
            p.center(seen.x, seen.y + 8, 62, "Clear all", 12, INK);
            p.region(seen, "shell:notifications:seen", "Mark all as read");
        }
    }
    flyout(p, cal);
    p.left(
        cal.x + 18,
        cal.y + 16,
        cal.width - 60,
        &format!(
            "{}, {} {}",
            date.weekday_name(),
            date.month_name(),
            date.day
        ),
        14,
        INK,
    );
    // The agenda this header would unfold is the Calendar application's, so the header
    // opens it. Without Calendar installed there is nothing behind it at all.
    let agenda = Rect::new(cal.x + 12, cal.y + 10, cal.width - 24, 32);
    if ctx.installed("calendar") {
        if ctx.hovered(agenda) {
            p.box_(agenda, Color(0, 0, 0, 14), 4);
        }
        p.symbol(
            "chevron-down",
            cal.x + cal.width as i32 - 34,
            cal.y + 19,
            12,
            MUTED,
        );
        p.region(agenda, "shell:launch:calendar", "Open Calendar");
    } else {
        p.symbol(
            "chevron-down",
            cal.x + cal.width as i32 - 34,
            cal.y + 19,
            12,
            Color(140, 140, 146, 150),
        );
        p.disabled("Expand agenda");
    }
    p.hline(cal.x + 1, cal.y + 50, cal.width - 2, STROKE);
    // The month title returns to today, which is only a move when the grid has left it.
    let title = Rect::new(cal.x + 12, cal.y + 58, 176, 26);
    let paged = ctx.panel_month != 0;
    if paged && ctx.hovered(title) {
        p.box_(title, Color(0, 0, 0, 14), 4);
    }
    p.strong(
        title.x + 6,
        cal.y + 64,
        title.width - 12,
        &format!("{} {}", shown.month_name(), shown.year),
        13,
        INK,
    );
    if paged {
        p.region(
            title,
            "shell:month:today",
            &format!("Back to {} {}", date.month_name(), date.year),
        );
    }
    // Both chevrons page the grid's month for real; Windows puts previous above next.
    for (i, (symbol, action, label)) in [
        ("chevron-up", "shell:month:prev", "Previous month"),
        ("chevron-down", "shell:month:next", "Next month"),
    ]
    .into_iter()
    .enumerate()
    {
        let hit = Rect::new(
            cal.x + cal.width as i32 - 70 + i as i32 * 28,
            cal.y + 58,
            28,
            26,
        );
        if ctx.hovered(hit) {
            p.box_(hit, Color(0, 0, 0, 14), 4);
        }
        p.symbol(symbol, hit.x + 8, hit.y + 8, 12, INK);
        p.region(hit, action, label);
    }
    let col = ((cal.width - 24) / 7) as i32;
    for (i, day) in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
        .iter()
        .enumerate()
    {
        p.center(
            cal.x + 12 + i as i32 * col,
            cal.y + 98,
            col as u32,
            day,
            12,
            INK,
        );
    }
    // Each day opens Calendar on that day; days before the world began are only text.
    for day in 1..=shown.days_in_month {
        let cell = (day - 1 + shown.first_weekday) as i32;
        let cx = cal.x + 12 + (cell % 7) * col;
        let cy = cal.y + 128 + (cell / 7) * 40;
        if let Some(open) = ctx.open_day(day) {
            p.region_above(
                Rect::new(cx, cy - 8, col as u32, 36),
                &open,
                &format!("{} {day}", shown.month_name()),
            );
        }
        // Only the real today is ringed, and only while the grid is on its month.
        let today = !paged && day == date.day;
        if today {
            p.circle(cx + col / 2, cy + 9, 17, ACCENT);
        }
        p.label(
            cx,
            cy,
            col as u32,
            &day.to_string(),
            13,
            if today { Color::WHITE } else { INK },
            false,
            Align::Center,
        );
    }
    if ctx.installed("calendar") {
        let link = Rect::new(
            cal.x + 12,
            cal.y + cal.height as i32 - 40,
            cal.width - 24,
            30,
        );
        if ctx.hovered(link) {
            p.box_(link, Color(0, 0, 0, 12), 4);
        }
        p.region(link, "shell:launch:calendar", "Open calendar");
        p.left(
            link.x + 8,
            link.y + 6,
            link.width - 16,
            "Open Calendar",
            13,
            ACCENT,
        );
    }
}

fn quick_settings(p: &mut Painter, ctx: &ShellContext<'_>) {
    let r = anchored(ctx, 360, 392);
    flyout(p, r);
    let tile = (r.width - 24 - 24) / 3;
    // Every tile flips a real system switch and paints the position it reads back.
    // Windows 11 also ships an Accessibility tile; nothing in the machine backs one, so
    // the sixth slot carries Mobile hotspot, which does have a switch.
    for (i, (symbol, name, switch)) in [
        ("wifi", "Wi-Fi", "wifi"),
        ("bluetooth", "Bluetooth", "bluetooth"),
        ("airplane", "Airplane mode", "airplane_mode"),
        ("leaf", "Energy saver", "battery_saver"),
        ("night-light", "Night light", "night_light"),
        ("hotspot", "Mobile hotspot", "hotspot"),
    ]
    .iter()
    .enumerate()
    {
        let on = ctx.switch(switch);
        let tx = r.x + 24 + (i as u32 % 3 * (tile + 12)) as i32 - 12;
        let ty = r.y + 24 + (i / 3) as i32 * 92;
        let button = Rect::new(tx, ty, tile, 48);
        p.button(
            button,
            match (on, ctx.hovered(button)) {
                (true, true) => Color::rgb(0, 110, 210),
                (true, false) => ACCENT,
                (false, true) => Color(255, 255, 255, 235),
                (false, false) => Color(255, 255, 255, 190),
            },
            4,
            &format!("shell:toggle:{switch}"),
            &format!("{name}, {}", if on { "on" } else { "off" }),
        );
        p.border(
            button,
            Color::TRANSPARENT,
            4,
            if on { Color(0, 0, 0, 40) } else { STROKE },
        );
        p.symbol(
            symbol,
            tx + tile as i32 / 2 - 8,
            ty + 16,
            16,
            if on { Color::WHITE } else { INK },
        );
        p.center(tx, ty + 56, tile, name, 12, INK);
    }
    // Twenty discrete steps per slider, so a click lands on an exact, stored level.
    const STEPS: u32 = 20;
    for (i, (symbol, level, label)) in [
        ("sun", "brightness", "Brightness"),
        ("volume", "volume", "Volume"),
    ]
    .iter()
    .enumerate()
    {
        let current = u32::from(ctx.level(level));
        let sy = r.y + 222 + i as i32 * 48;
        p.symbol(
            if *level == "volume" && current == 0 {
                "volume-mute"
            } else {
                symbol
            },
            r.x + 24,
            sy,
            16,
            INK,
        );
        let track = Rect::new(r.x + 60, sy + 6, r.width - 96, 4);
        for step in 0..STEPS {
            let x0 = track.x + (track.width * step / STEPS) as i32;
            let x1 = track.x + (track.width * (step + 1) / STEPS) as i32;
            let percent = (step + 1) * 100 / STEPS;
            p.box_(
                Rect::new(x0, track.y, (x1 - x0 - 2).max(1) as u32, 4),
                if percent <= current {
                    ACCENT
                } else {
                    Color(0, 0, 0, 70)
                },
                2,
            );
            p.button(
                Rect::new(x0 - 1, sy - 6, (x1 - x0 + 2) as u32, 24),
                Color::TRANSPARENT,
                0,
                &format!("shell:set:{level}:{percent}"),
                &format!("{label} {percent}%"),
            );
        }
        let fill = (track.width * current / 100) as i32;
        p.circle(track.x + fill, track.y + 2, 10, Color::WHITE);
        p.ring(track.x + fill, track.y + 2, 10, 1, Color(0, 0, 0, 40));
        p.circle(track.x + fill, track.y + 2, 6, ACCENT);
    }
    let foot = r.y + r.height as i32 - 48;
    p.box_(
        Rect::new(r.x + 1, foot, r.width - 2, 47),
        Color(0, 0, 0, 10),
        0,
    );
    p.hline(r.x + 1, foot, r.width - 2, STROKE);
    // The machine has no battery model, so the pill reports mains power and the energy
    // saver switch instead of inventing a percentage.
    let battery = Rect::new(r.x + 12, foot + 6, 150, 36);
    if ctx.hovered(battery) {
        p.box_(battery, Color(0, 0, 0, 14), 4);
    }
    p.symbol("battery", r.x + 22, foot + 14, 20, INK);
    p.left(
        r.x + 50,
        foot + 15,
        104,
        if ctx.switch("battery_saver") {
            "Energy saver"
        } else {
            "Plugged in"
        },
        12,
        INK,
    );
    p.region(battery, "shell:settings", "Power and battery settings");
    let gear = Rect::new(r.x + r.width as i32 - 52, foot + 6, 36, 36);
    if ctx.hovered(gear) {
        p.box_(gear, Color(0, 0, 0, 14), 4);
    }
    p.symbol("gear", gear.x + 10, gear.y + 10, 16, INK);
    p.region(gear, "shell:settings", "All settings");
    // Still greyed, deliberately: editing the tiles needs a stored tile order on the
    // machine, and nothing else in the world wants one. The six tiles here are exactly
    // the six switches the machine has, so there is no arrangement to change.
    p.symbol(
        "edit",
        gear.x - 30,
        gear.y + 10,
        16,
        Color(140, 140, 146, 150),
    );
    p.disabled("Edit quick settings");
}

fn settings(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = 560.min(ctx.width.saturating_sub(24));
    let height = 360.min(ctx.height.saturating_sub(72));
    let x = (ctx.width - width) as i32 / 2;
    let y = (ctx.height as i32 - 48 - height as i32) / 2;
    let r = Rect::new(x, y, width, height);
    flyout(p, r);
    p.symbol("gear", x + 16, y + 12, 14, INK);
    p.left(x + 38, y + 11, 200, "Settings", 12, INK);
    let close = Rect::new(x + width as i32 - 46, y + 1, 45, 32);
    if ctx.hovered(close) {
        p.box_(close, Color::rgb(196, 43, 28), 7);
    }
    p.symbol(
        "close",
        close.x + 17,
        close.y + 10,
        11,
        if ctx.hovered(close) {
            Color::WHITE
        } else {
            INK
        },
    );
    p.region(close, "shell:dismiss", "Close Settings");
    // Settings has a single page here, so the breadcrumb is a heading, not navigation.
    p.strong(x + 32, y + 52, width - 64, "System  ›  About", 20, INK);
    let card = Rect::new(x + 32, y + 100, width - 64, 150);
    p.border(card, Color(255, 255, 255, 200), 6, STROKE);
    for (i, (key, value, action)) in [
        ("Device name", "alice-pc".to_owned(), None),
        ("Edition", "Windows 11 Pro".to_owned(), None),
        ("Display", format!("{} × {}", ctx.width, ctx.height), None),
        (
            // The one reading with somewhere to go: Task view shows those windows.
            "Open windows",
            ctx.windows.len().to_string(),
            Some("shell:overview"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let ry = card.y + 14 + i as i32 * 33;
        if let Some(action) = action {
            let row = Rect::new(card.x + 8, ry - 5, card.width - 16, 30);
            if ctx.hovered(row) {
                p.box_(row, Color(0, 0, 0, 12), 4);
            }
            p.region(row, action, "Show open windows");
        }
        p.left(card.x + 18, ry, 160, key, 13, MUTED);
        p.left(
            card.x + 190,
            ry,
            card.width - 208,
            &value,
            13,
            if action.is_some() { ACCENT } else { INK },
        );
    }
    let apps = Rect::new(x + 32, y + height as i32 - 56, 120, 32);
    p.button(apps, ACCENT, 4, "shell:launcher", "Open Start");
    p.center(apps.x, apps.y + 7, 120, "Open Start", 13, Color::WHITE);
}

fn context_menu(p: &mut Painter, ctx: &ShellContext<'_>) {
    let (mx, my) = ctx
        .hover
        .unwrap_or((ctx.width as i32 / 3, ctx.height as i32 / 3));
    let entries = [
        ("grid-view", "View", "shell:panel:overview"),
        ("folder", "Open File Explorer", "shell:launch:files"),
        ("terminal", "Open in Terminal", "shell:launch:terminal"),
        ("display", "Display settings", "shell:settings"),
    ];
    let r = Rect::new(
        mx.clamp(4, ctx.width as i32 - 244),
        my.clamp(4, ctx.height as i32 - 60 - 160),
        240,
        entries.len() as u32 * 36 + 8,
    );
    flyout(p, r);
    for (i, (symbol, label, action)) in entries.iter().enumerate() {
        let row = Rect::new(r.x + 4, r.y + 4 + i as i32 * 36, r.width - 8, 36);
        if ctx.hovered(row) {
            p.box_(row, Color(0, 0, 0, 14), 4);
        }
        p.symbol(symbol, row.x + 12, row.y + 10, 16, INK);
        p.left(row.x + 42, row.y + 9, row.width - 54, label, 13, INK);
        p.region(row, action, label);
    }
}

fn task_view(p: &mut Painter, ctx: &ShellContext<'_>) {
    let full = Rect::new(0, 0, ctx.width, ctx.height.saturating_sub(48));
    p.glass(full, 0, 36, Color(24, 28, 40, 120), None);
    p.region(full, "shell:dismiss", "Close Task view");
    let open: Vec<_> = ctx.windows.iter().rev().take(6).collect();
    if open.is_empty() {
        p.center(
            0,
            full.height as i32 / 2 - 40,
            ctx.width,
            "No open windows",
            16,
            Color::WHITE,
        );
    }
    let columns = open.len().clamp(1, 3) as u32;
    let card_width = ((ctx.width - 96) / columns).min(380) - 24;
    let left = (ctx.width - columns * (card_width + 24)) as i32 / 2 + 12;
    for (i, w) in open.iter().enumerate() {
        let cx = left + (i as u32 % columns * (card_width + 24)) as i32;
        let cy = 72 + (i as u32 / columns) as i32 * 250;
        let card = Rect::new(cx, cy, card_width, 214);
        if card.y + 214 > full.height as i32 - 110 {
            break;
        }
        p.drop_shadow(card, 8, 20, 110, 8);
        p.border(
            card,
            Color::rgb(249, 249, 249),
            8,
            if w.focused {
                ACCENT
            } else {
                Color(255, 255, 255, 60)
            },
        );
        p.box_(
            Rect::new(card.x + 1, card.y + 1, card.width - 2, 32),
            CAPTION,
            7,
        );
        p.asset(
            Rect::new(card.x + 10, card.y + 9, 16, 16),
            &format!("icon/windows/{}", w.kind),
        );
        p.left(
            card.x + 34,
            card.y + 9,
            card.width - 50,
            &format!("{} — {}", tab_title(w), app_name(&w.kind)),
            12,
            INK,
        );
        p.asset(
            Rect::new(card.x + card.width as i32 / 2 - 28, card.y + 92, 56, 56),
            &format!("icon/windows/{}", w.kind),
        );
        p.region(card, &w.action("focus"), &format!("Switch to {}", w.title));
    }
    desktop_strip(p, ctx, full.height as i32 - 96);
}

/// Task view's virtual desktop strip: one tile per desktop the machine really keeps,
/// the one on screen marked, and the controls that switch, add and close them.
fn desktop_strip(p: &mut Painter, ctx: &ShellContext<'_>, y: i32) {
    let count = ctx.workspaces.max(1);
    let full = count >= crate::WORKSPACE_LIMIT;
    let span = (count as i32 + i32::from(!full)) * 104 - 8;
    let mut x = ctx.width as i32 / 2 - span / 2;
    // A window view carries no workspace of its own, so only the desktop on screen can
    // show what is on it; the rest are named and reachable but not yet previewable.
    let here_windows = ctx.windows.iter().filter(|w| !w.minimized).count().min(3);
    for index in 0..count {
        let tile = Rect::new(x, y + 22, 96, 54);
        let here = index == ctx.workspace;
        let name = format!("Desktop {}", index + 1);
        p.button(
            tile,
            Color(
                255,
                255,
                255,
                match (here, ctx.hovered(tile)) {
                    (true, _) => 70,
                    (false, true) => 44,
                    (false, false) => 22,
                },
            ),
            4,
            &format!("shell:workspace:{index}"),
            &name,
        );
        p.border(
            tile,
            Color::TRANSPARENT,
            4,
            if here {
                Color::WHITE
            } else {
                Color(255, 255, 255, 60)
            },
        );
        p.left(
            x,
            y,
            96,
            &name,
            12,
            Color(255, 255, 255, if here { 255 } else { 150 }),
        );
        if here {
            for n in 0..here_windows as i32 {
                p.box_(
                    Rect::new(tile.x + 10 + n * 26, y + 36, 20, 26),
                    Color(255, 255, 255, 170),
                    2,
                );
            }
            // The handler closes the desktop you are on, so only that tile offers it.
            if count > 1 {
                let close = Rect::new(tile.x + 76, y + 14, 20, 20);
                p.button(
                    close,
                    Color::rgb(60, 60, 64),
                    10,
                    "shell:workspace:close",
                    &format!("Close {name}"),
                );
                p.symbol("close", close.x + 5, close.y + 5, 10, Color::WHITE);
            }
        }
        x += 104;
    }
    let add = Rect::new(x, y + 22, 96, 54);
    if full {
        p.border(add, Color(255, 255, 255, 18), 4, Color(255, 255, 255, 50));
        p.left(x, y, 96, "New desktop", 12, Color(255, 255, 255, 120));
        // Eight desktops is the machine's limit; a ninth would be refused.
        p.symbol("plus", x + 40, y + 41, 16, Color(255, 255, 255, 120));
        p.disabled("New desktop");
    } else {
        p.button(
            add,
            Color(255, 255, 255, if ctx.hovered(add) { 44 } else { 18 }),
            4,
            "shell:workspace:new",
            "New desktop",
        );
        p.border(add, Color::TRANSPARENT, 4, Color(255, 255, 255, 50));
        p.left(x, y, 96, "New desktop", 12, Color(255, 255, 255, 190));
        p.symbol("plus", x + 40, y + 41, 16, Color::WHITE);
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
            desktop_selection: None,
            settings: &crate::SystemSettings::DEFAULT,
            screen: crate::ScreenState::Active,
            panel_month: 0,
            text_entry: false,
            keyboard: crate::KeyboardState::default(),
            bookmarks: &[],
            downloads: &[],
            notifications: &[],
            workspaces: 1,
            workspace: 0,
            library_group: None,
            bookmarked: false,
            panel_over_launcher: false,
            typed: "",
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
            ..Default::default()
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
    fn actions(p: &Painter) -> Vec<&str> {
        p.scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.as_deref())
            .collect()
    }
    fn labelled<'a>(p: &'a Painter, label: &str) -> Option<&'a cw_scene::Node> {
        p.scene
            .nodes
            .iter()
            .find(|n| n.semantic.as_ref().is_some_and(|s| s.label == label))
    }
    #[test]
    fn quick_settings_flip_real_switches_and_set_exact_levels() {
        let settings = crate::SystemSettings {
            wifi: false,
            night_light: true,
            brightness: 40,
            volume: 0,
            ..crate::SystemSettings::DEFAULT
        };
        let mut ctx = context(&[], &[]);
        ctx.settings = &settings;
        ctx.panel = Some("quick");
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        let painted = actions(&p);
        // Every switch and level the flyout paints must be one the machine really holds.
        for action in &painted {
            if let Some(name) = action.strip_prefix("shell:toggle:") {
                assert!(crate::SystemSettings::DEFAULT.flag(name).is_ok(), "{name}");
            }
            if let Some(rest) = action.strip_prefix("shell:set:") {
                let (name, percent) = rest.split_once(':').unwrap();
                let mut state = crate::SystemSettings::DEFAULT;
                assert!(state.set_level(name, percent.parse().unwrap()).is_ok());
            }
        }
        for name in [
            "wifi",
            "bluetooth",
            "airplane_mode",
            "battery_saver",
            "night_light",
            "hotspot",
        ] {
            assert!(painted.contains(&format!("shell:toggle:{name}").as_str()));
        }
        for level in ["brightness", "volume"] {
            let steps: Vec<_> = painted
                .iter()
                .filter(|a| a.starts_with(&format!("shell:set:{level}:")))
                .collect();
            assert_eq!(steps.len(), 20, "{level}");
            assert!(steps.contains(&&format!("shell:set:{level}:5").as_str()));
            assert!(steps.contains(&&format!("shell:set:{level}:100").as_str()));
        }
        // Tiles render the position they read back, not a fixed picture.
        let fill = |action: &str| {
            let node = p
                .scene
                .nodes
                .iter()
                .find(|n| n.interaction.as_deref() == Some(action))
                .unwrap();
            match node.primitive {
                cw_scene::Primitive::RoundedBox { fill, .. } => fill,
                _ => panic!("tile is a rounded box"),
            }
        };
        assert_eq!(fill("shell:toggle:night_light"), ACCENT);
        assert_ne!(fill("shell:toggle:wifi"), ACCENT);
        assert!(labelled(&p, "Night light, on").is_some());
        assert!(labelled(&p, "Wi-Fi, off").is_some());
        // A click really lands on the step it names, not on the flyout behind it.
        let step = p
            .scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some("shell:set:brightness:50"))
            .unwrap()
            .bounds;
        assert_eq!(
            p.scene
                .hit_test(step.x + 2, step.y + 12)
                .unwrap()
                .interaction
                .as_deref(),
            Some("shell:set:brightness:50")
        );
    }
    #[test]
    fn start_power_opens_a_flyout_and_a_dark_screen_offers_a_way_back() {
        let mut ctx = context(&[], &[]);
        ctx.launcher_open = true;
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        assert_eq!(
            labelled(&p, "Power").unwrap().interaction.as_deref(),
            Some(POWER_MENU)
        );
        ctx.launcher_open = false;
        ctx.panel = Some(POWER_PANEL);
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        for action in ["shell:power:lock", "shell:power:restart", "shell:power:off"] {
            let row = p
                .scene
                .nodes
                .iter()
                .find(|n| n.interaction.as_deref() == Some(action))
                .unwrap()
                .bounds;
            assert_eq!(
                p.scene
                    .hit_test(row.x + 8, row.y + 8)
                    .unwrap()
                    .interaction
                    .as_deref(),
                Some(action)
            );
        }
        // With Start still open the flyout sits over it, not instead of it: the Start
        // menu is still painted and the power rows still answer their own clicks. That
        // is all this shell can do until `panel_over_launcher` reaches `ShellContext`.
        ctx.launcher_open = true;
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        assert!(texts(&p).contains(&"Pinned"));
        assert_eq!(hit_labelled(&p, "Shut down"), Some("shell:power:off"));
        // A locked or powered-off display covers everything with one real way back.
        ctx.launcher_open = false;
        ctx.panel = None;
        for screen in [crate::ScreenState::Locked, crate::ScreenState::Off] {
            ctx.screen = screen;
            let mut p = Painter::new(1280, 800);
            chrome(&mut p, &ctx);
            for (x, y) in [(640, 400), (12, 780), (1270, 8)] {
                assert_eq!(
                    p.scene.hit_test(x, y).unwrap().interaction.as_deref(),
                    Some("shell:power:wake"),
                    "{screen:?} at {x},{y}"
                );
            }
            assert!(actions(&p).iter().all(|a| *a == "shell:power:wake"));
        }
    }
    #[test]
    fn flyout_utilities_are_either_wired_or_announced_disabled() {
        let windows = vec![WindowView {
            id: 7,
            title: "Example — Edge".into(),
            kind: "browser".into(),
            rect: Rect::new(80, 60, 900, 600),
            focused: true,
            ..Default::default()
        }];
        let mut ctx = context(&windows, &[]);
        let mut p = Painter::new(1280, 800);
        browser_chrome(&mut p, &ctx, &windows[0]);
        assert_eq!(
            labelled(&p, "Settings and more")
                .unwrap()
                .interaction
                .as_deref(),
            Some("shell:settings")
        );
        // The avatar reaches the same account settings Start's own user row does.
        assert_eq!(hit_labelled(&p, "Profile, alice"), Some("shell:settings"));
        // A fresh tab has no page, so the star is greyed: never clickable, never
        // focusable, and a click on it does not answer with the bookmark id.
        let node = labelled(&p, "Add to favourites").unwrap();
        assert!(node.semantic.as_ref().unwrap().disabled);
        assert!(node.interaction.is_none());
        assert!(!node.semantic.as_ref().unwrap().focusable);
        assert_ne!(
            hit_labelled(&p, "Add to favourites"),
            Some("shell:bookmark")
        );
        // The search flyout's query field focuses search, not the click absorber.
        ctx.panel = Some("search");
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        assert_eq!(
            p.scene.hit_test(640, 260).unwrap().interaction.as_deref(),
            Some("shell:search")
        );
        // The notification header carries the real do-not-disturb switch.
        ctx.panel = Some("notifications");
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        assert!(actions(&p).contains(&"shell:toggle:do_not_disturb"));
        // The date header unfolds the agenda the Calendar application really keeps.
        assert_eq!(
            hit_labelled(&p, "Open Calendar"),
            Some("shell:launch:calendar")
        );
        // Start's account tile and "All apps" both go somewhere real.
        ctx.panel = None;
        ctx.launcher_open = true;
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        assert_eq!(
            labelled(&p, "All apps").unwrap().interaction.as_deref(),
            Some("shell:search")
        );
        assert_eq!(
            labelled(&p, "Account settings for alice")
                .unwrap()
                .interaction
                .as_deref(),
            Some("shell:settings")
        );
        assert!(labelled(&p, "Hidden icons").unwrap().interaction.is_none());
        // The settings card's only navigable reading opens Task view.
        ctx.launcher_open = false;
        ctx.panel = Some("settings");
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        assert_eq!(
            labelled(&p, "Show open windows")
                .unwrap()
                .interaction
                .as_deref(),
            Some("shell:overview")
        );
    }
    /// Every string the painter drew, bold or not.
    fn texts(p: &Painter) -> Vec<&str> {
        p.scene
            .nodes
            .iter()
            .filter_map(|n| match &n.primitive {
                cw_scene::Primitive::UiText { text, .. }
                | cw_scene::Primitive::UiTextBold { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
    /// Hit-test the middle of a labelled control, so occlusion counts too.
    fn hit_labelled<'a>(p: &'a Painter, label: &str) -> Option<&'a str> {
        let node = labelled(p, label)?;
        let (x, y) = (
            node.bounds.x + node.bounds.width as i32 / 2,
            node.bounds.y + node.bounds.height as i32 / 2,
        );
        p.scene
            .hit_test(x, y)
            .and_then(|n| n.interaction.as_deref())
    }
    #[test]
    fn start_carries_the_whole_roster_and_the_taskbar_only_its_pins() {
        let mut ctx = context(&[], &[]);
        ctx.launcher_open = true;
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        let launches = actions(&p);
        for (kind, _) in APPS {
            assert!(
                launches.contains(&format!("shell:launch:{kind}").as_str()),
                "Start hides {kind}"
            );
        }
        // The taskbar pins a handful; everything else lives behind Start and search.
        let ctx = context(&[], &[]);
        let mut bar = Painter::new(1280, 800);
        chrome(&mut bar, &ctx);
        let pinned = actions(&bar);
        for kind in ["notes", "contacts", "calculator", "clock"] {
            assert!(!pinned.contains(&format!("shell:launch:{kind}").as_str()));
        }
        assert!(pinned.contains(&"shell:launcher") && pinned.contains(&"shell:search"));
        // An application the machine has not got is offered nowhere.
        let apps: Vec<String> = vec!["browser".into(), "calculator".into()];
        let mut ctx = context(&[], &apps);
        ctx.launcher_open = true;
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        for action in actions(&p) {
            if let Some(kind) = action.strip_prefix("shell:launch:") {
                assert!(apps.iter().any(|a| a == kind), "{kind} is not installed");
            }
        }
    }
    #[test]
    fn the_calendar_flyout_pages_the_month_the_panel_reports() {
        let mut ctx = context(&[], &[]);
        ctx.panel = Some("calendar");
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        // On the world's own month: chevrons page, and there is nothing to return from.
        assert_eq!(hit_labelled(&p, "Previous month"), Some("shell:month:prev"));
        assert_eq!(hit_labelled(&p, "Next month"), Some("shell:month:next"));
        assert!(texts(&p).contains(&"September 2026"));
        assert!(!actions(&p).contains(&"shell:month:today"));
        // Paged away, the grid draws the month `panel_date` reports, with a way back.
        ctx.panel_month = 5;
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        let shown = ctx.panel_date();
        assert_eq!((shown.year, shown.month), (2027, 2));
        assert!(texts(&p).contains(&"February 2027"));
        assert!(!texts(&p).contains(&"September 2026"));
        assert_eq!(
            hit_labelled(&p, "Back to September 2026"),
            Some("shell:month:today")
        );
        // Twenty-eight day cells, and no day of another month wearing today's ring.
        let days: Vec<&str> = texts(&p)
            .into_iter()
            .filter(|t| t.parse::<u64>().is_ok_and(|d| (1..=31).contains(&d)))
            .collect();
        assert_eq!(days.len(), 28);
        assert!(days.contains(&"28") && !days.contains(&"29"));
        // Day cells are painted, never targeted.
        assert!(!actions(&p).iter().any(|a| a.starts_with("shell:day")));
    }
    #[test]
    fn the_edge_toolbar_greys_history_it_does_not_have() {
        let fresh = vec![WindowView {
            id: 9,
            title: "New tab".into(),
            kind: "browser".into(),
            rect: Rect::new(40, 40, 900, 600),
            focused: true,
            ..Default::default()
        }];
        let ctx = context(&fresh, &[]);
        let mut p = Painter::new(1280, 800);
        browser_chrome(&mut p, &ctx, &fresh[0]);
        for label in ["Back", "Forward", "Refresh"] {
            let node = labelled(&p, label).unwrap();
            let semantic = node.semantic.as_ref().unwrap();
            assert!(semantic.disabled && !semantic.focusable, "{label}");
            assert!(node.interaction.is_none(), "{label}");
            assert_eq!(hit_labelled(&p, label), None, "{label}");
        }
        // Real history and a loaded page put all three back on the toolbar.
        let loaded = vec![WindowView {
            document: "http://example.test/".into(),
            can_go_back: true,
            can_go_forward: true,
            ..fresh[0].clone()
        }];
        let ctx = context(&loaded, &[]);
        let mut p = Painter::new(1280, 800);
        browser_chrome(&mut p, &ctx, &loaded[0]);
        for (label, action) in [
            ("Back", "back"),
            ("Forward", "forward"),
            ("Refresh", "reload"),
        ] {
            assert_eq!(
                hit_labelled(&p, label),
                Some(format!("window:9:content:shell:{action}").as_str())
            );
        }
    }
    #[test]
    fn the_shell_paints_no_control_the_simulator_cannot_route() {
        // A loaded browser so the toolbar paints its live star, and more than one
        // desktop so Task view paints every workspace control it has.
        let windows = vec![WindowView {
            id: 3,
            title: "Example — http://example.test/".into(),
            kind: "browser".into(),
            rect: Rect::new(60, 40, 900, 420),
            focused: true,
            document: "http://example.test/".into(),
            can_go_back: true,
            can_go_forward: true,
            ..Default::default()
        }];
        for panel in [
            None,
            Some("quick"),
            Some("search"),
            Some("calendar"),
            Some("notifications"),
            Some("settings"),
            Some("overview"),
            Some("context"),
            Some(POWER_PANEL),
        ] {
            let posted = [notice("files", "Screenshot taken", false)];
            let mut ctx = context(&windows, &[]);
            ctx.panel = panel;
            // Start stays open under the power flyout, which is where it belongs.
            ctx.launcher_open = panel.is_none() || panel == Some(POWER_PANEL);
            ctx.workspaces = 3;
            ctx.workspace = 1;
            ctx.notifications = &posted;
            let mut p = Painter::new(1280, 800);
            chrome(&mut p, &ctx);
            window_frame(&mut p, &ctx, &windows[0]);
            browser_chrome(&mut p, &ctx, &windows[0]);
            for action in actions(&p) {
                assert!(
                    matches!(
                        action,
                        "shell:noop"
                            | "shell:dismiss"
                            | "shell:desktop"
                            | "shell:launcher"
                            | "shell:search"
                            | "shell:settings"
                            | "shell:overview"
                            | "shell:panel:quick"
                            | "shell:panel:calendar"
                            | "shell:panel:overview"
                            | "shell:bookmark"
                            | "shell:notifications:seen"
                            | POWER_MENU
                    ) || action.starts_with("shell:month:")
                        || action.starts_with("shell:workspace:")
                        || action.starts_with("shell:notice:")
                        || action.starts_with("shell:launch:")
                        || action.starts_with("shell:open:")
                        || action.starts_with("shell:toggle:")
                        || action.starts_with("shell:set:")
                        || action.starts_with("shell:power:")
                        || action.starts_with("window:"),
                    "{panel:?} paints unroutable control {action}"
                );
            }
        }
    }
    #[test]
    fn flyouts_anchor_to_the_tray_and_absorb_clicks() {
        let mut ctx = context(&[], &[]);
        ctx.panel = Some("quick");
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        assert_eq!(
            p.scene.hit_test(1100, 600).unwrap().interaction.as_deref(),
            Some("shell:noop")
        );
        assert_eq!(
            p.scene.hit_test(300, 300).unwrap().interaction.as_deref(),
            Some("shell:dismiss")
        );
        assert_eq!(
            p.scene.hit_test(1234, 717).unwrap().interaction.as_deref(),
            Some("shell:settings")
        );
    }

    fn notice(app: &str, title: &str, seen: bool) -> crate::Notice {
        crate::Notice {
            app: app.into(),
            title: title.into(),
            body: "Saved to ~/Pictures".into(),
            time_us: 0,
            action: Some("shell:launch:files".into()),
            seen,
        }
    }

    #[test]
    fn the_notification_flyout_lists_what_the_machine_really_posted() {
        let posted = [notice("files", "Screenshot taken", false)];
        let mut ctx = context(&[], &[]);
        ctx.panel = Some("notifications");
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        assert!(texts(&p).contains(&"No new notifications"));
        assert!(!actions(&p).iter().any(|a| a.starts_with("shell:notice:")));
        ctx.notifications = &posted;
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        assert!(!texts(&p).contains(&"No new notifications"));
        assert_eq!(hit_labelled(&p, "Screenshot taken"), Some("shell:notice:0"));
        assert_eq!(
            hit_labelled(&p, "Mark all as read"),
            Some("shell:notifications:seen")
        );
    }

    #[test]
    fn task_view_shows_the_real_desktops_and_switches_adds_and_closes_them() {
        let windows = vec![WindowView {
            id: 3,
            title: "Notepad".into(),
            kind: "editor".into(),
            rect: Rect::new(60, 40, 600, 420),
            focused: true,
            ..Default::default()
        }];
        // One desktop: the tile you are on is still a real switch, closing the last
        // desktop is refused so no tile offers it, and a second one can be added.
        let mut ctx = context(&windows, &[]);
        ctx.panel = Some("overview");
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        assert_eq!(hit_labelled(&p, "Desktop 1"), Some("shell:workspace:0"));
        assert_eq!(hit_labelled(&p, "New desktop"), Some("shell:workspace:new"));
        assert!(labelled(&p, "Close Desktop 1").is_none());
        // Three desktops, sitting on the second: every tile is named and reachable, and
        // only the desktop on screen offers the close the handler would actually accept.
        ctx.workspaces = 3;
        ctx.workspace = 1;
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        for index in 0..3 {
            assert_eq!(
                hit_labelled(&p, &format!("Desktop {}", index + 1)),
                Some(format!("shell:workspace:{index}").as_str()),
                "desktop {index}"
            );
        }
        assert!(labelled(&p, "Desktop 4").is_none());
        assert_eq!(
            hit_labelled(&p, "Close Desktop 2"),
            Some("shell:workspace:close")
        );
        for other in ["Close Desktop 1", "Close Desktop 3"] {
            assert!(labelled(&p, other).is_none(), "{other}");
        }
        // At the machine's limit there is no ninth desktop, and the tile says so
        // instead of dispatching an action the handler would refuse.
        ctx.workspaces = crate::WORKSPACE_LIMIT;
        ctx.workspace = 0;
        let mut p = Painter::new(1280, 800);
        chrome(&mut p, &ctx);
        let node = labelled(&p, "New desktop").unwrap();
        assert!(node.semantic.as_ref().unwrap().disabled);
        assert!(node.interaction.is_none() && !node.semantic.as_ref().unwrap().focusable);
        assert_ne!(hit_labelled(&p, "New desktop"), Some("shell:workspace:new"));
    }

    #[test]
    fn the_edge_star_saves_the_page_and_reads_its_own_position_back() {
        let loaded = vec![WindowView {
            id: 7,
            title: "Example — http://example.test/".into(),
            kind: "browser".into(),
            rect: Rect::new(80, 60, 900, 600),
            focused: true,
            document: "http://example.test/".into(),
            ..Default::default()
        }];
        let mut ctx = context(&loaded, &[]);
        let mut p = Painter::new(1280, 800);
        browser_chrome(&mut p, &ctx, &loaded[0]);
        assert_eq!(
            hit_labelled(&p, "Add to favourites"),
            Some("shell:bookmark")
        );
        // Already saved, the same control removes it and says which it is.
        ctx.bookmarked = true;
        let mut p = Painter::new(1280, 800);
        browser_chrome(&mut p, &ctx, &loaded[0]);
        assert!(labelled(&p, "Add to favourites").is_none());
        assert_eq!(
            hit_labelled(&p, "Remove from favourites"),
            Some("shell:bookmark")
        );
    }
}
