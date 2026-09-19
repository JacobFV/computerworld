//! Ubuntu 24.04 / GNOME 46 presentation: 32 px top bar, 68 px Ubuntu Dock, 46 px
//! libadwaita header bars with circular window buttons, and Yaru's orange accent.
//! All controls that accept input dispatch Rust simulator actions.
use super::shared::{Align, Painter, ShellContext, WindowView};
use cw_scene::{Color, Rect};

const PANEL: Color = Color::rgb(19, 19, 19);
const INK: Color = Color::rgb(61, 61, 61);
const DIM: Color = Color::rgb(146, 146, 146);
const ORANGE: Color = Color::rgb(233, 84, 32);
const HEADER: Color = Color::rgb(235, 235, 235);
const HEADER_BACKDROP: Color = Color::rgb(242, 242, 242);
const DARK_HEADER: Color = Color::rgb(48, 48, 48);
const POPOVER: Color = Color::rgb(53, 53, 53);
const POPOVER_EDGE: Color = Color::rgb(80, 80, 80);
const TILE: Color = Color::rgb(80, 80, 80);
const LIGHT: Color = Color::rgb(246, 246, 246);
/// Width of the Files sidebar; shared with the Files client area.
pub const FILES_SIDEBAR: u32 = 180;
/// Every application this shell can present, in Activities grid order.
const APPS: [(&str, &str); 25] = [
    ("browser", "Firefox"),
    ("files", "Files"),
    ("terminal", "Terminal"),
    ("editor", "Text Editor"),
    ("code", "Visual Studio Code"),
    ("freecad", "FreeCAD"),
    ("database", "DB Browser for SQLite"),
    ("kicad", "KiCad"),
    ("mail", "Thunderbird Mail"),
    ("calendar", "Calendar"),
    ("chat", "Chat"),
    ("docs", "LibreOffice Writer"),
    ("spreadsheet", "LibreOffice Calc"),
    ("notes", "Notes"),
    ("contacts", "Contacts"),
    ("photos", "Image Viewer"),
    ("gimp", "GNU Image Manipulation Program"),
    ("pinta", "Pinta"),
    ("kdenlive", "Kdenlive"),
    ("music", "Rhythmbox"),
    ("maps", "Maps"),
    ("weather", "Weather"),
    ("calculator", "Calculator"),
    ("clock", "Clocks"),
    ("settings", "Settings"),
];
/// The favourites the Ubuntu dock keeps, in 24.04's default order (Firefox,
/// Thunderbird, Files, Rhythmbox, LibreOffice Writer), then Terminal and Text Editor;
/// Activities carries the whole grid.
const DOCK: [&str; 8] = [
    "browser", "mail", "files", "music", "docs", "terminal", "editor", "code",
];

pub(super) fn app_name(kind: &str) -> Option<&'static str> {
    APPS.iter().find(|(k, _)| *k == kind).map(|(_, n)| *n)
}

fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("Computer")
}

pub fn background(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.asset(Rect::new(0, 0, ctx.width, ctx.height), "wallpaper/ubuntu");
    if ctx.width > 300 && ctx.height > 250 && ctx.installed("files") {
        // Desktop Icons NG lays icons out from the bottom-right corner, Home first.
        let x = ctx.width as i32 - 104;
        let top = ctx.height as i32 - 112;
        let slot = Rect::new(x, top, 92, 92);
        if ctx.selected("files") {
            p.box_(slot, Color(255, 255, 255, 64), 8);
        } else if ctx.hovered(slot) {
            p.box_(slot, Color(255, 255, 255, 40), 8);
        }
        p.platform_icon(
            Rect::new(x + 20, top + 6, 52, 52),
            "ubuntu",
            "files",
            "shell:open:files",
            "Open home folder",
        );
        // Yaru's home folder carries a house on it.
        p.symbol("home", x + 38, top + 28, 16, Color(255, 255, 255, 200));
        for (dy, alpha) in [(1, 170), (2, 70)] {
            p.label(
                x,
                top + 64 + dy,
                92,
                "Home",
                13,
                Color(0, 0, 0, alpha),
                false,
                Align::Center,
            );
        }
        p.label(
            x,
            top + 64,
            92,
            "Home",
            13,
            Color::WHITE,
            false,
            Align::Center,
        );
    }
}

pub fn chrome(p: &mut Painter, ctx: &ShellContext<'_>) {
    // A locked or powered-off display covers the session entirely, chrome included.
    if !ctx.awake() {
        shield(p, ctx);
        return;
    }
    let overview = ctx.launcher_open || matches!(ctx.panel, Some("search" | "overview"));
    if overview {
        activities(p, ctx);
    }
    top_bar(p, ctx);
    dock(p, ctx);
    if let Some(panel) = ctx.panel {
        if !matches!(panel, "search" | "overview") {
            panel_surface(p, ctx, panel);
        }
    }
}

/// GNOME's shield and the powered-off display. The only way back is a real wake.
fn shield(p: &mut Painter, ctx: &ShellContext<'_>) {
    let full = Rect::new(0, 0, ctx.width, ctx.height);
    let (width, height) = (ctx.width as i32, ctx.height as i32);
    if ctx.screen == crate::ScreenState::Off {
        p.box_(full, Color::BLACK, 0);
        p.region(full, "shell:power:wake", "Turn the screen on");
        p.center(
            0,
            height / 2 - 8,
            ctx.width,
            "Screen off",
            13,
            Color::rgb(58, 58, 58),
        );
        return;
    }
    p.asset(full, "wallpaper/ubuntu");
    p.box_(full, Color(0, 0, 0, 178), 0);
    // Clicking anywhere raises the shield; GDM has no password here, there is no auth model.
    p.region(full, "shell:power:wake", "Unlock the session");
    let date = ctx.date();
    p.label(
        0,
        height / 4,
        ctx.width,
        &ctx.time(),
        64,
        Color::WHITE,
        true,
        Align::Center,
    );
    p.center(
        0,
        height / 4 + 96,
        ctx.width,
        &format!("{} {} {}", date.weekday_name(), date.day, date.month_name()),
        17,
        Color(255, 255, 255, 200),
    );
    p.symbol(
        "lock",
        width / 2 - 10,
        height / 2 + 40,
        20,
        Color(255, 255, 255, 190),
    );
    let button = Rect::new(width / 2 - 90, height / 2 + 80, 180, 40);
    p.button(
        button,
        Color(255, 255, 255, if ctx.hovered(button) { 60 } else { 34 }),
        20,
        "shell:power:wake",
        "Unlock the session",
    );
    p.strong_center(button.x, button.y + 11, 180, "Unlock", 14, Color::WHITE);
}

fn top_bar(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = ctx.width as i32;
    p.box_(Rect::new(0, 0, ctx.width, 32), PANEL, 0);
    let plate = Color(255, 255, 255, 38);
    // GNOME 46's Activities control is a workspace indicator, not a text menu.
    let activities = Rect::new(6, 3, 63, 26);
    p.button(
        activities,
        if ctx.hovered(activities) || ctx.launcher_open {
            plate
        } else {
            PANEL
        },
        13,
        "shell:launcher",
        "Activities / Show applications",
    );
    p.box_(Rect::new(16, 12, 25, 8), Color::rgb(247, 247, 247), 4);
    p.box_(Rect::new(47, 12, 8, 8), Color::rgb(154, 154, 154), 4);
    let date = ctx.date();
    let clock = format!("{} {}  {}", &date.month_name()[..3], date.day, ctx.time());
    let clock_width = p.measure(&clock, 13, true);
    let hit = Rect::new(
        width / 2 - clock_width as i32 / 2 - 12,
        3,
        clock_width + 24,
        26,
    );
    if ctx.hovered(hit) || ctx.panel == Some("calendar") {
        p.box_(hit, plate, 13);
    }
    p.strong_center(hit.x, 8, hit.width, &clock, 13, Color::WHITE);
    p.region(
        Rect::new(hit.x, 0, hit.width, 32),
        "shell:panel:calendar",
        "Open calendar and notifications",
    );
    if width > 380 {
        let tray = Rect::new(width - 94, 3, 88, 26);
        if ctx.hovered(tray) || ctx.panel == Some("quick") {
            p.box_(tray, plate, 13);
        }
        // Network, volume, and the battery on a laptop; with no battery to report,
        // the power glyph GNOME shows on a desktop computer.
        let last = if ctx.battery { "battery" } else { "power" };
        for (i, symbol) in ["wifi", "volume", last].iter().enumerate() {
            p.symbol(
                symbol,
                tray.x + 11 + i as i32 * 24,
                8,
                16,
                Color::rgb(242, 242, 242),
            );
        }
        p.region(
            Rect::new(width - 98, 0, 98, 32),
            "shell:panel:quick",
            "Open system menu",
        );
    }
}

fn dock(p: &mut Painter, ctx: &ShellContext<'_>) {
    let height = ctx.height as i32;
    // Ubuntu's fixed full-height dock: translucent over the blurred wallpaper.
    p.glass(
        Rect::new(0, 32, 68, ctx.height.saturating_sub(32)),
        0,
        30,
        Color(28, 24, 30, 205),
        None,
    );
    p.vline(
        67,
        32,
        ctx.height.saturating_sub(32),
        Color(255, 255, 255, 18),
    );
    for (i, (kind, name)) in DOCK
        .iter()
        .filter_map(|kind| APPS.iter().find(|(k, _)| k == kind))
        .filter(|(kind, _)| ctx.installed(kind))
        .enumerate()
    {
        let y = 43 + i as i32 * 59;
        if y + 55 > height - 125 {
            break;
        }
        let running: Vec<_> = ctx.windows.iter().filter(|w| w.kind == *kind).collect();
        let slot = Rect::new(6, y - 2, 56, 56);
        let hovered = ctx.hovered(slot);
        let focused = running.iter().any(|w| w.focused && !w.minimized);
        if hovered || focused {
            p.box_(
                slot,
                Color(255, 255, 255, if hovered { 46 } else { 30 }),
                12,
            );
        }
        let action = running
            .last()
            .map_or_else(|| format!("shell:launch:{kind}"), |w| w.action("focus"));
        p.platform_icon(Rect::new(10, y + 2, 48, 48), "ubuntu", kind, &action, name);
        p.region(slot, &action, name);
        if hovered && !ctx.launcher_open {
            let width = p.measure(name, 13, false) + 24;
            let tip = Rect::new(78, y + 11, width, 30);
            p.drop_shadow(tip, 15, 10, 80, 3);
            p.border(tip, Color::rgb(36, 36, 36), 15, Color(255, 255, 255, 30));
            p.center(tip.x, tip.y + 7, width, name, 13, Color::WHITE);
        }
        let dots = running.len().min(4) as i32;
        for n in 0..dots {
            p.circle(3, y + 26 - (dots - 1) * 4 + n * 8, 2, ORANGE);
        }
    }
    if height > 320 {
        p.hline(16, height - 122, 36, Color(255, 255, 255, 40));
        let slot = Rect::new(6, height - 118, 56, 56);
        if ctx.hovered(slot) {
            p.box_(slot, Color(255, 255, 255, 46), 12);
        }
        p.asset(Rect::new(10, height - 114, 48, 48), "icon/ubuntu/trash");
        // Deleted files really land in ~/.local/share/Trash/files; this opens that folder.
        p.region(slot, "shell:trash", "Trash");
    }
    if height > 190 {
        let y = height - 62;
        let slot = Rect::new(6, y, 56, 54);
        p.button(
            slot,
            if ctx.launcher_open || ctx.hovered(slot) {
                Color(255, 255, 255, 40)
            } else {
                Color::TRANSPARENT
            },
            12,
            "shell:launcher",
            "Show applications",
        );
        // Ubuntu 24.04's Show Applications button is the Ubuntu logo.
        p.symbol("ubuntu", 20, y + 13, 28, Color::rgb(241, 241, 241));
    }
}

/// Activities overview: search, the open windows of the workspace, and the app grid.
fn activities(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = ctx.width as i32;
    let height = ctx.height as i32;
    let area = Rect::new(
        68,
        32,
        ctx.width.saturating_sub(68),
        ctx.height.saturating_sub(32),
    );
    p.glass(area, 0, 40, Color(30, 26, 34, 215), None);
    p.region(area, "shell:dismiss", "Close Activities");
    let centre = (width + 68) / 2;
    let search_width = (width - 110).clamp(140, 360) as u32;
    let field = Rect::new(centre - search_width as i32 / 2, 52, search_width, 38);
    p.border(
        field,
        Color(255, 255, 255, 34),
        19,
        Color(255, 255, 255, 40),
    );
    p.region(field, "shell:search", "Search applications");
    p.symbol(
        "search",
        field.x + 14,
        field.y + 11,
        16,
        Color::rgb(200, 198, 204),
    );
    if ctx.search.is_empty() {
        p.left(
            field.x + 40,
            field.y + 10,
            search_width - 52,
            "Type to search",
            14,
            Color::rgb(200, 198, 204),
        );
    } else {
        let end = p.left(
            field.x + 40,
            field.y + 10,
            search_width - 52,
            ctx.search,
            14,
            Color::WHITE,
        );
        p.box_(
            Rect::new(field.x + 41 + end as i32, field.y + 10, 1, 18),
            Color::WHITE,
            0,
        );
    }
    let mut top = 120;
    let open: Vec<_> = ctx.windows.iter().rev().take(4).collect();
    if ctx.search.is_empty() && !open.is_empty() && height > 520 {
        // Workspace thumbnails of the running windows.
        let card_width = ((width - 68 - 80) / open.len() as i32).min(230);
        let left = centre - card_width * open.len() as i32 / 2;
        for (i, w) in open.iter().enumerate() {
            let card = Rect::new(
                left + i as i32 * card_width + 8,
                top,
                card_width as u32 - 16,
                124,
            );
            p.drop_shadow(card, 10, 16, 120, 6);
            p.border(
                card,
                Color::rgb(250, 250, 250),
                10,
                if w.focused {
                    ORANGE
                } else {
                    Color(255, 255, 255, 40)
                },
            );
            p.box_(
                Rect::new(card.x + 1, card.y + 1, card.width - 2, 24),
                HEADER,
                9,
            );
            p.strong_center(
                card.x + 8,
                card.y + 5,
                card.width - 16,
                &window_title(w),
                11,
                INK,
            );
            p.asset(
                Rect::new(card.x + card.width as i32 / 2 - 24, card.y + 56, 48, 48),
                &format!("icon/ubuntu/{}", w.kind),
            );
            p.region(card, &w.action("focus"), &format!("Switch to {}", w.title));
        }
        top += 160;
    }
    // Six columns from 1000 px up, so the whole grid of 21 fits a 768 px screen.
    let columns = ((width - 68 - 100) / 136).clamp(1, 6);
    let cell = ((width - 68 - 100) / columns).min(160);
    let start = centre - columns * cell / 2;
    let query = ctx.search.to_lowercase();
    let shown: Vec<&(&str, &str)> = APPS
        .iter()
        .filter(|(kind, name)| {
            ctx.installed(kind)
                && (query.is_empty()
                    || name.to_lowercase().contains(&query)
                    || kind.contains(&query))
        })
        .collect();
    // GNOME pages the app grid: as many rows as fit above the workspace switcher, and
    // dots under the grid for the pages beyond. The page shown is clamped to the pages
    // there are, so a search that shrinks the grid never lands on an empty page.
    let rows = ((height - 96 - 110 - (top + 20)) / 140 + 1).max(1);
    let per_page = (columns * rows) as usize;
    let pages = shown.len().div_ceil(per_page).max(1);
    let page = (ctx.home_page as usize).min(pages - 1);
    for (i, (kind, name)) in shown
        .iter()
        .skip(page * per_page)
        .take(per_page)
        .enumerate()
    {
        let x = start + (i as i32 % columns) * cell;
        let y = top + 20 + (i as i32 / columns) * 140;
        let tile = Rect::new(x + 6, y - 10, cell as u32 - 12, 124);
        if ctx.hovered(tile) {
            p.box_(tile, Color(255, 255, 255, 28), 16);
        }
        let action = format!("shell:launch:{kind}");
        p.region(tile, &action, name);
        p.platform_icon(
            Rect::new(x + (cell - 72) / 2, y, 72, 72),
            "ubuntu",
            kind,
            &action,
            name,
        );
        p.label(
            x + 8,
            y + 82,
            cell as u32 - 16,
            name,
            13,
            Color::WHITE,
            false,
            Align::Center,
        );
    }
    if pages > 1 {
        let dots_y = top + 20 + rows * 140 - 16;
        let left = centre - pages as i32 * 10;
        for i in 0..pages {
            let dot = Rect::new(left + i as i32 * 20, dots_y, 20, 20);
            p.button(
                dot,
                Color::TRANSPARENT,
                10,
                &format!("shell:home-page:{i}"),
                &format!("Page {} of {pages}", i + 1),
            );
            p.circle(
                dot.x + 10,
                dot.y + 10,
                if i == page { 5 } else { 4 },
                if i == page {
                    Color::WHITE
                } else {
                    Color(255, 255, 255, 110)
                },
            );
        }
    }
    // The foot of the overview carries GNOME's workspace switcher, and those desktops
    // are real.
    if height > 400 {
        workspace_strip(p, ctx, centre, height - 60);
    }
}

/// Workspace switcher: one tile per desktop the machine really has, the one on screen
/// marked, and the controls that add, close and switch between them.
fn workspace_strip(p: &mut Painter, ctx: &ShellContext<'_>, centre: i32, y: i32) {
    let count = ctx.workspaces.max(1);
    let full = count >= crate::WORKSPACE_LIMIT;
    let span = (count as i32 + i32::from(!full)) * 74 - 10;
    let mut x = centre - span / 2;
    // A window view carries no workspace, so only the desktop on screen can show what
    // is on it; the others are named and reachable but cannot be previewed yet.
    let here_windows = ctx.windows.iter().filter(|w| !w.minimized).count().min(3);
    for index in 0..count {
        let r = Rect::new(x, y, 64, 40);
        let here = index == ctx.workspace;
        p.button(
            r,
            Color(255, 255, 255, if here { 74 } else { 24 }),
            6,
            &format!("shell:workspace:{index}"),
            &format!("Desktop {}", index + 1),
        );
        p.border(
            r,
            Color::TRANSPARENT,
            6,
            if here {
                ORANGE
            } else {
                Color(255, 255, 255, 60)
            },
        );
        if here {
            for n in 0..here_windows as i32 {
                p.box_(
                    Rect::new(r.x + 8 + n * 17, y + 12, 13, 16),
                    Color(255, 255, 255, 170),
                    2,
                );
            }
            // The handler closes the desktop you are on, so only that tile offers it.
            if count > 1 {
                let close = Rect::new(r.x + 48, y - 8, 20, 20);
                p.button(
                    close,
                    Color::rgb(46, 46, 46),
                    10,
                    "shell:workspace:close",
                    &format!("Close desktop {}", index + 1),
                );
                p.symbol("close", close.x + 5, close.y + 5, 10, Color::WHITE);
            }
        }
        x += 74;
    }
    let add = Rect::new(x, y, 64, 40);
    if full {
        p.border(add, Color(255, 255, 255, 12), 6, Color(255, 255, 255, 40));
        // Eight desktops is the machine's limit; a ninth would be refused.
        p.symbol("plus", add.x + 24, y + 12, 16, Color(255, 255, 255, 90));
        p.disabled("New desktop");
    } else {
        p.button(
            add,
            Color(255, 255, 255, if ctx.hovered(add) { 46 } else { 18 }),
            6,
            "shell:workspace:new",
            "New desktop",
        );
        p.symbol("plus", add.x + 24, y + 12, 16, Color::WHITE);
    }
}

pub(super) fn window_title(w: &WindowView) -> String {
    match w.kind.as_str() {
        "files" if !w.caption.is_empty() => w.caption.clone(),
        "files" if !w.home.is_empty() && w.tilde(&w.document) == "~" => "Home".into(),
        "files" => basename(&w.document).to_owned(),
        // GNOME Terminal titles itself from the prompt: `user@host: ~/dir`.
        "terminal" => w
            .shell_identity()
            .map(|(user, host, dir)| format!("{user}@{host}: {dir}"))
            .unwrap_or_else(|| "Terminal".into()),
        "editor" if w.document.is_empty() => "Untitled Document".into(),
        "editor" => basename(&w.document).to_owned(),
        "browser" if w.caption.is_empty() => "New Tab".into(),
        "browser" => w.caption.clone(),
        // Anything else names itself with the name its launcher icon carries.
        kind => app_name(kind).map_or_else(|| w.title.clone(), str::to_owned),
    }
}
/// Flat libadwaita header bar button with its hover plate.
fn header_button(p: &mut Painter, ctx: &ShellContext<'_>, r: Rect, symbol: &str, ink: Color) {
    if ctx.hovered(r) {
        p.box_(r, Color(128, 128, 128, 50), 8);
    }
    p.symbol(
        symbol,
        r.x + (r.width as i32 - 16) / 2,
        r.y + (r.height as i32 - 16) / 2,
        16,
        ink,
    );
}

/// GNOME's path bar: one button per component, each navigating to that exact folder.
/// Inside the home folder the trail starts at Home, as Files starts it; a place that is
/// not a folder (Recent, Starred, Trash) is a single named button.
fn path_bar(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView, bar: Rect, ink: Color) {
    p.box_(bar, Color(0, 0, 0, 18), 8);
    let home = w.home.trim_end_matches('/');
    let under_home =
        !home.is_empty() && (w.document == home || w.document.starts_with(&format!("{home}/")));
    let (mut crumbs, rest, mut path) = if !w.caption.is_empty() {
        (vec![(w.caption.clone(), String::new())], "", String::new())
    } else if under_home {
        (
            vec![("Home".to_owned(), home.to_owned())],
            &w.document[home.len()..],
            home.to_owned(),
        )
    } else {
        (
            vec![("Computer".to_owned(), "/".to_owned())],
            w.document.as_str(),
            String::new(),
        )
    };
    for part in rest.split('/').filter(|s| !s.is_empty()) {
        path.push('/');
        path.push_str(part);
        crumbs.push((part.to_owned(), path.clone()));
    }
    let lead = if !w.caption.is_empty() {
        None
    } else if under_home {
        Some("home")
    } else {
        Some("drive")
    };
    let last = crumbs.len() - 1;
    let widths: Vec<u32> = crumbs
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            p.measure(name, 13, true)
                + if i == last { 24 } else { 36 }
                + if i == 0 && lead.is_some() { 22 } else { 0 }
        })
        .collect();
    // Elide from the front when the bar is short: the deepest components are the useful ones.
    let mut first = 0;
    while first < last && widths[first..].iter().sum::<u32>() > bar.width.saturating_sub(8) {
        first += 1;
    }
    let mut cx = bar.x + 4;
    for (i, (name, target)) in crumbs.iter().enumerate().skip(first) {
        let room = bar.width.saturating_sub(8).max(24);
        let width = widths[i].min(room);
        let chip = Rect::new(cx, bar.y + 3, width - if i == last { 0 } else { 12 }, 26);
        if ctx.hovered(chip) {
            p.box_(chip, Color(0, 0, 0, 24), 6);
        }
        let (text_x, text_width) = match lead.filter(|_| i == 0) {
            Some(symbol) => {
                p.symbol(symbol, chip.x + 10, chip.y + 5, 16, ink);
                (chip.x + 22, chip.width.saturating_sub(22))
            }
            None => (chip.x, chip.width),
        };
        p.label(
            text_x,
            chip.y + 5,
            text_width,
            name,
            13,
            ink,
            i == last,
            Align::Center,
        );
        if target.is_empty() {
            // A named place re-reads itself: Recent and Starred refresh from state.
            p.region(
                chip,
                &w.action("content:files-reload"),
                &format!("Reload {name}"),
            );
        } else {
            p.region(
                chip,
                &w.action(&format!("content:files-location:{target}")),
                &format!("Go to {name}"),
            );
        }
        if i < last {
            p.symbol(
                "chevron-right",
                chip.x + chip.width as i32,
                bar.y + 11,
                12,
                DIM,
            );
        }
        cx += width as i32;
        if cx >= bar.x + bar.width as i32 {
            break;
        }
    }
}

pub fn window_frame(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let r = w.rect;
    let radius = super::corner_radius(ctx.theme, w.maximized);
    if !w.maximized {
        if w.focused {
            p.drop_shadow(r, radius, 30, 110, 12);
        } else {
            p.drop_shadow(r, radius, 16, 70, 5);
        }
    }
    let dark = w.kind == "terminal";
    let header = match (dark, w.focused) {
        (true, true) => DARK_HEADER,
        (true, false) => Color::rgb(58, 58, 58),
        (false, true) => HEADER,
        (false, false) => HEADER_BACKDROP,
    };
    let ink = match (dark, w.focused) {
        (true, true) => Color::rgb(246, 246, 246),
        (true, false) => Color::rgb(160, 160, 160),
        (false, true) => INK,
        (false, false) => DIM,
    };
    p.border(
        r,
        if dark {
            Color::rgb(48, 10, 36)
        } else {
            Color::rgb(250, 250, 250)
        },
        radius,
        Color(0, 0, 0, if w.focused { 110 } else { 70 }),
    );
    if w.kind == "code" {
        return crate::apps::code::title_bar(p, ctx, w);
    }
    let inner = Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(2), 45);
    p.box_(inner, header, radius.saturating_sub(1));
    p.box_(Rect::new(inner.x, r.y + 24, inner.width, 22), header, 0);
    if !dark {
        p.hline(inner.x, r.y + 45, inner.width, Color(0, 0, 0, 30));
    }
    p.region(
        Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(2), 44),
        &w.action("drag"),
        &format!("Move {}", w.title),
    );
    let right = r.x + r.width as i32;
    match w.kind.as_str() {
        "browser" => {
            // Firefox draws its tab strip in the title bar; each tab is a real target.
            let room = r.width.saturating_sub(190);
            let width = (232.min(room / w.tabs.len().max(1) as u32))
                .max(92)
                .min(room);
            let mut x = r.x + 8;
            for (index, label) in w.tabs.iter().enumerate() {
                if x + width as i32 > r.x + r.width as i32 - 160 {
                    break;
                }
                let tab = Rect::new(x, r.y + 6, width, 34);
                let selected = index == w.active_tab;
                if selected {
                    p.drop_shadow(tab, 5, 4, 40, 1);
                }
                p.button(
                    tab,
                    if selected {
                        Color::WHITE
                    } else {
                        Color::TRANSPARENT
                    },
                    5,
                    &w.tab_select(index),
                    label,
                );
                p.symbol("globe", tab.x + 10, tab.y + 10, 14, DIM);
                p.left(
                    tab.x + 32,
                    tab.y + 9,
                    width.saturating_sub(62),
                    label,
                    12,
                    ink,
                );
                let close = Rect::new(tab.x + width as i32 - 28, tab.y + 7, 21, 21);
                p.button(
                    close,
                    Color::TRANSPARENT,
                    4,
                    &w.tab_close(index),
                    "Close tab",
                );
                p.symbol("close", close.x + 6, close.y + 6, 10, ink);
                x += width as i32 + 2;
            }
            let plus = Rect::new(x + 4, r.y + 8, 26, 26);
            p.button(plus, Color::TRANSPARENT, 4, &w.tab_new(), "New tab");
            p.symbol("plus", plus.x + 6, plus.y + 6, 14, ink);
        }
        "files" => {
            let sidebar = r.width > 470;
            let x = r.x + if sidebar { FILES_SIDEBAR as i32 } else { 0 };
            if sidebar {
                p.box_(
                    Rect::new(inner.x, inner.y, FILES_SIDEBAR, 45),
                    LIGHT,
                    radius.saturating_sub(1),
                );
                p.box_(
                    Rect::new(inner.x + 14, inner.y, FILES_SIDEBAR - 14, 45),
                    LIGHT,
                    0,
                );
                p.box_(Rect::new(inner.x, r.y + 24, FILES_SIDEBAR, 22), LIGHT, 0);
                p.vline(x, inner.y, 45, Color(0, 0, 0, 24));
                // `files-search` opens the tab's own query field and really filters the
                // rows below, so this is the search the sidebar always claimed to be.
                let find = Rect::new(r.x + 8, r.y + 7, 32, 32);
                header_button(p, ctx, find, "search", ink);
                p.region(
                    find,
                    &w.action("content:files-search"),
                    "Search this folder",
                );
                // A heading, not a control: GNOME's sidebar title does nothing.
                p.strong_center(r.x + 44, r.y + 14, FILES_SIDEBAR - 88, "Files", 14, ink);
                // GNOME's primary menu is gone rather than greyed: everything it would
                // hold is already on this header bar, and nothing opens a popover here.
            }
            // Files' arrows are history, like a browser's; the path bar climbs.
            let back = Rect::new(x + 8, r.y + 7, 34, 32);
            header_button(
                p,
                ctx,
                back,
                "chevron-left",
                if w.can_go_back { ink } else { DIM },
            );
            if w.can_go_back {
                p.region(
                    back,
                    &w.action("content:files-back"),
                    "Back to the previous folder",
                );
            } else {
                p.disabled("Back to the previous folder");
            }
            let forward = Rect::new(x + 46, r.y + 7, 34, 32);
            header_button(
                p,
                ctx,
                forward,
                "chevron-right",
                if w.can_go_forward { ink } else { DIM },
            );
            // Nothing to go forward to is a greyed control, not one that refuses.
            if w.can_go_forward {
                p.region(
                    forward,
                    &w.action("content:files-forward"),
                    "Forward to the next folder",
                );
            } else {
                p.disabled("Forward to the next folder");
            }
            let bar = Rect::new(x + 84, r.y + 7, (right - 210 - x - 84).max(60) as u32, 32);
            path_bar(p, ctx, w, bar, ink);
            if r.width > 560 {
                // `files-view` swaps the tab between its list and grid layouts for real.
                // The ⋮ beside it is gone: preferences and shortcuts do not exist, and
                // the header already carries every action the file manager has.
                let view = Rect::new(right - 200, r.y + 7, 48, 32);
                if ctx.hovered(view) {
                    p.box_(view, Color(128, 128, 128, 50), 8);
                }
                p.symbol("list-view", view.x + 6, r.y + 15, 16, ink);
                p.symbol("chevron-down", view.x + 28, r.y + 18, 10, ink);
                p.region(view, &w.action("content:files-view"), "Change view");
            }
        }
        "editor" => {
            let open = Rect::new(r.x + 8, r.y + 7, 74, 32);
            // This shell's file chooser is the Files application: documents open from there.
            if ctx.installed("files") {
                p.button(
                    open,
                    Color(0, 0, 0, if ctx.hovered(open) { 34 } else { 18 }),
                    8,
                    &w.action("content:shell:launch:files"),
                    "Open a document in Files",
                );
            } else {
                p.box_(open, Color(0, 0, 0, 18), 8);
                // Files is not installed, so there is nowhere to pick a document.
                p.disabled("Open a document");
            }
            p.left(open.x + 12, open.y + 8, 40, "Open", 13, ink);
            p.symbol("chevron-down", open.x + 54, open.y + 11, 10, ink);
            let new = Rect::new(r.x + 90, r.y + 7, 32, 32);
            header_button(p, ctx, new, "new-tab", ink);
            p.region(new, &w.action("content:shell:new"), "New document");
            let title = format!("{}{}", if w.modified { "• " } else { "" }, window_title(w));
            p.strong_center(
                r.x + 130,
                r.y + 7,
                r.width.saturating_sub(300),
                &title,
                13,
                ink,
            );
            let folder = if w.document.is_empty() {
                "Draft".to_owned()
            } else {
                // GNOME writes the folder under home as `~/...`.
                w.tilde(w.document.rsplit_once('/').map_or("/", |(dir, _)| {
                    if dir.is_empty() {
                        "/"
                    } else {
                        dir
                    }
                }))
            };
            p.center(
                r.x + 130,
                r.y + 24,
                r.width.saturating_sub(300),
                &folder,
                11,
                DIM,
            );
            // Text Editor's primary menu: new window, save, wrap, date and close.
            let menu = Rect::new(right - 152, r.y + 7, 32, 32);
            header_button(p, ctx, menu, "menu", ink);
            p.region(menu, "shell:panel:app-menu", "Primary menu");
        }
        "terminal" => {
            let new = Rect::new(r.x + 10, r.y + 7, 32, 32);
            header_button(p, ctx, new, "new-tab", ink);
            // One shell per window: the simulator has no tabbed terminal.
            p.region(new, &w.action("content:shell:new"), "New terminal window");
            p.strong_center(
                r.x + 120,
                r.y + 14,
                r.width.saturating_sub(240),
                &window_title(w),
                13,
                ink,
            );
            if r.width > 420 {
                // Terminal's primary menu: new window, full screen, reset and clear.
                // Find stays out: a scrollback search needs state the terminal lacks.
                let menu = Rect::new(right - 152, r.y + 7, 32, 32);
                header_button(p, ctx, menu, "menu", ink);
                p.region(menu, "shell:panel:app-menu", "Primary menu");
            }
        }
        _ => p.strong_center(
            r.x + 120,
            r.y + 14,
            r.width.saturating_sub(240),
            &w.title,
            13,
            ink,
        ),
    }
    for (offset, action, label) in [
        (104, "minimize", "Minimize window"),
        (70, "maximize", "Maximize or restore window"),
        (36, "close", "Close window"),
    ] {
        let cx = right - offset;
        let hit = Rect::new(cx, r.y + 9, 28, 28);
        let hovered = ctx.hovered(hit);
        p.region(hit, &w.action(action), label);
        let plate = match (dark, hovered) {
            (true, true) => Color(255, 255, 255, 60),
            (true, false) => Color(255, 255, 255, 30),
            (false, true) => Color(0, 0, 0, 50),
            (false, false) => Color(0, 0, 0, 22),
        };
        p.circle(cx + 14, r.y + 23, 12, plate);
        let (mx, my) = (cx + 14, r.y + 23);
        match action {
            "minimize" => p.line(vec![(mx - 4, my + 3), (mx + 4, my + 3)], ink, 1),
            "maximize" => {
                if w.maximized {
                    p.border(Rect::new(mx - 2, my - 5, 7, 7), Color::TRANSPARENT, 1, ink);
                    p.box_(Rect::new(mx - 5, my - 2, 7, 7), header, 1);
                    p.border(Rect::new(mx - 5, my - 2, 7, 7), Color::TRANSPARENT, 1, ink);
                } else {
                    p.border(Rect::new(mx - 4, my - 4, 9, 9), Color::TRANSPARENT, 1, ink);
                }
            }
            _ => {
                p.line(vec![(mx - 4, my - 4), (mx + 4, my + 4)], ink, 1);
                p.line(vec![(mx + 4, my - 4), (mx - 4, my + 4)], ink, 1);
            }
        }
    }
}

/// Firefox navigation toolbar beneath the title bar's tab strip.
pub fn browser_chrome(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let r = super::window_content_rect(ctx.theme, w.rect);
    let ink = if w.focused { INK } else { DIM };
    p.box_(
        Rect::new(r.x, r.y, r.width, 40),
        Color::rgb(249, 249, 251),
        0,
    );
    p.hline(r.x, r.y + 39, r.width, Color(0, 0, 0, 26));
    // Firefox greys what it cannot do: Back and Forward follow the tab's real history,
    // and Reload needs a page to re-request.
    for (i, (symbol, action, label, ready)) in [
        ("arrow-left", "back", "Back", w.can_go_back),
        ("arrow-right", "forward", "Forward", w.can_go_forward),
        ("reload", "reload", "Reload", !w.document.is_empty()),
    ]
    .into_iter()
    .enumerate()
    {
        let hit = Rect::new(r.x + 6 + i as i32 * 34, r.y + 4, 32, 32);
        if ready && ctx.hovered(hit) {
            p.box_(hit, Color(0, 0, 0, 16), 4);
        }
        p.symbol(
            symbol,
            hit.x + 8,
            hit.y + 8,
            16,
            if ready { ink } else { DIM },
        );
        if ready {
            p.region(hit, &w.action(&format!("content:shell:{action}")), label);
        } else {
            p.disabled(label);
        }
    }
    let trailing = if r.width > 520 { 80 } else { 10 };
    let field = Rect::new(
        r.x + 114,
        r.y + 4,
        r.width.saturating_sub(114 + trailing),
        32,
    );
    p.button(
        field,
        if w.editing {
            Color::WHITE
        } else {
            Color::rgb(240, 240, 244)
        },
        5,
        &w.action("content:shell:address"),
        "Address and search",
    );
    if w.editing {
        p.border(field, Color::TRANSPARENT, 5, Color::rgb(0, 96, 223));
    }
    let typed = w
        .title
        .split_once(" — ")
        .map_or(w.title.as_str(), |(_, a)| a);
    let inner = field.width.saturating_sub(96);
    if typed.is_empty() {
        p.symbol("search", field.x + 12, field.y + 9, 14, DIM);
        p.left(
            field.x + 36,
            field.y + 8,
            inner + 30,
            "Search or enter address",
            13,
            DIM,
        );
    } else {
        p.symbol("shield", field.x + 10, field.y + 9, 14, DIM);
        p.symbol("lock", field.x + 32, field.y + 9, 13, DIM);
        let shown = typed
            .strip_prefix("http://")
            .or_else(|| typed.strip_prefix("https://"))
            .unwrap_or(typed);
        let shown = if w.editing {
            typed
        } else {
            shown.trim_end_matches('/')
        };
        let end = p.left(field.x + 56, field.y + 8, inner, shown, 13, INK);
        if w.editing {
            p.box_(
                Rect::new(field.x + 57 + end as i32, field.y + 8, 1, 16),
                INK,
                0,
            );
        }
    }
    // The star saves the page to the machine's real bookmark list and reads its own
    // position back; an empty tab has no page, so there the star is greyed.
    let star = Rect::new(field.x + field.width as i32 - 32, field.y + 4, 24, 24);
    let saved = ctx.bookmarked && w.focused;
    if w.document.is_empty() {
        p.symbol("star-outline", star.x + 4, star.y + 4, 15, DIM);
        p.disabled("Bookmark this page");
    } else {
        if ctx.hovered(star) {
            p.box_(star, Color(0, 0, 0, 16), 4);
        }
        p.symbol(
            if saved { "star" } else { "star-outline" },
            star.x + 4,
            star.y + 4,
            15,
            if saved { ORANGE } else { ink },
        );
        p.region(
            star,
            "shell:bookmark",
            if saved {
                "Remove bookmark"
            } else {
                "Bookmark this page"
            },
        );
    }
    if r.width > 520 {
        let right = r.x + r.width as i32;
        // `shell:download` fetches the page on screen through the gateway and writes it
        // to ~/Downloads; with no page there is nothing to fetch, so it greys instead.
        let save = Rect::new(right - 74, r.y + 4, 32, 32);
        if w.document.is_empty() {
            p.symbol("download", save.x + 8, save.y + 8, 16, DIM);
            p.disabled("Save this page");
        } else {
            if ctx.hovered(save) {
                p.box_(save, Color(0, 0, 0, 16), 4);
            }
            p.symbol("download", save.x + 8, save.y + 8, 16, ink);
            p.region(save, "shell:download", "Save this page to Downloads");
        }
        // The badge counts files the browser really wrote, never a guess.
        if !ctx.downloads.is_empty() {
            p.circle(save.x + 25, save.y + 8, 7, ORANGE);
            p.label(
                save.x + 18,
                save.y + 2,
                14,
                &ctx.downloads.len().min(99).to_string(),
                9,
                Color::WHITE,
                true,
                Align::Center,
            );
        }
        // Firefox's ☰ is its settings menu, so it opens the machine's Settings panel.
        let menu = Rect::new(right - 42, r.y + 4, 32, 32);
        if ctx.hovered(menu) {
            p.box_(menu, Color(0, 0, 0, 16), 4);
        }
        p.symbol("menu", menu.x + 8, menu.y + 8, 16, ink);
        p.region(menu, "shell:settings", "Application menu");
    }
}

fn popover(p: &mut Painter, r: Rect) {
    p.drop_shadow(r, 22, 24, 120, 8);
    p.border(r, POPOVER, 22, POPOVER_EDGE);
    p.region(r, "shell:noop", "System menu");
}

fn panel_surface(p: &mut Painter, ctx: &ShellContext<'_>, panel: &str) {
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
    match panel {
        "calendar" | "notifications" => calendar(p, ctx),
        "settings" => settings(p, ctx),
        "context" => context_menu(p, ctx),
        "app-menu" => app_menu(p, ctx),
        _ => quick_settings(p, ctx),
    }
}

/// The primary (hamburger) menu of the focused Text Editor or Terminal window, dropped
/// from the button that opened it. Every entry acts on the window, its document or a
/// real machine setting.
fn app_menu(p: &mut Painter, ctx: &ShellContext<'_>) {
    let Some(w) = ctx.windows.iter().find(|w| w.focused) else {
        return;
    };
    let date = ctx.date();
    let entries: Vec<(String, String)> = match w.kind.as_str() {
        "editor" => vec![
            ("New Window".into(), "shell:new".into()),
            ("Save".into(), "shell:save".into()),
            (
                if ctx.settings.word_wrap {
                    "✓ Wrap Text".into()
                } else {
                    "Wrap Text".into()
                },
                "shell:toggle:word_wrap".into(),
            ),
            (
                "Insert Date and Time".into(),
                format!(
                    "shell:insert:{:04}-{:02}-{:02} {}",
                    date.year,
                    date.month,
                    date.day,
                    ctx.time()
                ),
            ),
            ("Close".into(), w.action("close")),
        ],
        "terminal" => vec![
            ("New Window".into(), "shell:new".into()),
            (
                if w.maximized {
                    "Leave Full Screen".into()
                } else {
                    "Full Screen".into()
                },
                w.action("maximize"),
            ),
            ("Reset and Clear".into(), "shell:terminal:clear".into()),
            ("Close Window".into(), w.action("close")),
        ],
        _ => return,
    };
    let right = w.rect.x + w.rect.width as i32;
    let r = Rect::new(
        (right - 240).clamp(72, ctx.width as i32 - 224),
        w.rect.y + 44,
        220,
        entries.len() as u32 * 34 + 12,
    );
    p.drop_shadow(r, 12, 18, 110, 6);
    p.border(r, Color::rgb(250, 250, 250), 12, Color(0, 0, 0, 50));
    p.region(r, "shell:noop", "Menu");
    for (i, (label, action)) in entries.iter().enumerate() {
        let row = Rect::new(r.x + 6, r.y + 6 + i as i32 * 34, r.width - 12, 34);
        if ctx.hovered(row) {
            p.box_(row, Color(0, 0, 0, 18), 8);
        }
        p.left(row.x + 12, row.y + 9, row.width - 24, label, 13, INK);
        p.region(row, action, label);
    }
}

fn calendar(p: &mut Painter, ctx: &ShellContext<'_>) {
    let text = Color::rgb(246, 246, 246);
    let faint = Color::rgb(170, 170, 170);
    let date = ctx.date();
    // The heading reports the world's own day; the grid below follows the paged month.
    let shown = ctx.panel_date();
    let paged = ctx.panel_month != 0;
    let two_column = ctx.width >= 900;
    let cal_width = 320.min(ctx.width.saturating_sub(90));
    let width = if two_column {
        cal_width + 380
    } else {
        cal_width
    };
    let x = ((ctx.width.saturating_sub(width)) / 2) as i32;
    let r = Rect::new(x, 38, width, 412);
    popover(p, r);
    let cx = if two_column {
        // The left column lists what applications really posted; when nothing has been
        // posted it says so, instead of saying so whatever the machine holds.
        if ctx.notifications.is_empty() {
            p.symbol(
                "bell",
                x + 190 - 24,
                r.y + 150,
                48,
                Color::rgb(110, 110, 110),
            );
            p.strong_center(x, r.y + 214, 380, "No Notifications", 15, faint);
        } else {
            p.strong(x + 20, r.y + 18, 200, "Notifications", 13, faint);
            let clear = Rect::new(x + 264, r.y + 12, 100, 26);
            p.button(
                clear,
                if ctx.hovered(clear) {
                    Color::rgb(100, 100, 100)
                } else {
                    Color::rgb(72, 72, 72)
                },
                13,
                "shell:notifications:seen",
                "Mark all as read",
            );
            p.center(clear.x, clear.y + 6, 100, "Clear", 12, text);
            for (i, notice) in ctx.notifications.iter().take(5).enumerate() {
                let row = Rect::new(x + 16, r.y + 50 + i as i32 * 62, 348, 56);
                p.box_(
                    row,
                    if ctx.hovered(row) {
                        Color::rgb(80, 80, 80)
                    } else {
                        Color::rgb(64, 64, 64)
                    },
                    14,
                );
                p.asset(
                    Rect::new(row.x + 12, row.y + 14, 28, 28),
                    &format!("icon/ubuntu/{}", notice.app),
                );
                p.strong(row.x + 52, row.y + 9, 250, &notice.title, 12, text);
                p.left(row.x + 52, row.y + 28, 250, &notice.body, 11, faint);
                // Unread is a real flag on the notice, not a guess from its age.
                if !notice.seen {
                    p.circle(row.x + 330, row.y + 16, 4, ORANGE);
                }
                p.region(row, &format!("shell:notice:{i}"), &notice.title);
            }
        }
        p.vline(x + 380, r.y + 1, r.height - 2, POPOVER_EDGE);
        let dnd = Rect::new(x + 16, r.y + r.height as i32 - 46, 348, 32);
        p.left(dnd.x + 4, dnd.y + 8, 160, "Do Not Disturb", 13, text);
        let on = ctx.switch("do_not_disturb");
        let switch = Rect::new(dnd.x + 300, dnd.y + 5, 44, 24);
        p.button(
            switch,
            if on { ORANGE } else { Color::rgb(95, 95, 95) },
            12,
            "shell:toggle:do_not_disturb",
            if on {
                "Turn Do Not Disturb off"
            } else {
                "Turn Do Not Disturb on"
            },
        );
        // The knob sits where the switch really is.
        p.circle(
            switch.x + if on { 32 } else { 12 },
            switch.y + 12,
            10,
            Color::rgb(230, 230, 230),
        );
        x + 380
    } else {
        x
    };
    p.left(
        cx + 24,
        r.y + 20,
        cal_width - 48,
        date.weekday_name(),
        13,
        faint,
    );
    p.strong(
        cx + 24,
        r.y + 40,
        cal_width - 48,
        &format!("{} {} {}", date.month_name(), date.day, date.year),
        19,
        text,
    );
    let grid = Rect::new(cx + 16, r.y + 84, cal_width - 32, 252);
    p.box_(grid, Color::rgb(64, 64, 64), 14);
    // Both chevrons page the grid's month for real.
    for (i, (symbol, action, label)) in [
        ("chevron-left", "shell:month:prev", "Previous month"),
        ("chevron-right", "shell:month:next", "Next month"),
    ]
    .into_iter()
    .enumerate()
    {
        let hit = Rect::new(
            grid.x + 4 + i as i32 * (grid.width as i32 - 36),
            grid.y + 6,
            32,
            28,
        );
        if ctx.hovered(hit) {
            p.box_(hit, Color(255, 255, 255, 24), 14);
        }
        p.symbol(symbol, hit.x + 8, hit.y + 8, 12, text);
        p.region(hit, action, label);
    }
    // GNOME's month heading returns to today, which only moves when the grid has left it.
    let heading = Rect::new(grid.x + 44, grid.y + 6, grid.width - 88, 28);
    if paged && ctx.hovered(heading) {
        p.box_(heading, Color(255, 255, 255, 24), 14);
    }
    p.strong_center(
        heading.x,
        grid.y + 11,
        heading.width,
        &if paged {
            format!("{} {}", shown.month_name(), shown.year)
        } else {
            shown.month_name().to_owned()
        },
        13,
        text,
    );
    if paged {
        p.region(
            heading,
            "shell:month:today",
            &format!("Back to {} {}", date.month_name(), date.year),
        );
    }
    let cell = (grid.width - 16) / 7;
    // GNOME weeks begin on Monday.
    for (i, day) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
        p.center(
            grid.x + 8 + (i as u32 * cell) as i32,
            grid.y + 42,
            cell,
            day,
            11,
            faint,
        );
    }
    let offset = (shown.first_weekday + 6) % 7;
    // Each day opens Calendar on that day; days before the world began are only text.
    for day in 1..=shown.days_in_month {
        let slot = day - 1 + offset;
        let dx = grid.x + 8 + ((slot % 7) as u32 * cell) as i32;
        let dy = grid.y + 66 + (slot / 7) as i32 * 30;
        if let Some(open) = ctx.open_day(day) {
            p.region_above(
                Rect::new(dx, dy - 5, cell, 28),
                &open,
                &format!("{} {day}", shown.month_name()),
            );
        }
        // Only the world's own day is ringed, and only on its own month.
        let today = !paged && day == date.day;
        if today {
            p.circle(dx + cell as i32 / 2, dy + 9, 14, ORANGE);
        }
        p.label(
            dx,
            dy + 1,
            cell,
            &day.to_string(),
            12,
            text,
            today,
            Align::Center,
        );
    }
    let row = Rect::new(cx + 16, r.y + 348, cal_width - 32, 48);
    p.box_(row, Color::rgb(64, 64, 64), 14);
    if ctx.installed("calendar") {
        if ctx.hovered(row) {
            p.box_(row, Color(255, 255, 255, 20), 14);
        }
        p.region(row, "shell:launch:calendar", "Open calendar application");
        p.strong(row.x + 16, row.y + 8, row.width - 32, "Today", 12, text);
        p.left(
            row.x + 16,
            row.y + 25,
            row.width - 32,
            "Open Calendar",
            12,
            faint,
        );
    } else {
        p.strong(row.x + 16, row.y + 8, row.width - 32, "Today", 12, text);
        p.left(
            row.x + 16,
            row.y + 25,
            row.width - 32,
            "No Events",
            12,
            faint,
        );
    }
}

fn quick_settings(p: &mut Painter, ctx: &ShellContext<'_>) {
    let text = Color::rgb(246, 246, 246);
    let width = 360.min(ctx.width.saturating_sub(84));
    let x = ctx.width as i32 - width as i32 - 8;
    let r = Rect::new(x, 38, width, 356);
    popover(p, r);
    // Top row: a laptop's battery pill, then round system buttons. A desktop computer
    // has no battery, so GNOME shows no pill there. The machine keeps no charge model:
    // a laptop here is on mains power and full, which is what the pill says.
    let pill = if ctx.battery { 96 } else { 0 };
    if ctx.battery {
        let b = Rect::new(x + 16, r.y + 16, 84, 36);
        p.box_(
            b,
            if ctx.hovered(b) {
                Color::rgb(110, 110, 110)
            } else {
                TILE
            },
            18,
        );
        p.symbol("battery", b.x + 12, b.y + 10, 16, text);
        p.left(b.x + 34, b.y + 9, 46, "100 %", 13, text);
        p.region(b, "shell:settings", "Power settings, fully charged");
    }
    // Screenshot rasterises the display and writes a real PNG to ~/Pictures; every
    // button in this row changes the machine rather than decorating the panel.
    for (i, (symbol, action, label)) in [
        ("screenshot", "shell:screenshot", "Take screenshot"),
        ("gear", "shell:settings", "Settings"),
        ("lock", "shell:power:lock", "Lock screen"),
        ("power", "shell:power:off", "Power off"),
    ]
    .iter()
    .enumerate()
    {
        // GNOME 46: screenshot, settings and lock from the left, power alone at the right.
        let bx = if i == 3 {
            x + width as i32 - 16 - 36
        } else {
            x + 16 + pill + i as i32 * 44
        };
        let hit = Rect::new(bx, r.y + 16, 36, 36);
        p.circle(
            bx + 18,
            r.y + 34,
            18,
            if ctx.hovered(hit) {
                Color::rgb(110, 110, 110)
            } else {
                TILE
            },
        );
        p.symbol(symbol, bx + 10, r.y + 26, 16, text);
        p.region(hit, action, label);
    }
    // Sliders are twenty-one discrete stops, so a click lands on an exact level.
    const STOPS: i32 = 20;
    for (i, (symbol, name)) in [("volume", "volume"), ("sun", "brightness")]
        .iter()
        .enumerate()
    {
        let sy = r.y + 76 + i as i32 * 44;
        p.symbol(symbol, x + 22, sy, 16, text);
        let track = Rect::new(x + 52, sy + 6, width - 76, 4);
        let span = track.width as i32;
        for step in 0..=STOPS {
            let lo = if step == 0 {
                0
            } else {
                span * (2 * step - 1) / (2 * STOPS)
            };
            let hi = if step == STOPS {
                span
            } else {
                span * (2 * step + 1) / (2 * STOPS)
            };
            let percent = step * 100 / STOPS;
            p.button(
                Rect::new(track.x + lo, sy - 2, (hi - lo).max(1) as u32, 24),
                Color::TRANSPARENT,
                0,
                &format!("shell:set:{name}:{percent}"),
                &format!("Set {name} to {percent} %"),
            );
        }
        // The filled portion is the level the machine really holds.
        let fill = track.width * u32::from(ctx.level(name)).min(100) / 100;
        p.box_(track, Color::rgb(100, 100, 100), 2);
        p.box_(Rect::new(track.x, track.y, fill, 4), ORANGE, 2);
        p.circle(track.x + fill as i32, track.y + 2, 9, Color::WHITE);
    }
    let pill = (width - 44) / 2;
    for (i, (symbol, name, switch)) in [
        ("wifi", "Wi-Fi", "wifi"),
        ("bluetooth", "Bluetooth", "bluetooth"),
        ("leaf", "Power Mode", "battery_saver"),
        ("night-light", "Night Light", "night_light"),
        ("moon", "Dark Style", "dark_mode"),
        ("airplane", "Airplane Mode", "airplane_mode"),
    ]
    .iter()
    .enumerate()
    {
        let on = ctx.switch(switch);
        let px = x + 16 + (i % 2) as i32 * (pill as i32 + 12);
        let py = r.y + 170 + (i / 2) as i32 * 60;
        let slot = Rect::new(px, py, pill, 48);
        p.button(
            slot,
            match (on, ctx.hovered(slot)) {
                (true, true) => Color::rgb(243, 104, 52),
                (true, false) => ORANGE,
                (false, true) => Color::rgb(110, 110, 110),
                (false, false) => TILE,
            },
            24,
            &format!("shell:toggle:{switch}"),
            &format!("Turn {name} {}", if on { "off" } else { "on" }),
        );
        p.symbol(symbol, px + 16, py + 16, 16, text);
        // Each pill states the position the switch is really in.
        let detail = match (*switch, on) {
            ("wifi", true) => "Connected",
            ("battery_saver", true) => "Power Saver",
            ("battery_saver", false) => "Balanced",
            (_, true) => "On",
            (_, false) => "Off",
        };
        p.strong(px + 42, py + 8, pill - 52, name, 13, text);
        p.left(
            px + 42,
            py + 25,
            pill - 52,
            detail,
            11,
            Color(255, 255, 255, 200),
        );
    }
}

fn settings(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = 560.min(ctx.width.saturating_sub(90));
    let height = 360.min(ctx.height.saturating_sub(60));
    let x = 68 + (ctx.width as i32 - 68 - width as i32) / 2;
    let y = 32 + (ctx.height as i32 - 32 - height as i32) / 2;
    let r = Rect::new(x, y, width, height);
    p.drop_shadow(r, 12, 30, 110, 12);
    p.border(r, Color::rgb(250, 250, 250), 12, Color(0, 0, 0, 110));
    p.region(r, "shell:noop", "Settings");
    p.box_(Rect::new(x + 1, y + 1, width - 2, 45), HEADER, 11);
    p.box_(Rect::new(x + 1, y + 24, width - 2, 22), HEADER, 0);
    p.strong_center(x, y + 14, width, "About", 14, INK);
    let close = Rect::new(x + width as i32 - 36, y + 9, 28, 28);
    p.circle(
        close.x + 14,
        close.y + 14,
        12,
        Color(0, 0, 0, if ctx.hovered(close) { 50 } else { 22 }),
    );
    p.symbol("close", close.x + 9, close.y + 9, 10, INK);
    p.region(close, "shell:dismiss", "Close Settings");
    p.strong_center(x, y + 70, width, "Ubuntu 24.04 LTS", 22, INK);
    let card = Rect::new(x + 40, y + 120, width - 80, 144);
    p.border(card, Color::WHITE, 12, Color(0, 0, 0, 30));
    // GNOME's About rows are read-only facts, so they are painted, never clickable.
    for (i, (key, value)) in [
        ("Device Name", "alice-ubuntu".to_owned()),
        ("Display", format!("{} × {}", ctx.width, ctx.height)),
        ("Windowing System", "Wayland".to_owned()),
        ("Open Windows", ctx.windows.len().to_string()),
    ]
    .iter()
    .enumerate()
    {
        let ry = card.y + 10 + i as i32 * 34;
        p.left(card.x + 16, ry, 200, key, 13, INK);
        p.right(card.x + card.width as i32 - 216, ry, 200, value, 13, DIM);
        if i < 3 {
            p.hline(card.x + 1, ry + 25, card.width - 2, Color(0, 0, 0, 20));
        }
    }
    let launch = Rect::new(x + width as i32 / 2 - 90, y + height as i32 - 60, 180, 34);
    p.button(launch, ORANGE, 8, "shell:launcher", "Show Applications");
    p.strong_center(
        launch.x,
        launch.y + 9,
        180,
        "Show Applications",
        13,
        Color::WHITE,
    );
}

fn context_menu(p: &mut Painter, ctx: &ShellContext<'_>) {
    let (mx, my) = ctx
        .anchor
        .or(ctx.hover)
        .unwrap_or((ctx.width as i32 / 2, ctx.height as i32 / 3));
    let mut entries: Vec<(String, String)> = Vec::new();
    // Over Files with a file selected, the menu is about that file first, as Nautilus's
    // is: open it, and star or unstar it (the grid has no star column).
    if let Some(w) = ctx
        .windows
        .iter()
        .find(|w| w.focused && !w.minimized && w.kind == "files" && !w.selection.is_empty())
    {
        entries.push(("Open".into(), w.action("content:files-open")));
        let starred = w.chrome("starred") == Some("1");
        entries.push((
            if starred { "Unstar" } else { "Star" }.into(),
            w.action("content:files-star"),
        ));
    }
    for (label, action) in [
        ("New Window", "shell:new"),
        ("Open in Terminal", "shell:launch:terminal"),
        ("Show Applications", "shell:launcher"),
        ("Settings", "shell:settings"),
    ] {
        entries.push((label.into(), action.into()));
    }
    let tall = entries.len() as i32 * 34 + 12;
    let r = Rect::new(
        mx.clamp(72, ctx.width as i32 - 224),
        my.clamp(36, (ctx.height as i32 - tall - 2).max(36)),
        220,
        tall as u32,
    );
    p.drop_shadow(r, 12, 18, 110, 6);
    p.border(r, Color::rgb(250, 250, 250), 12, Color(0, 0, 0, 50));
    p.region(r, "shell:noop", "Menu");
    for (i, (label, action)) in entries.iter().enumerate() {
        let row = Rect::new(r.x + 6, r.y + 6 + i as i32 * 34, r.width - 12, 34);
        if ctx.hovered(row) {
            p.box_(row, Color(0, 0, 0, 18), 8);
        }
        p.left(row.x + 12, row.y + 9, row.width - 24, label, 13, INK);
        p.region(row, action, label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_scene::DesktopTheme;

    fn context<'a>(windows: &'a [WindowView], active: bool) -> ShellContext<'a> {
        ShellContext {
            theme: DesktopTheme::Ubuntu,
            width: 1024,
            height: 768,
            clock_us: 0,
            title: "",
            launcher_open: false,
            active,
            windows,
            installed_apps: &[],
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
            home_page: 0,
            typed: "",
            user: "alice",
            home: "/Users/alice",
            recents: &[],
            battery: false,
            anchor: None,
            overview: Default::default(),
        }
    }
    #[test]
    fn titlebar_controls_do_not_get_captured_by_drag_region() {
        let windows = [WindowView {
            id: 42,
            title: "Files".into(),
            kind: "files".into(),
            rect: Rect::new(120, 90, 700, 430),
            focused: true,
            can_go_back: true,
            ..Default::default()
        }];
        let ctx = context(&windows, true);
        let mut painter = Painter::new(1024, 768);
        window_frame(&mut painter, &ctx, &windows[0]);
        for (x, expected) in [
            (320, "content:files-back"),
            (600, "drag"),
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
    #[test]
    fn dock_restores_latest_running_window_and_launches_missing_apps() {
        let windows = [7, 19].map(|id| WindowView {
            id,
            title: "Terminal".into(),
            kind: "terminal".into(),
            rect: Rect::new(120, 90, 600, 400),
            minimized: true,
            ..Default::default()
        });
        let ctx = context(&windows, false);
        let mut painter = Painter::new(1024, 768);
        chrome(&mut painter, &ctx);
        // The Terminal slot, wherever the dock's order puts it, raises the latest window.
        let slot = painter
            .scene
            .nodes
            .iter()
            .find(|n| n.semantic.as_ref().is_some_and(|s| s.label == "Terminal"))
            .expect("a Terminal slot");
        let b = slot.bounds;
        assert_eq!(
            painter
                .scene
                .hit_test(32, b.y + b.height as i32 / 2)
                .and_then(|n| n.interaction.as_deref()),
            Some("window:19:focus")
        );
        assert_eq!(
            painter
                .scene
                .hit_test(32, 68)
                .and_then(|n| n.interaction.as_deref()),
            Some("shell:launch:browser")
        );
    }
    #[test]
    fn system_menu_absorbs_clicks_and_dismisses_outside() {
        let mut ctx = context(&[], false);
        ctx.panel = Some("quick");
        let mut painter = Painter::new(1024, 768);
        chrome(&mut painter, &ctx);
        let hit = |x, y| {
            painter
                .scene
                .hit_test(x, y)
                .and_then(|n| n.interaction.as_deref())
        };
        // Bare popover background between the lock and power buttons.
        assert_eq!(hit(900, 60), Some("shell:noop"));
        assert_eq!(hit(300, 500), Some("shell:dismiss"));
        assert_eq!(
            hit(1024 - 8 - 360 + 16 + 44 + 18, 72),
            Some("shell:settings")
        );
    }
    #[test]
    fn quick_settings_pills_switches_and_power_buttons_carry_real_ids() {
        let mut ctx = context(&[], false);
        ctx.panel = Some("quick");
        let mut painter = Painter::new(1024, 768);
        chrome(&mut painter, &ctx);
        let hit = |x, y| {
            painter
                .scene
                .hit_test(x, y)
                .and_then(|n| n.interaction.as_deref())
        };
        // Six pills, two per row, in the order the panel paints them.
        let (left, right) = (656 + 16 + 40, 656 + 16 + (360 - 44) / 2 + 12 + 40);
        for (row, (a, b)) in [
            ("wifi", "bluetooth"),
            ("battery_saver", "night_light"),
            ("dark_mode", "airplane_mode"),
        ]
        .iter()
        .enumerate()
        {
            let y = 38 + 170 + row as i32 * 60 + 24;
            assert_eq!(hit(left, y), Some(format!("shell:toggle:{a}").as_str()));
            assert_eq!(hit(right, y), Some(format!("shell:toggle:{b}").as_str()));
        }
        // No battery pill: the machine has no battery. Screenshot, Settings, lock
        // and power off all change the machine.
        assert_eq!(hit(656 + 240, 72), Some("shell:noop"));
        assert_eq!(hit(656 + 16 + 44 + 18, 72), Some("shell:settings"));
        assert_eq!(hit(656 + 16 + 18, 72), Some("shell:screenshot"));
        assert_eq!(hit(656 + 16 + 2 * 44 + 18, 72), Some("shell:power:lock"));
        assert_eq!(hit(656 + 360 - 16 - 18, 72), Some("shell:power:off"));
    }
    #[test]
    fn sliders_are_discrete_stops_that_set_the_level_they_paint() {
        let mut ctx = context(&[], false);
        ctx.panel = Some("quick");
        let settings = crate::SystemSettings {
            volume: 25,
            ..crate::SystemSettings::DEFAULT
        };
        ctx.settings = &settings;
        let mut painter = Painter::new(1024, 768);
        chrome(&mut painter, &ctx);
        let track = Rect::new(656 + 52, 38 + 76 + 6, 360 - 76, 4);
        let hit = |x, y| {
            painter
                .scene
                .hit_test(x, y)
                .and_then(|n| n.interaction.as_deref())
        };
        assert_eq!(hit(track.x, track.y), Some("shell:set:volume:0"));
        assert_eq!(
            hit(track.x + track.width as i32 / 2, track.y),
            Some("shell:set:volume:50")
        );
        assert_eq!(
            hit(track.x + track.width as i32 - 1, track.y),
            Some("shell:set:volume:100")
        );
        assert_eq!(
            hit(track.x + 52, 38 + 76 + 44 + 6),
            Some("shell:set:brightness:20")
        );
        // The orange fill reports the level the machine really holds.
        let fill = track.width * 25 / 100;
        assert!(painter
            .scene
            .nodes
            .iter()
            .any(|n| n.bounds == Rect::new(track.x, track.y, fill, 4)));
    }
    #[test]
    fn do_not_disturb_switch_toggles_and_shows_its_real_position() {
        let on = crate::SystemSettings {
            do_not_disturb: true,
            ..crate::SystemSettings::DEFAULT
        };
        let mut positions = Vec::new();
        for settings in [&crate::SystemSettings::DEFAULT, &on] {
            let mut ctx = context(&[], false);
            ctx.panel = Some("calendar");
            ctx.settings = settings;
            let mut painter = Painter::new(1024, 768);
            chrome(&mut painter, &ctx);
            let x = (1024 - (320 + 380)) / 2;
            let switch = Rect::new(x + 16 + 300, 38 + 412 - 46 + 5, 44, 24);
            assert_eq!(
                painter
                    .scene
                    .hit_test(switch.x + 22, switch.y + 12)
                    .and_then(|n| n.interaction.as_deref()),
                Some("shell:toggle:do_not_disturb")
            );
            positions.push(
                painter
                    .scene
                    .nodes
                    .iter()
                    .filter(|n| n.bounds.width == 20 && n.bounds.y == switch.y + 2)
                    .map(|n| n.bounds.x)
                    .next_back()
                    .expect("the knob is painted"),
            );
        }
        assert!(positions[0] < positions[1]);
    }
    #[test]
    fn locked_and_powered_off_screens_cover_everything_with_a_real_way_back() {
        for screen in [crate::ScreenState::Locked, crate::ScreenState::Off] {
            let windows = [WindowView {
                id: 3,
                kind: "terminal".into(),
                rect: Rect::new(120, 90, 600, 400),
                ..Default::default()
            }];
            let mut ctx = context(&windows, true);
            ctx.screen = screen;
            ctx.panel = Some("quick");
            let mut painter = Painter::new(1024, 768);
            painter.z = 1_000_000;
            chrome(&mut painter, &ctx);
            for (x, y) in [(20, 20), (512, 384), (1000, 700)] {
                assert_eq!(
                    painter
                        .scene
                        .hit_test(x, y)
                        .and_then(|n| n.interaction.as_deref()),
                    Some("shell:power:wake"),
                    "{screen:?} leaks a control at {x},{y}"
                );
            }
            // Neither the dock nor the system menu survives behind the shield.
            assert!(!painter
                .scene
                .nodes
                .iter()
                .any(|n| n.interaction.as_deref() == Some("shell:launcher")));
        }
    }
    #[test]
    fn files_header_navigates_by_breadcrumb_and_offers_forward() {
        let window = |width: u32| WindowView {
            id: 42,
            title: "Files".into(),
            kind: "files".into(),
            document: "/home/alice/work".into(),
            rect: Rect::new(60, 90, width, 430),
            focused: true,
            can_go_back: true,
            can_go_forward: true,
            ..Default::default()
        };
        let crumbs = |w: &WindowView| {
            let windows = [w.clone()];
            let ctx = context(&windows, true);
            let mut painter = Painter::new(1024, 768);
            window_frame(&mut painter, &ctx, &windows[0]);
            let trail: Vec<String> = painter
                .scene
                .nodes
                .iter()
                .filter_map(|n| n.interaction.as_deref())
                .filter(|a| a.contains("files-location:"))
                .map(str::to_owned)
                .collect();
            let hit = |x: i32| {
                painter
                    .scene
                    .hit_test(x, 113)
                    .and_then(|n| n.interaction.as_deref())
                    .map(str::to_owned)
            };
            (trail, hit(260), hit(300))
        };
        let (trail, back, forward) = crumbs(&window(900));
        // Files' arrows walk history, as a browser's do; the path bar climbs.
        assert_eq!(back.as_deref(), Some("window:42:content:files-back"));
        assert_eq!(forward.as_deref(), Some("window:42:content:files-forward"));
        assert_eq!(
            trail,
            [
                "window:42:content:files-location:/",
                "window:42:content:files-location:/home",
                "window:42:content:files-location:/home/alice",
                "window:42:content:files-location:/home/alice/work",
            ]
        );
        // A short bar elides from the front and always keeps the folder in view.
        let (short, _, _) = crumbs(&window(620));
        assert!(short.len() < trail.len());
        assert_eq!(short.last(), trail.last());
        // Inside the home folder the trail starts at Home, as Files starts it.
        let mut homed = window(900);
        homed.home = "/home/alice".into();
        let (trail, _, _) = crumbs(&homed);
        assert_eq!(
            trail,
            [
                "window:42:content:files-location:/home/alice",
                "window:42:content:files-location:/home/alice/work",
            ]
        );
        // A named place is one button that re-reads it, not a trail of folders.
        homed.caption = "Starred".into();
        let (trail, _, _) = crumbs(&homed);
        assert!(trail.is_empty());
        // With no history, Back is greyed rather than refused.
        let mut fresh = window(900);
        fresh.can_go_back = false;
        let (_, back, _) = crumbs(&fresh);
        assert_ne!(back.as_deref(), Some("window:42:content:files-back"));
    }
    #[test]
    fn editor_and_terminal_header_controls_open_real_windows() {
        for (kind, label) in [
            ("editor", "New document"),
            ("terminal", "New terminal window"),
        ] {
            let windows = [WindowView {
                id: 8,
                title: "Text Editor".into(),
                kind: kind.into(),
                rect: Rect::new(100, 80, 640, 420),
                focused: true,
                ..Default::default()
            }];
            let ctx = context(&windows, true);
            let mut painter = Painter::new(1024, 768);
            window_frame(&mut painter, &ctx, &windows[0]);
            let new = painter
                .scene
                .nodes
                .iter()
                .find(|n| n.semantic.as_ref().is_some_and(|s| s.label == label))
                .expect("the header paints a new-window control");
            assert_eq!(
                new.interaction.as_deref(),
                Some("window:8:content:shell:new")
            );
            assert!(!new.semantic.as_ref().unwrap().disabled);
        }
        let windows = [WindowView {
            id: 8,
            kind: "editor".into(),
            rect: Rect::new(100, 80, 640, 420),
            focused: true,
            ..Default::default()
        }];
        let ctx = context(&windows, true);
        let mut painter = Painter::new(1024, 768);
        window_frame(&mut painter, &ctx, &windows[0]);
        assert_eq!(
            painter
                .scene
                .hit_test(120, 103)
                .and_then(|n| n.interaction.as_deref()),
            Some("window:8:content:shell:launch:files")
        );
    }
    fn actions(p: &Painter) -> Vec<&str> {
        p.scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.as_deref())
            .collect()
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
    fn labelled<'a>(p: &'a Painter, label: &str) -> Option<&'a cw_scene::Node> {
        p.scene
            .nodes
            .iter()
            .find(|n| n.semantic.as_ref().is_some_and(|s| s.label == label))
    }
    /// Hit-test the middle of a labelled control, so occlusion counts too.
    fn hit_labelled<'a>(p: &'a Painter, label: &str) -> Option<&'a str> {
        let node = labelled(p, label)?;
        p.scene
            .hit_test(
                node.bounds.x + node.bounds.width as i32 / 2,
                node.bounds.y + node.bounds.height as i32 / 2,
            )
            .and_then(|n| n.interaction.as_deref())
    }
    #[test]
    fn activities_carries_the_whole_grid_and_the_dock_only_its_favourites() {
        let mut ctx = context(&[], true);
        ctx.launcher_open = true;
        let mut p = Painter::new(1024, 768);
        chrome(&mut p, &ctx);
        // The grid pages; its dots reach the rest, and every page together is the grid.
        let mut launches: Vec<String> = actions(&p).iter().map(|a| a.to_string()).collect();
        let pages = launches
            .iter()
            .filter(|a| a.starts_with("shell:home-page:"))
            .count()
            .max(1);
        for page in 1..pages {
            ctx.home_page = page as u32;
            let mut next = Painter::new(1024, 768);
            chrome(&mut next, &ctx);
            launches.extend(actions(&next).iter().map(|a| a.to_string()));
        }
        for (kind, _) in APPS {
            assert!(
                launches.contains(&format!("shell:launch:{kind}")),
                "Activities hides {kind}"
            );
        }
        // The dock keeps its favourites; the rest is one "Show applications" away.
        let ctx = context(&[], true);
        let mut docked = Painter::new(1024, 768);
        chrome(&mut docked, &ctx);
        let dock = actions(&docked);
        for kind in ["notes", "contacts", "calculator", "clock", "settings"] {
            assert!(!dock.contains(&format!("shell:launch:{kind}").as_str()));
        }
        assert!(dock.contains(&"shell:launcher"));
        // Only what the machine has installed is ever offered.
        let apps: Vec<String> = vec!["files".into(), "clock".into()];
        let mut ctx = context(&[], true);
        ctx.installed_apps = &apps;
        ctx.launcher_open = true;
        let mut p = Painter::new(1024, 768);
        chrome(&mut p, &ctx);
        for action in actions(&p) {
            if let Some(kind) = action.strip_prefix("shell:launch:") {
                assert!(apps.iter().any(|a| a == kind), "{kind} is not installed");
            }
        }
    }
    #[test]
    fn the_calendar_grid_pages_the_month_the_panel_reports() {
        let mut ctx = context(&[], true);
        ctx.panel = Some("calendar");
        let mut p = Painter::new(1024, 768);
        chrome(&mut p, &ctx);
        assert_eq!(hit_labelled(&p, "Previous month"), Some("shell:month:prev"));
        assert_eq!(hit_labelled(&p, "Next month"), Some("shell:month:next"));
        assert!(texts(&p).contains(&"September"));
        assert!(!actions(&p).contains(&"shell:month:today"));
        // One month on, the grid is February's, drawn from `panel_date`.
        ctx.panel_month = 17;
        let mut p = Painter::new(1024, 768);
        chrome(&mut p, &ctx);
        let shown = ctx.panel_date();
        assert_eq!(
            (shown.year, shown.month, shown.days_in_month),
            (2028, 2, 29)
        );
        assert!(texts(&p).contains(&"February 2028"));
        assert_eq!(
            hit_labelled(&p, "Back to September 2026"),
            Some("shell:month:today")
        );
        let days: Vec<&str> = texts(&p)
            .into_iter()
            .filter(|t| t.parse::<u64>().is_ok_and(|d| (1..=31).contains(&d)))
            .collect();
        // A leap February, and no cell of it is a target.
        assert_eq!(days.len(), 29);
        assert!(days.contains(&"29"));
    }
    #[test]
    fn firefox_greys_history_and_reload_it_cannot_perform() {
        let fresh = [WindowView {
            id: 5,
            kind: "browser".into(),
            rect: Rect::new(60, 60, 880, 600),
            focused: true,
            tabs: vec!["New tab".into()],
            ..Default::default()
        }];
        let ctx = context(&fresh, true);
        let mut p = Painter::new(1024, 768);
        browser_chrome(&mut p, &ctx, &fresh[0]);
        for label in ["Back", "Forward", "Reload"] {
            let node = labelled(&p, label).unwrap();
            let semantic = node.semantic.as_ref().unwrap();
            assert!(semantic.disabled && !semantic.focusable, "{label}");
            assert!(node.interaction.is_none(), "{label}");
            assert_eq!(hit_labelled(&p, label), None, "{label}");
        }
        let loaded = [WindowView {
            document: "http://example.test/".into(),
            can_go_back: true,
            can_go_forward: true,
            ..fresh[0].clone()
        }];
        let ctx = context(&loaded, true);
        let mut p = Painter::new(1024, 768);
        browser_chrome(&mut p, &ctx, &loaded[0]);
        for (label, action) in [
            ("Back", "back"),
            ("Forward", "forward"),
            ("Reload", "reload"),
        ] {
            assert_eq!(
                hit_labelled(&p, label),
                Some(format!("window:5:content:shell:{action}").as_str())
            );
        }
    }
    #[test]
    fn nothing_decorative_is_left_clickable() {
        let windows = [WindowView {
            id: 5,
            kind: "browser".into(),
            rect: Rect::new(60, 60, 880, 600),
            focused: true,
            tabs: vec!["Start".into()],
            ..Default::default()
        }];
        let mut ctx = context(&windows, true);
        ctx.launcher_open = true;
        let mut painter = Painter::new(1024, 768);
        chrome(&mut painter, &ctx);
        window_frame(&mut painter, &ctx, &windows[0]);
        browser_chrome(&mut painter, &ctx, &windows[0]);
        let announced: Vec<_> = painter
            .scene
            .nodes
            .iter()
            .filter(|n| n.semantic.as_ref().is_some_and(|s| s.disabled))
            .collect();
        // What is left greyed is only what this window genuinely cannot do: an empty
        // tab has no page to bookmark and none to save.
        let labels: Vec<&str> = announced
            .iter()
            .map(|n| n.semantic.as_ref().unwrap().label.as_str())
            .collect();
        assert!(labels.contains(&"Bookmark this page"), "{labels:?}");
        assert!(labels.contains(&"Save this page"), "{labels:?}");
        for n in announced {
            assert!(n.interaction.is_none());
            assert!(!n.semantic.as_ref().unwrap().focusable);
        }
        // The dock's trash and the overview's workspace tile are real now, and the
        // app grid carries no page indicator at all.
        let live = actions(&painter);
        assert!(live.contains(&"shell:trash"));
        assert!(live.contains(&"shell:workspace:0"));
        assert!(!labels.contains(&"Page 1 of 1"));
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
    fn the_calendar_panel_lists_the_notices_the_machine_really_posted() {
        let posted = [notice("files", "Screenshot taken", false)];
        let mut ctx = context(&[], true);
        ctx.width = 1024;
        ctx.panel = Some("calendar");
        let mut p = Painter::new(1024, 768);
        chrome(&mut p, &ctx);
        assert!(texts(&p).contains(&"No Notifications"));
        assert!(!actions(&p).iter().any(|a| a.starts_with("shell:notice:")));
        ctx.notifications = &posted;
        let mut p = Painter::new(1024, 768);
        chrome(&mut p, &ctx);
        assert!(!texts(&p).contains(&"No Notifications"));
        assert_eq!(hit_labelled(&p, "Screenshot taken"), Some("shell:notice:0"));
        assert_eq!(
            hit_labelled(&p, "Mark all as read"),
            Some("shell:notifications:seen")
        );
    }

    #[test]
    fn the_activities_switcher_shows_the_real_desktops() {
        let windows = [WindowView {
            id: 5,
            kind: "editor".into(),
            rect: Rect::new(120, 90, 500, 360),
            focused: true,
            ..Default::default()
        }];
        // One desktop: switching to it is real, and the last one cannot be closed.
        let mut ctx = context(&windows, true);
        ctx.launcher_open = true;
        let mut p = Painter::new(1024, 768);
        chrome(&mut p, &ctx);
        assert_eq!(hit_labelled(&p, "Desktop 1"), Some("shell:workspace:0"));
        assert_eq!(hit_labelled(&p, "New desktop"), Some("shell:workspace:new"));
        assert!(labelled(&p, "Close desktop 1").is_none());
        // Three desktops, sitting on the second: each tile carries its own index, and
        // only the one on screen offers the close the handler would accept.
        ctx.workspaces = 3;
        ctx.workspace = 1;
        let mut p = Painter::new(1024, 768);
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
            hit_labelled(&p, "Close desktop 2"),
            Some("shell:workspace:close")
        );
        assert!(labelled(&p, "Close desktop 3").is_none());
        // At the machine's limit the plus is announced unavailable and cannot be hit.
        ctx.workspaces = crate::WORKSPACE_LIMIT;
        ctx.workspace = 0;
        let mut p = Painter::new(1024, 768);
        chrome(&mut p, &ctx);
        let node = labelled(&p, "New desktop").unwrap();
        assert!(node.semantic.as_ref().unwrap().disabled);
        assert!(node.interaction.is_none() && !node.semantic.as_ref().unwrap().focusable);
        assert_ne!(hit_labelled(&p, "New desktop"), Some("shell:workspace:new"));
    }

    #[test]
    fn the_files_header_searches_and_swaps_view_for_real() {
        let windows = [WindowView {
            id: 4,
            title: "Files".into(),
            kind: "files".into(),
            rect: Rect::new(0, 0, 900, 600),
            focused: true,
            document: "/home/alice".into(),
            ..Default::default()
        }];
        let ctx = context(&windows, true);
        let mut p = Painter::new(1024, 768);
        window_frame(&mut p, &ctx, &windows[0]);
        assert_eq!(
            hit_labelled(&p, "Search this folder"),
            Some("window:4:content:files-search")
        );
        assert_eq!(
            hit_labelled(&p, "Change view"),
            Some("window:4:content:files-view")
        );
        // The sidebar's hamburger and the header's ⋮ are gone, not greyed.
        for gone in [
            "Primary menu",
            "More options",
            "View options",
            "Search files",
        ] {
            assert!(labelled(&p, gone).is_none(), "{gone}");
        }
    }

    #[test]
    fn the_firefox_toolbar_saves_pages_and_bookmarks_them() {
        let loaded = [WindowView {
            id: 5,
            title: "Example — http://example.test/".into(),
            kind: "browser".into(),
            rect: Rect::new(60, 60, 880, 600),
            focused: true,
            document: "http://example.test/".into(),
            tabs: vec!["Example".into()],
            ..Default::default()
        }];
        let downloads = [crate::Download {
            name: "index.html".into(),
            path: "/home/alice/Downloads/index.html".into(),
            url: "http://example.test/".into(),
            bytes: 12,
        }];
        let mut ctx = context(&loaded, true);
        ctx.downloads = &downloads;
        let mut p = Painter::new(1024, 768);
        browser_chrome(&mut p, &ctx, &loaded[0]);
        assert_eq!(
            hit_labelled(&p, "Bookmark this page"),
            Some("shell:bookmark")
        );
        assert_eq!(
            hit_labelled(&p, "Save this page to Downloads"),
            Some("shell:download")
        );
        assert_eq!(hit_labelled(&p, "Application menu"), Some("shell:settings"));
        // Saved already: the same star takes it off the list and says so.
        ctx.bookmarked = true;
        let mut p = Painter::new(1024, 768);
        browser_chrome(&mut p, &ctx, &loaded[0]);
        assert_eq!(hit_labelled(&p, "Remove bookmark"), Some("shell:bookmark"));
    }
}
