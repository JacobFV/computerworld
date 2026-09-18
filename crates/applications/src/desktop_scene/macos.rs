//! macOS presentation: a translucent 28 px menu bar, unified window toolbars with
//! 12 px traffic lights, vibrancy panels and a floating glass Dock. Every control
//! that accepts input dispatches a real simulator action.
use super::shared::{Align, Painter, ShellContext, WindowView};
use cw_scene::{Color, Rect};

const INK: Color = Color::rgb(29, 29, 31);
const SECONDARY: Color = Color::rgb(110, 110, 115);
const TERTIARY: Color = Color::rgb(160, 160, 166);
const ACCENT: Color = Color::rgb(0, 122, 255);
const SEPARATOR: Color = Color(0, 0, 0, 28);
const SIDEBAR: Color = Color::rgb(232, 231, 234);
const TOOLBAR: Color = Color::rgb(246, 246, 247);
const TOOLBAR_INACTIVE: Color = Color::rgb(238, 238, 239);
/// Width of the Finder sidebar; shared with the Finder client area.
pub const FINDER_SIDEBAR: u32 = 172;
/// Every application this shell can present, in Launchpad order.
const APPS: [(&str, &str); 25] = [
    ("files", "Finder"),
    ("browser", "Safari"),
    ("mail", "Mail"),
    ("calendar", "Calendar"),
    ("notes", "Notes"),
    ("chat", "Messages"),
    ("contacts", "Contacts"),
    ("docs", "Pages"),
    ("spreadsheet", "Numbers"),
    ("editor", "TextEdit"),
    ("terminal", "Terminal"),
    ("code", "Visual Studio Code"),
    ("freecad", "FreeCAD"),
    ("excel", "Microsoft Excel"),
    ("database", "TablePlus"),
    ("kicad", "KiCad"),
    ("photos", "Photos"),
    ("preview", "Preview"),
    ("pixelmator", "Pixelmator Pro"),
    ("music", "Music"),
    ("maps", "Maps"),
    ("weather", "Weather"),
    ("calculator", "Calculator"),
    ("clock", "Clock"),
    ("settings", "System Settings"),
];
/// The Dock keeps the applications a Mac keeps on it, in Sequoia's default order
/// (Safari, Messages, Mail, Maps, Photos, Calendar, Contacts, Notes, Music, Pages),
/// then TextEdit and Terminal; Launchpad carries the rest.
const DOCK: [&str; 13] = [
    "browser", "chat", "mail", "maps", "photos", "calendar", "contacts", "notes", "music", "docs",
    "editor", "terminal", "code",
];

fn app_name(kind: &str) -> Option<&'static str> {
    APPS.iter().find(|(k, _)| *k == kind).map(|(_, n)| *n)
}
fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("Macintosh HD")
}

pub fn background(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.asset(Rect::new(0, 0, ctx.width, ctx.height), "wallpaper/macos");
    // Desktop items are real entry points, not unrelated artwork. The startup disk is
    // the one item a Mac shows here; applications live in the Dock, not on the desktop.
    if ctx.width >= 640 {
        let x = ctx.width as i32 - 108;
        for (i, (kind, name)) in [("files", "Macintosh HD")]
            .into_iter()
            .filter(|(kind, _)| ctx.installed(kind))
            .enumerate()
        {
            let y = 48 + i as i32 * 104;
            if ctx.selected(kind) {
                p.box_(Rect::new(x + 6, y - 6, 84, 92), Color(255, 255, 255, 48), 8);
            }
            p.platform_icon(
                Rect::new(x + 20, y, 56, 56),
                "macos",
                kind,
                &format!("shell:open:{kind}"),
                &format!("Open {name}"),
            );
            desktop_label(p, x - 8, y + 62, 112, name);
        }
    }
}

fn desktop_label(p: &mut Painter, x: i32, y: i32, width: u32, text: &str) {
    for (dx, dy, alpha) in [(0, 1, 150), (0, 2, 60), (1, 1, 50), (-1, 1, 50)] {
        p.label(
            x + dx,
            y + dy,
            width,
            text,
            12,
            Color(0, 0, 0, alpha),
            true,
            Align::Center,
        );
    }
    p.label(x, y, width, text, 12, Color::WHITE, true, Align::Center);
}

/// Toolbar glyph button with the rounded hover plate of AppKit toolbar items.
fn tool(p: &mut Painter, ctx: &ShellContext<'_>, r: Rect, symbol: &str, color: Color) {
    if ctx.hovered(r) {
        p.box_(r, Color(0, 0, 0, 18), 6);
    }
    let size = 16.min(r.height);
    p.symbol(
        symbol,
        r.x + (r.width as i32 - size as i32) / 2,
        r.y + (r.height as i32 - size as i32) / 2,
        size,
        color,
    );
}

pub fn window_frame(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let r = w.rect;
    let radius = super::corner_radius(ctx.theme, w.maximized);
    if !w.maximized {
        if w.focused {
            p.drop_shadow(r, radius, 34, 105, 16);
        } else {
            p.drop_shadow(r, radius, 18, 60, 7);
        }
    }
    let bar = if w.focused { TOOLBAR } else { TOOLBAR_INACTIVE };
    // Body, then the unified 42 px toolbar with only its top corners rounded.
    p.border(r, Color::rgb(252, 252, 253), radius, Color(0, 0, 0, 58));
    if w.kind == "code" {
        return crate::apps::code::title_bar(p, ctx, w);
    }
    let inner = Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(2), 41);
    p.box_(inner, bar, radius.saturating_sub(1));
    p.box_(Rect::new(inner.x, r.y + 21, inner.width, 21), bar, 0);
    let finder_sidebar = w.kind == "files" && r.width > 470;
    if finder_sidebar {
        // Finder's sidebar material runs to the top edge, beneath the traffic lights.
        p.box_(
            Rect::new(inner.x, inner.y, FINDER_SIDEBAR, 41),
            SIDEBAR,
            radius.saturating_sub(1),
        );
        p.box_(
            Rect::new(inner.x + 12, inner.y, FINDER_SIDEBAR - 12, 41),
            SIDEBAR,
            0,
        );
        p.box_(Rect::new(inner.x, r.y + 21, FINDER_SIDEBAR, 21), SIDEBAR, 0);
        p.vline(inner.x + FINDER_SIDEBAR as i32 - 1, inner.y, 41, SEPARATOR);
    } else if w.kind != "browser" {
        p.hline(inner.x, r.y + 41, inner.width, SEPARATOR);
    }
    p.region(
        Rect::new(r.x, r.y, r.width, 42),
        &w.action("drag"),
        &format!("Move {}", w.title),
    );
    let ink = if w.focused { INK } else { TERTIARY };
    match w.kind.as_str() {
        "browser" => {}
        "files" => {
            let x = r.x
                + if finder_sidebar {
                    FINDER_SIDEBAR as i32
                } else {
                    78
                };
            // Finder's chevrons are history, not hierarchy; the sidebar owns "up".
            for (i, (symbol, target, label, ready)) in [
                ("chevron-left", "files-back", "Back", w.can_go_back),
                (
                    "chevron-right",
                    "files-forward",
                    "Forward",
                    w.can_go_forward,
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let hit = Rect::new(x + 10 + i as i32 * 34, r.y + 8, 28, 26);
                tool(p, ctx, hit, symbol, if ready { ink } else { TERTIARY });
                // Nothing to go back to is a greyed control, not one that refuses.
                if ready {
                    p.region(hit, &w.action(&format!("content:{target}")), label);
                } else {
                    p.disabled(label);
                }
            }
            let right = r.x + r.width as i32;
            // Finder's toolbar, right to left: search, share, view. Search is a
            // magnifier until it is in use, then the field holding the query.
            let finding = w.editing || !w.query.is_empty();
            let search = if finding {
                Rect::new(right - 186, r.y + 8, 174, 26)
            } else {
                Rect::new(right - 42, r.y + 8, 30, 26)
            };
            let view = Rect::new(search.x - 72, r.y + 9, 62, 24);
            let share = Rect::new(view.x - 36, r.y + 8, 28, 26);
            let title_width = (share.x - x - 84).max(40) as u32;
            p.strong(
                x + 74,
                r.y + 12,
                title_width,
                if w.caption.is_empty() {
                    basename(&w.document)
                } else {
                    &w.caption
                },
                15,
                ink,
            );
            if r.width > 620 {
                // One control, not two segments: `files-view` swaps the tab between its
                // icon and list layouts, and there is no id that sets an exact one. The
                // window view carries no view mode either, so neither half is lit.
                if ctx.hovered(view) {
                    p.box_(view, Color(0, 0, 0, 18), 6);
                }
                p.border(view, Color(0, 0, 0, 10), 6, Color(0, 0, 0, 20));
                p.symbol("grid-view", view.x + 8, r.y + 14, 14, SECONDARY);
                p.symbol("list-view", view.x + 38, r.y + 14, 14, ink);
                p.region(view, &w.action("content:files-view"), "Icon or list view");
                // Share hands the selected item to Messages, or Mail without it; with
                // nothing selected or nothing to receive it, it is greyed.
                let via = ["chat", "mail"]
                    .into_iter()
                    .find(|kind| ctx.installed(kind));
                match via.filter(|_| !w.selection.is_empty()) {
                    Some(kind) => {
                        tool(p, ctx, share, "share", SECONDARY);
                        p.region(
                            share,
                            &format!("shell:share:{kind}"),
                            if kind == "chat" {
                                "Share with Messages"
                            } else {
                                "Share with Mail"
                            },
                        );
                    }
                    None => {
                        p.symbol("share", share.x + 6, share.y + 5, 16, TERTIARY);
                        p.disabled("Share");
                    }
                }
                // Tags, Group and the action menu have no model and are left out; the
                // Go menu carries Home, Computer and the other places.
                if finding {
                    p.border(search, Color::rgb(255, 255, 255), 7, Color(0, 0, 0, 30));
                    p.region(search, &w.action("content:files-search"), "Search");
                    p.symbol("search", search.x + 8, search.y + 6, 14, SECONDARY);
                    let shown = if w.query.is_empty() {
                        "Search"
                    } else {
                        w.query.as_str()
                    };
                    let used = p.left(
                        search.x + 28,
                        search.y + 5,
                        search.width - 52,
                        shown,
                        13,
                        if w.query.is_empty() { TERTIARY } else { INK },
                    );
                    if w.editing {
                        let caret = if w.query.is_empty() { 0 } else { used as i32 };
                        p.box_(
                            Rect::new(search.x + 29 + caret, search.y + 6, 1, 15),
                            ACCENT,
                            0,
                        );
                    }
                    let clear =
                        Rect::new(search.x + search.width as i32 - 22, search.y + 5, 16, 16);
                    p.button(
                        clear,
                        Color::TRANSPARENT,
                        8,
                        &w.action("content:files-search-clear"),
                        "Clear search",
                    );
                    p.circle(clear.x + 8, clear.y + 8, 7, Color(0, 0, 0, 60));
                    p.symbol("close", clear.x + 4, clear.y + 4, 8, Color::WHITE);
                } else {
                    tool(p, ctx, search, "search", SECONDARY);
                    p.region(search, &w.action("content:files-search"), "Search");
                }
            }
        }
        kind => {
            let name = match kind {
                // Terminal titles a window with the user and the shell it runs.
                "terminal" => w
                    .shell_identity()
                    .map(|(user, _, _)| format!("{user} — -zsh"))
                    .unwrap_or_else(|| "Terminal".into()),
                "editor" if w.document.is_empty() => "Untitled".to_owned(),
                "editor" => basename(&w.document).to_owned(),
                _ => w.title.clone(),
            };
            let edited = if w.modified { " — Edited" } else { "" };
            let width = r.width.saturating_sub(180);
            let measured = p.measure(&name, 13, true) + p.measure(edited, 13, false);
            let mut x = r.x + 90 + (width.saturating_sub(measured) / 2) as i32;
            if kind == "editor" {
                // The document's own icon beside its name, and nothing more: a proxy
                // icon exists to be dragged and nothing here consumes a document drag.
                // Sharing the document is `shell:share`, which needs no icon to drag.
                p.asset(Rect::new(x - 22, r.y + 12, 17, 17), "icon/macos/docs");
            }
            x += p.strong(x, r.y + 13, width, &name, 13, ink) as i32;
            if w.modified {
                p.left(x, r.y + 13, 80, edited, 13, SECONDARY);
            }
        }
    }
    traffic_lights(p, ctx, w, 0);
}

/// The close, minimise and zoom buttons. `lift` raises them for a title bar shorter
/// than the standard 42 px toolbar, such as Visual Studio Code's 35 px one.
pub(crate) fn traffic_lights(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView, lift: i32) {
    let mut r = w.rect;
    r.y -= lift;
    let group = Rect::new(r.x + 10, r.y + 9, 72, 24);
    let hovered = ctx.hovered(group);
    for (i, (action, label, color, edge, glyph)) in [
        (
            "close",
            "Close window",
            Color::rgb(255, 95, 87),
            Color::rgb(224, 68, 62),
            Color::rgb(77, 0, 0),
        ),
        (
            "minimize",
            "Minimize window",
            Color::rgb(254, 188, 46),
            Color::rgb(222, 161, 35),
            Color::rgb(153, 87, 0),
        ),
        (
            "maximize",
            "Zoom window",
            Color::rgb(40, 200, 64),
            Color::rgb(26, 171, 41),
            Color::rgb(0, 101, 0),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let x = r.x + 20 + i as i32 * 20;
        let y = r.y + 15;
        let lit = w.focused || hovered;
        p.border(
            Rect::new(x - 6, y, 12, 12),
            if lit {
                color
            } else {
                Color::rgb(206, 205, 207)
            },
            6,
            if lit { edge } else { Color::rgb(186, 185, 187) },
        );
        if hovered {
            let cy = y + 6;
            match action {
                "close" => {
                    p.line(vec![(x - 2, cy - 2), (x + 2, cy + 2)], glyph, 1);
                    p.line(vec![(x - 2, cy + 2), (x + 2, cy - 2)], glyph, 1);
                }
                "minimize" => p.line(vec![(x - 3, cy), (x + 3, cy)], glyph, 1),
                _ => {
                    p.path(
                        vec![(x - 3, cy - 3), (x + 2, cy - 3), (x - 3, cy + 2)],
                        glyph,
                    );
                    p.path(
                        vec![(x + 3, cy + 3), (x - 2, cy + 3), (x + 3, cy - 2)],
                        glyph,
                    );
                }
            }
        }
        p.region(Rect::new(x - 9, r.y + 11, 19, 20), &w.action(action), label);
    }
}

/// Safari: a unified toolbar holding the traffic lights and address field, above a
/// tab bar. Geometry matches `window_content_rect_for_kind(.., "browser")`.
pub fn browser_chrome(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let r = w.rect;
    let ink = if w.focused { INK } else { TERTIARY };
    let soft = if w.focused { SECONDARY } else { TERTIARY };
    let left = r.x + 1;
    let width = r.width.saturating_sub(2);
    let right = left + width as i32;
    let wide = width > 560;
    let mut x = left + 86;
    if wide {
        // The sidebar has no surface of its own — nothing opens a pane beside the page —
        // so it shows the saved bookmarks where macOS keeps "Show Bookmarks Sidebar":
        // the View menu, which really lists them and really navigates to one.
        let hit = Rect::new(x - 6, r.y + 8, 28, 26);
        tool(p, ctx, hit, "sidebar", soft);
        p.region(hit, "shell:panel:view", "Bookmarks");
        x += 38;
    }
    // Safari greys history it does not have, exactly as Finder's chevrons do.
    for (symbol, action, label, ready) in [
        ("chevron-left", "back", "Back", w.can_go_back),
        ("chevron-right", "forward", "Forward", w.can_go_forward),
    ] {
        let hit = Rect::new(x, r.y + 8, 28, 26);
        if ready {
            tool(p, ctx, hit, symbol, ink);
            p.region(hit, &w.action(&format!("content:shell:{action}")), label);
        } else {
            // Same glyph in the same place as `tool`, without the hover plate.
            p.symbol(symbol, hit.x + 6, hit.y + 5, 16, TERTIARY);
            p.disabled(label);
        }
        x += 30;
    }
    let trailing = if wide { 118 } else { 44 };
    let room = (right - trailing - x - 12).max(60);
    let field_width = room.min(560) as u32;
    let field = Rect::new(
        x + 12 + (room - field_width as i32) / 2,
        r.y + 8,
        field_width,
        27,
    );
    p.button(
        field,
        if w.editing {
            Color::WHITE
        } else {
            Color(0, 0, 0, 16)
        },
        7,
        &w.action("content:shell:address"),
        "Address and search",
    );
    if w.editing {
        p.border(
            Rect::new(field.x - 2, field.y - 2, field.width + 4, field.height + 4),
            Color::TRANSPARENT,
            9,
            Color(0, 122, 255, 120),
        );
    }
    let typed = w
        .title
        .split_once(" — ")
        .map_or(w.title.as_str(), |(_, address)| address);
    let shown = if w.editing {
        typed.to_owned()
    } else {
        // Safari shows the bare host until the field is focused.
        typed
            .split_once("://")
            .map_or(typed, |(_, rest)| rest)
            .split('/')
            .next()
            .unwrap_or(typed)
            .to_owned()
    };
    let inner = field.width.saturating_sub(64);
    if shown.is_empty() {
        p.symbol("search", field.x + 9, field.y + 7, 13, TERTIARY);
        p.left(
            field.x + 28,
            field.y + 5,
            inner,
            "Search or enter website name",
            13,
            TERTIARY,
        );
    } else if w.editing {
        let end = p.left(field.x + 10, field.y + 5, inner + 20, &shown, 13, INK);
        p.box_(
            Rect::new(field.x + 11 + end as i32, field.y + 6, 1, 15),
            ACCENT,
            0,
        );
    } else {
        let text = p.measure(&shown, 13, false).min(inner);
        let start = field.x + (field.width as i32 - text as i32) / 2;
        p.symbol("lock", start - 16, field.y + 8, 11, soft);
        p.left(start, field.y + 5, inner, &shown, 13, ink);
    }
    let reload = Rect::new(field.x + field.width as i32 - 27, field.y + 1, 25, 25);
    if w.document.is_empty() {
        // An empty tab has no request to repeat, so the handler would refuse a reload.
        p.symbol("reload", reload.x + 5, reload.y + 5, 15, TERTIARY);
        p.disabled("Reload");
    } else {
        tool(p, ctx, reload, "reload", soft);
        p.region(reload, &w.action("content:shell:reload"), "Reload");
    }
    // The new-tab button keeps its place in a wide toolbar and takes the tab
    // overview's slot when there is no room for both.
    let plus = Rect::new(if wide { right - 72 } else { right - 40 }, r.y + 8, 28, 26);
    if wide {
        // Sharing really hands the page to a messaging application and posts a notice.
        // Messages first, Mail behind it; with neither installed nothing can receive it.
        let share = Rect::new(right - 106, r.y + 8, 28, 26);
        let via = ["chat", "mail"]
            .into_iter()
            .find(|kind| ctx.installed(kind))
            .filter(|_| !w.document.is_empty());
        match via {
            Some(kind) => {
                tool(p, ctx, share, "share", soft);
                p.region(
                    share,
                    &format!("shell:share:{kind}"),
                    if kind == "chat" {
                        "Share with Messages"
                    } else {
                        "Share with Mail"
                    },
                );
            }
            None => {
                p.symbol("share", share.x + 6, share.y + 5, 16, TERTIARY);
                p.disabled("Share");
            }
        }
    }
    tool(p, ctx, plus, "plus", soft);
    p.region(plus, &w.tab_new(), "New tab");
    if wide {
        // The tab overview is gone rather than greyed: it needs a surface of its own and
        // the strip immediately below already switches, closes and opens every tab. Its
        // slot carries the bookmark control, which does have real state behind it.
        let star = Rect::new(right - 40, r.y + 8, 28, 26);
        let saved = ctx.bookmarked && w.focused;
        if w.document.is_empty() {
            p.symbol("star-outline", star.x + 6, star.y + 5, 16, TERTIARY);
            p.disabled("Add bookmark");
        } else {
            tool(
                p,
                ctx,
                star,
                if saved { "star" } else { "star-outline" },
                if saved { ACCENT } else { soft },
            );
            p.region(
                star,
                "shell:bookmark",
                if saved {
                    "Remove bookmark"
                } else {
                    "Add bookmark"
                },
            );
        }
    }
    // Tab bar driven by the browser session's real tab list.
    let strip = Rect::new(left, r.y + 42, width, 40);
    p.box_(
        strip,
        if w.focused {
            Color::rgb(232, 232, 233)
        } else {
            Color::rgb(236, 236, 237)
        },
        0,
    );
    p.hline(left, r.y + 42, width, SEPARATOR);
    p.hline(left, r.y + 81, width, SEPARATOR);
    let caption = if w.caption.is_empty() {
        "Start Page"
    } else {
        &w.caption
    };
    // A browser window always owns at least one tab; a bare preview owns none, and
    // then the strip only mirrors the page instead of pretending to switch tabs.
    let live = !w.tabs.is_empty();
    let labels: Vec<&str> = if live {
        w.tabs.iter().map(String::as_str).collect()
    } else {
        vec![caption]
    };
    let room = width.saturating_sub(12);
    let count = labels.len() as u32;
    // Two pixels of gutter per tab, so the last one still fits inside the strip.
    let tab_width = if count == 1 {
        room
    } else {
        ((room + 2) / count)
            .saturating_sub(2)
            .clamp(96, 480)
            .min(room)
    };
    let mut tx = left + 6;
    for (index, label) in labels.iter().enumerate() {
        if tx + tab_width as i32 > left + width as i32 - 6 {
            break;
        }
        let tab = Rect::new(tx, r.y + 47, tab_width, 30);
        let selected = index == w.active_tab;
        let fill = match (selected, w.focused) {
            (false, _) => Color::TRANSPARENT,
            (true, true) => TOOLBAR,
            (true, false) => TOOLBAR_INACTIVE,
        };
        if live {
            p.button(tab, fill, 7, &w.tab_select(index), label);
        } else {
            p.box_(tab, fill, 7);
            p.disabled(label);
        }
        if selected {
            p.border(tab, Color::TRANSPARENT, 7, Color(0, 0, 0, 22));
        }
        let closable = live && tab_width >= 140;
        let inner = tab_width.saturating_sub(if closable { 80 } else { 52 });
        let text = p.measure(label, 12, false).min(inner);
        let cx = tab.x + (tab_width as i32 - text as i32) / 2;
        p.symbol("globe", cx - 19, tab.y + 8, 13, soft);
        p.left(
            cx,
            tab.y + 7,
            inner,
            label,
            12,
            if selected { ink } else { soft },
        );
        if closable {
            let close = Rect::new(tab.x + tab_width as i32 - 26, tab.y + 5, 20, 20);
            p.button(
                close,
                Color(0, 0, 0, 14),
                5,
                &w.tab_close(index),
                "Close tab",
            );
            p.symbol("close", close.x + 5, close.y + 5, 10, soft);
        }
        tx += tab_width as i32 + 2;
    }
}

pub fn chrome(p: &mut Painter, ctx: &ShellContext<'_>) {
    if !ctx.awake() {
        // The shell is drawn last, so this really covers the desktop beneath it.
        return locked(p, ctx);
    }
    if ctx.launcher_open {
        launchpad(p, ctx);
    }
    menu_bar(p, ctx);
    dock(p, ctx);
    if let Some(panel) = ctx.panel {
        panel_surface(p, ctx, panel);
    }
}

/// Lock and sleep screens. `DesktopState::screen` really moves between these and the
/// only way back is `shell:power:wake`; there is no credential model, so the lock
/// screen never pretends to ask for a password.
fn locked(p: &mut Painter, ctx: &ShellContext<'_>) {
    let full = Rect::new(0, 0, ctx.width, ctx.height);
    let cx = ctx.width as i32 / 2;
    let middle = ctx.height as i32 / 2;
    if ctx.screen == crate::ScreenState::Off {
        p.box_(full, Color::rgb(4, 4, 6), 0);
        p.region(full, "shell:power:wake", "Turn on the display");
        p.center(
            0,
            middle - 34,
            ctx.width,
            "Display Off",
            15,
            Color::rgb(86, 86, 92),
        );
        let power = Rect::new(cx - 66, middle, 132, 34);
        p.button(
            power,
            Color(255, 255, 255, 30),
            17,
            "shell:power:wake",
            "Turn on",
        );
        p.symbol("power", power.x + 30, power.y + 9, 16, Color::WHITE);
        p.left(power.x + 56, power.y + 9, 80, "Turn On", 13, Color::WHITE);
        return;
    }
    p.asset(full, "wallpaper/macos");
    p.glass(full, 0, 40, Color(10, 12, 22, 150), None);
    // Clicking anywhere unlocks, the way a locked Mac wakes on any input.
    p.region(full, "shell:power:wake", "Unlock");
    let date = ctx.date();
    p.label(
        0,
        (middle / 3).max(40),
        ctx.width,
        &format!("{} {} {}", date.weekday_name(), date.month_name(), date.day),
        15,
        Color(255, 255, 255, 205),
        false,
        Align::Center,
    );
    p.label(
        0,
        (middle / 3).max(40) + 26,
        ctx.width,
        &ctx.time12(),
        54,
        Color::WHITE,
        true,
        Align::Center,
    );
    let base = (ctx.height as i32 * 3 / 5).max(middle + 20);
    p.circle(cx, base + 30, 30, Color(255, 255, 255, 60));
    p.symbol("person", cx - 15, base + 15, 30, Color(255, 255, 255, 220));
    let unlock = Rect::new(cx - 76, base + 76, 152, 32);
    p.button(
        unlock,
        Color(255, 255, 255, 52),
        16,
        "shell:power:wake",
        "Unlock",
    );
    p.center(unlock.x, unlock.y + 8, 152, "Unlock", 13, Color::WHITE);
    for (i, (label, action)) in [
        ("Shut Down", "shell:power:off"),
        ("Restart", "shell:power:restart"),
    ]
    .into_iter()
    .enumerate()
    {
        let button = Rect::new(cx - 76 + i as i32 * 80, base + 120, 72, 28);
        p.button(button, Color(255, 255, 255, 30), 14, action, label);
        p.center(button.x, button.y + 7, 72, label, 12, Color::WHITE);
    }
}

const MENUS: [&str; 5] = ["File", "Edit", "View", "Window", "Help"];
/// Finder's menus: the same, with its Go menu between View and Window.
const FINDER_MENUS: [&str; 6] = ["File", "Edit", "View", "Go", "Window", "Help"];
fn front_app(ctx: &ShellContext<'_>) -> &'static str {
    ctx.windows
        .iter()
        .find(|w| w.focused && !w.minimized)
        .and_then(|w| app_name(&w.kind))
        .unwrap_or("Finder")
}
/// Left edge and width of each pull-down title, shared by the bar and its menus.
fn menu_layout(p: &Painter, ctx: &ShellContext<'_>) -> Vec<(&'static str, i32, u32)> {
    let mut x = 46 + p.measure(front_app(ctx), 13, true) as i32 + 20;
    let mut out = Vec::new();
    let menus: &[&'static str] = if front_app(ctx) == "Finder" {
        &FINDER_MENUS
    } else {
        &MENUS
    };
    for &label in menus {
        let width = p.measure(label, 13, false) + 20;
        if x + width as i32 > ctx.width as i32 - 300 {
            break;
        }
        out.push((label, x, width));
        x += width as i32;
    }
    out
}

fn menu_bar(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.glass(
        Rect::new(0, 0, ctx.width, 28),
        0,
        24,
        Color(250, 248, 252, 150),
        None,
    );
    p.hline(0, 27, ctx.width, Color(0, 0, 0, 22));
    let open = |name: &str| ctx.panel == Some(name);
    let plate = Color(0, 0, 0, 28);
    let apple = Rect::new(8, 2, 34, 24);
    if ctx.hovered(apple) || open("apple") {
        p.box_(apple, plate, 5);
    }
    p.symbol("fruit", 17, 5, 17, INK);
    p.region(Rect::new(6, 0, 38, 28), "shell:panel:apple", "Apple menu");
    // The application menu: this shell's Window menu is the real front-window menu.
    let name = front_app(ctx);
    let name_hit = Rect::new(42, 2, p.measure(name, 13, true) + 16, 24);
    if ctx.hovered(name_hit) {
        p.box_(name_hit, plate, 5);
    }
    p.strong(46, 6, 160, name, 13, INK);
    p.region(
        Rect::new(name_hit.x, 0, name_hit.width, 28),
        "shell:panel:window",
        &format!("{name} menu"),
    );
    for (label, x, width) in menu_layout(p, ctx) {
        let hit = Rect::new(x, 2, width, 24);
        if ctx.hovered(hit) || open(&label.to_lowercase()) {
            p.box_(hit, plate, 5);
        }
        p.left(x + 10, 6, width, label, 13, INK);
        p.region(
            Rect::new(x, 0, width, 28),
            &format!("shell:panel:{}", label.to_lowercase()),
            &format!("{label} menu"),
        );
    }
    let date = ctx.date();
    let clock = format!(
        "{} {} {}  {}",
        &date.weekday_name()[..3],
        &date.month_name()[..3],
        date.day,
        ctx.time12()
    );
    let right = ctx.width as i32;
    let clock_width = p.measure(&clock, 13, false);
    let clock_x = right - 14 - clock_width as i32;
    let clock_hit = Rect::new(clock_x - 8, 2, clock_width + 16, 24);
    if ctx.hovered(clock_hit) || open("notifications") {
        p.box_(clock_hit, plate, 5);
    }
    p.left(clock_x, 6, clock_width + 4, &clock, 13, INK);
    p.region(
        Rect::new(clock_x - 8, 0, clock_width + 22, 28),
        "shell:panel:notifications",
        "Notification Center",
    );
    if ctx.width > 720 {
        // Status glyphs read the real switches back and open Control Center, which
        // is where the switch they report actually lives.
        let flight = ctx.switch("airplane_mode");
        let wifi = ctx.switch("wifi") && !flight;
        let saver = ctx.switch("battery_saver");
        let mut x = clock_x - 12;
        for (i, (symbol, size, action, tint, label)) in [
            (
                "control-center",
                16,
                "shell:panel:control",
                INK,
                "Control Center".to_owned(),
            ),
            (
                "search",
                15,
                "shell:panel:spotlight",
                INK,
                "Spotlight Search".to_owned(),
            ),
            (
                if flight { "airplane" } else { "wifi" },
                17,
                "shell:panel:control",
                if wifi || flight { INK } else { TERTIARY },
                if flight {
                    "Airplane Mode on".to_owned()
                } else {
                    format!("Wi-Fi {}", if wifi { "on" } else { "off" })
                },
            ),
            (
                "battery",
                25,
                "shell:panel:control",
                if saver { Color::rgb(232, 155, 20) } else { INK },
                if saver {
                    "Battery, Low Power Mode".to_owned()
                } else {
                    "Battery".to_owned()
                },
            ),
        ]
        .into_iter()
        .enumerate()
        {
            x -= size + 18;
            let hit = Rect::new(x - 7, 2, size as u32 + 14, 24);
            // Only the two glyphs that own a panel show that panel's open state.
            let active = i < 2 && ctx.panel == Some(if i == 0 { "quick" } else { "search" });
            if ctx.hovered(hit) || active {
                p.box_(hit, plate, 5);
            }
            p.symbol(symbol, x, 14 - size / 2, size as u32, tint);
            p.region(Rect::new(x - 7, 0, size as u32 + 14, 28), action, &label);
        }
    }
}

fn dock(p: &mut Painter, ctx: &ShellContext<'_>) {
    // (kind, label, action override). Launchpad and Settings are shell surfaces.
    let mut items: Vec<(&str, &str, Option<&str>)> = vec![("files", "Finder", None)];
    items.push(("launcher", "Launchpad", Some("shell:launcher")));
    for (kind, label) in DOCK
        .iter()
        .filter_map(|kind| APPS.iter().find(|(k, _)| k == kind))
    {
        items.push((kind, label, None));
    }
    items.push(("settings", "System Settings", Some("shell:settings")));
    items.retain(|(kind, _, action)| action.is_some() || ctx.installed(kind));
    let icon: u32 = if ctx.width < 700 { 40 } else { 50 };
    let gap = 6;
    let pad = 7;
    let trash = ctx.width >= 700;
    let tray_width =
        |count: u32| count * icon + (count - 1) * gap + pad * 2 + if trash { icon + 21 } else { 0 };
    // A narrow display drops the tail of the Dock rather than painting a tray wider
    // than the screen; Launchpad still holds every installed application.
    while items.len() > 3 && tray_width(items.len() as u32) > ctx.width.saturating_sub(24) {
        let Some(last) = items
            .iter()
            .rposition(|(_, _, action)| action.is_none())
            .filter(|index| *index > 0)
        else {
            break;
        };
        items.remove(last);
    }
    let count = items.len() as u32;
    let width = tray_width(count);
    let height = icon + 15;
    let x = (ctx.width.saturating_sub(width) / 2) as i32;
    let y = ctx.height as i32 - height as i32 - 6;
    let tray = Rect::new(x, y, width, height);
    p.drop_shadow(tray, 19, 22, 70, 8);
    p.glass(
        tray,
        19,
        30,
        Color(246, 246, 250, 92),
        Some(Color(255, 255, 255, 96)),
    );
    p.border(
        Rect::new(x - 1, y - 1, width + 2, height + 2),
        Color::TRANSPARENT,
        20,
        Color(0, 0, 0, 36),
    );
    let mut ix = x + pad as i32;
    for (kind, label, shell_action) in &items {
        let existing = ctx.windows.iter().rev().find(|w| w.kind == *kind);
        let action = shell_action.map(str::to_owned).unwrap_or_else(|| {
            existing
                .map(|w| w.action("focus"))
                .unwrap_or_else(|| format!("shell:launch:{kind}"))
        });
        let slot = Rect::new(ix - 3, y, icon + 6, height);
        let hovered = ctx.hovered(slot);
        // Magnification: the hovered icon grows upward from the shelf.
        let grow = if hovered { icon / 5 } else { 0 };
        p.platform_icon(
            Rect::new(
                ix - grow as i32 / 2,
                y + 5 - grow as i32,
                icon + grow,
                icon + grow,
            ),
            "macos",
            kind,
            &action,
            label,
        );
        p.region(slot, &action, label);
        if hovered {
            tooltip(p, ix + icon as i32 / 2, y - grow as i32 - 12, label);
        }
        if existing.is_some() {
            p.circle(
                ix + icon as i32 / 2,
                y + height as i32 - 5,
                2,
                Color(0, 0, 0, 170),
            );
        }
        ix += (icon + gap) as i32;
    }
    if trash {
        p.box_(
            Rect::new(ix + 3, y + 9, 1, height - 18),
            Color(0, 0, 0, 50),
            0,
        );
        // Deleted files really land in ~/.local/share/Trash/files, and this opens it.
        let slot = Rect::new(ix + 11, y, icon + 6, height);
        let grow = if ctx.hovered(slot) { icon / 5 } else { 0 };
        p.asset(
            Rect::new(
                ix + 14 - grow as i32 / 2,
                y + 5 - grow as i32,
                icon + grow,
                icon + grow,
            ),
            "icon/macos/trash",
        );
        p.region(slot, "shell:trash", "Trash");
        if grow > 0 {
            tooltip(p, ix + 14 + icon as i32 / 2, y - grow as i32 - 12, "Trash");
        }
    }
}

fn tooltip(p: &mut Painter, centre: i32, bottom: i32, text: &str) {
    let width = p.measure(text, 13, false) + 24;
    let r = Rect::new(centre - width as i32 / 2, bottom - 28, width, 28);
    p.drop_shadow(r, 7, 10, 60, 3);
    p.glass(
        r,
        7,
        16,
        Color(236, 236, 240, 205),
        Some(Color(0, 0, 0, 40)),
    );
    p.path(
        vec![
            (centre - 7, bottom - 1),
            (centre + 7, bottom - 1),
            (centre, bottom + 6),
        ],
        Color(236, 236, 240, 235),
    );
    p.center(r.x, r.y + 6, width, text, 13, INK);
}

fn launchpad(p: &mut Painter, ctx: &ShellContext<'_>) {
    let full = Rect::new(0, 0, ctx.width, ctx.height);
    p.glass(full, 0, 40, Color(40, 44, 60, 90), None);
    p.region(full, "shell:launcher", "Close Launchpad");
    let field = Rect::new(ctx.width as i32 / 2 - 120, 52, 240, 30);
    p.border(field, Color(255, 255, 255, 40), 8, Color(255, 255, 255, 70));
    p.region(field, "shell:search", "Search applications");
    let hint = p.measure("Search", 13, false) as i32;
    p.symbol(
        "search",
        ctx.width as i32 / 2 - hint / 2 - 20,
        60,
        13,
        Color(255, 255, 255, 190),
    );
    p.left(
        ctx.width as i32 / 2 - hint / 2,
        58,
        120,
        "Search",
        13,
        Color(255, 255, 255, 190),
    );
    let apps: Vec<_> = APPS
        .iter()
        .filter(|(kind, _)| ctx.installed(kind))
        .collect();
    let columns = (ctx.width.saturating_sub(120) / 150).clamp(3, 7);
    let cell = (ctx.width.saturating_sub(120) / columns).min(168);
    let icon = if ctx.width < 800 { 64 } else { 84 };
    let left = (ctx.width - columns * cell) as i32 / 2;
    for (i, (kind, label)) in apps.iter().enumerate() {
        let cx = left + (i as u32 % columns * cell) as i32;
        let cy = 126 + (i as u32 / columns) as i32 * (icon as i32 + 62);
        if cy + icon as i32 + 40 > ctx.height as i32 - 90 {
            break;
        }
        let action = format!("shell:launch:{kind}");
        let slot = Rect::new(cx, cy - 8, cell, icon + 44);
        if ctx.hovered(slot) {
            p.box_(slot, Color(255, 255, 255, 26), 16);
        }
        p.region(slot, &action, label);
        p.platform_icon(
            Rect::new(cx + (cell - icon) as i32 / 2, cy, icon, icon),
            "macos",
            kind,
            &action,
            label,
        );
        p.label(
            cx,
            cy + icon as i32 + 9,
            cell,
            label,
            13,
            Color::WHITE,
            false,
            Align::Center,
        );
    }
    // Launchpad holds a single page, so it carries no page indicator at all: a dot that
    // can never be a second page is a picture of a control, not a control.
}

/// Click-away layer every transient surface needs: the menu bar stays live above it
/// so the pull-downs still track the pointer, exactly as macOS does.
fn dismiss_layer(p: &mut Painter, ctx: &ShellContext<'_>, label: &str) {
    p.region(
        Rect::new(0, 28, ctx.width, ctx.height.saturating_sub(28)),
        "shell:dismiss",
        label,
    );
}

/// Standard popover material shared by menus and system panels.
fn popover(p: &mut Painter, r: Rect, radius: u32) {
    p.drop_shadow(r, radius, 26, 80, 10);
    p.glass(
        r,
        radius,
        30,
        Color(246, 246, 248, 200),
        Some(Color(255, 255, 255, 120)),
    );
    p.border(
        Rect::new(r.x - 1, r.y - 1, r.width + 2, r.height + 2),
        Color::TRANSPARENT,
        radius + 1,
        Color(0, 0, 0, 48),
    );
    // Absorb clicks so they never reach a window beneath the panel.
    p.region(r, "shell:noop", "Panel");
}

fn panel_surface(p: &mut Painter, ctx: &ShellContext<'_>, panel: &str) {
    match panel {
        "spotlight" | "search" => spotlight(p, ctx),
        "settings" => settings(p, ctx),
        "quick" | "control" => control_center(p, ctx),
        "notifications" | "calendar" => notification_center(p, ctx),
        "overview" => mission_control(p, ctx),
        _ => menu(p, ctx, panel),
    }
}

fn spotlight(p: &mut Painter, ctx: &ShellContext<'_>) {
    dismiss_layer(p, ctx, "Close Spotlight");
    let width = ctx.width.saturating_sub(48).min(640);
    let x = (ctx.width - width) as i32 / 2;
    let y = (ctx.height / 5).max(45) as i32;
    let query = ctx.search.to_lowercase();
    let apps: Vec<_> = APPS
        .iter()
        .filter(|(k, n)| {
            ctx.installed(k)
                && (query.is_empty() || n.to_lowercase().contains(&query) || k.contains(&query))
        })
        .take(6)
        .collect();
    let h = 56
        + if apps.is_empty() {
            0
        } else {
            apps.len() as u32 * 40 + 38
        };
    let r = Rect::new(x, y, width, h);
    popover(p, r, 14);
    p.symbol("search", x + 18, y + 17, 22, SECONDARY);
    if ctx.search.is_empty() {
        p.left(x + 52, y + 14, width - 80, "Spotlight Search", 22, TERTIARY);
    } else {
        let end = p.left(x + 52, y + 14, width - 80, ctx.search, 22, INK);
        p.box_(Rect::new(x + 54 + end as i32, y + 15, 2, 26), ACCENT, 0);
    }
    // `shell:search` keeps the panel open and keeps whatever has been typed.
    p.region(
        Rect::new(x, y, width, 56),
        "shell:search",
        "Spotlight search field",
    );
    if apps.is_empty() {
        return;
    }
    p.hline(x + 1, y + 56, width - 2, SEPARATOR);
    p.strong(x + 18, y + 66, 200, "Applications", 11, SECONDARY);
    for (i, (kind, name)) in apps.iter().enumerate() {
        let ay = y + 88 + i as i32 * 40;
        let row = Rect::new(x + 8, ay, width - 16, 38);
        let selected = i == 0 || ctx.hovered(row);
        if selected {
            p.box_(row, if i == 0 { ACCENT } else { Color(0, 0, 0, 16) }, 8);
        }
        p.asset(
            Rect::new(x + 18, ay + 5, 28, 28),
            &format!("icon/macos/{kind}"),
        );
        p.left(
            x + 58,
            ay + 10,
            width - 200,
            name,
            14,
            if i == 0 { Color::WHITE } else { INK },
        );
        p.right(
            x + width as i32 - 150,
            ay + 11,
            130,
            "Application",
            12,
            if i == 0 {
                Color(255, 255, 255, 200)
            } else {
                TERTIARY
            },
        );
        p.region(row, &format!("shell:launch:{kind}"), name);
    }
}

fn settings(p: &mut Painter, ctx: &ShellContext<'_>) {
    dismiss_layer(p, ctx, "Dismiss System Settings");
    let width = ctx.width.saturating_sub(48).min(520);
    let x = (ctx.width - width) as i32 / 2;
    let y = (ctx.height as i32 - 340).max(80) / 2;
    let r = Rect::new(x, y, width, 320);
    p.drop_shadow(r, 12, 34, 105, 16);
    p.border(r, Color::rgb(246, 246, 247), 12, Color(0, 0, 0, 58));
    p.region(r, "shell:noop", "System Settings");
    // One circle, not three. This is a sheet, not a window: it has no window id to
    // minimize, no Dock slot to minimize into, and its size follows the display, so
    // there is nothing to zoom. macOS draws no traffic lights on a sheet either; the
    // two dead ones were the fiction, and closing is the one thing it can really do.
    p.circle(x + 20, y + 21, 6, Color::rgb(255, 95, 87));
    p.region(
        Rect::new(x + 10, y + 11, 20, 20),
        "shell:dismiss",
        "Close System Settings",
    );
    p.strong_center(x, y + 13, width, "System Settings", 13, INK);
    p.asset(
        Rect::new(x + width as i32 / 2 - 36, y + 54, 72, 72),
        "icon/macos/settings",
    );
    p.strong_center(x, y + 136, width, "macOS", 22, INK);
    p.center(x, y + 166, width, "Version 15.0", 12, SECONDARY);
    let card = Rect::new(x + 24, y + 196, width - 48, 64);
    p.border(card, Color::WHITE, 8, Color(0, 0, 0, 18));
    // Each row leads to the surface that really owns what it reports.
    for (i, (key, value, action, label)) in [
        (
            "Display",
            format!("{} × {}", ctx.width, ctx.height),
            "shell:panel:control",
            "Display settings",
        ),
        (
            "Open windows",
            ctx.windows.len().to_string(),
            "shell:panel:overview",
            "Show all windows",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let row = Rect::new(card.x + 1, card.y + 1 + i as i32 * 31, card.width - 2, 30);
        if ctx.hovered(row) {
            p.box_(row, Color(0, 0, 0, 12), 6);
        }
        let ry = card.y + 9 + i as i32 * 31;
        p.left(card.x + 14, ry, 200, key, 13, INK);
        p.right(
            card.x + card.width as i32 - 214,
            ry,
            200,
            &value,
            13,
            SECONDARY,
        );
        if i == 0 {
            p.hline(card.x + 14, card.y + 32, card.width - 14, SEPARATOR);
        }
        p.region(row, action, label);
    }
    let launch = Rect::new(x + 24, y + 274, 150, 28);
    p.button(
        launch,
        Color::WHITE,
        6,
        "shell:launcher",
        "Show Applications",
    );
    p.border(launch, Color::TRANSPARENT, 6, Color(0, 0, 0, 40));
    p.center(launch.x, launch.y + 6, 150, "Show Applications", 13, INK);
    let done = Rect::new(x + width as i32 - 100, y + 274, 76, 28);
    p.button(done, ACCENT, 6, "shell:dismiss", "Done");
    p.center(done.x, done.y + 6, 76, "Done", 13, Color::WHITE);
}

fn module(p: &mut Painter, r: Rect) {
    p.border(r, Color(255, 255, 255, 150), 14, Color(255, 255, 255, 110));
}
fn badge(p: &mut Painter, x: i32, y: i32, symbol: &str, on: bool) {
    p.circle(
        x + 14,
        y + 14,
        14,
        if on { ACCENT } else { Color(0, 0, 0, 40) },
    );
    p.symbol(
        symbol,
        x + 6,
        y + 6,
        16,
        if on { Color::WHITE } else { INK },
    );
}

/// Sliders are drawn as twenty 5% steps so a click lands on an exact level.
const SLIDER_STEPS: u32 = 20;

fn control_center(p: &mut Painter, ctx: &ShellContext<'_>) {
    dismiss_layer(p, ctx, "Close Control Center");
    let width = 324.min(ctx.width.saturating_sub(24));
    let x = ctx.width as i32 - width as i32 - 10;
    let y = 34;
    // Sequoia's layout: the connectivity module and Focus side by side, then the
    // Display and Sound sliders. AirDrop, Stage Manager, Screen Mirroring and Now
    // Playing have nothing behind them in the simulator and are left out, not faked.
    let r = Rect::new(x, y, width, 254);
    popover(p, r, 18);
    let half = (width - 36) / 2;
    let net = Rect::new(x + 12, y + 12, half, 106);
    module(p, net);
    // Every row flips a real switch and paints the position it reads back.
    for (i, (symbol, name, switch)) in [
        ("wifi", "Wi-Fi", "wifi"),
        ("bluetooth", "Bluetooth", "bluetooth"),
    ]
    .into_iter()
    .enumerate()
    {
        let ry = net.y + 12 + i as i32 * 44;
        let row = Rect::new(net.x + 6, ry - 6, half - 12, 40);
        if ctx.hovered(row) {
            p.box_(row, Color(255, 255, 255, 110), 10);
        }
        let on = ctx.switch(switch);
        badge(p, net.x + 12, ry, symbol, on);
        p.strong(net.x + 48, ry - 1, half - 56, name, 13, INK);
        p.left(
            net.x + 48,
            ry + 15,
            half - 56,
            if on { "On" } else { "Off" },
            11,
            SECONDARY,
        );
        p.region(
            row,
            &format!("shell:toggle:{switch}"),
            &format!("{name} {}", if on { "on" } else { "off" }),
        );
    }
    // Focus is its own tile, and Do Not Disturb is the Focus the machine has.
    let focus = Rect::new(x + 24 + half as i32, y + 12, half, 106);
    module(p, focus);
    if ctx.hovered(focus) {
        p.box_(focus, Color(255, 255, 255, 90), 14);
    }
    let on = ctx.switch("do_not_disturb");
    badge(p, focus.x + 12, focus.y + 36, "dnd", on);
    p.strong(focus.x + 48, focus.y + 35, half - 56, "Focus", 13, INK);
    p.left(
        focus.x + 48,
        focus.y + 51,
        half - 56,
        if on { "Do Not Disturb" } else { "Off" },
        11,
        SECONDARY,
    );
    p.region(
        focus,
        "shell:toggle:do_not_disturb",
        &format!("Focus {}", if on { "on" } else { "off" }),
    );
    for (i, (title, symbol, setting)) in [
        ("Display", "sun", "brightness"),
        ("Sound", "volume", "volume"),
    ]
    .into_iter()
    .enumerate()
    {
        let tile = Rect::new(x + 12, y + 130 + i as i32 * 60, width - 24, 54);
        module(p, tile);
        let level = u32::from(ctx.level(setting));
        p.strong(tile.x + 14, tile.y + 7, 140, title, 12, INK);
        let track = Rect::new(tile.x + 12, tile.y + 26, tile.width - 24, 22);
        p.border(track, Color(0, 0, 0, 22), 11, Color(0, 0, 0, 20));
        let fill = (track.width * level / 100).max(22);
        p.box_(Rect::new(track.x, track.y, fill, 22), Color::WHITE, 11);
        p.circle(track.x + fill as i32 - 11, track.y + 11, 11, Color::WHITE);
        p.ring(
            track.x + fill as i32 - 11,
            track.y + 11,
            11,
            1,
            Color(0, 0, 0, 40),
        );
        p.symbol(symbol, track.x + 6, track.y + 4, 14, SECONDARY);
        // One button per step, each carrying the exact level it sets.
        for step in 0..SLIDER_STEPS {
            let from = track.width * step / SLIDER_STEPS;
            let to = track.width * (step + 1) / SLIDER_STEPS;
            let percent = (step + 1) * 100 / SLIDER_STEPS;
            p.button(
                Rect::new(track.x + from as i32, track.y, to - from, 22),
                Color::TRANSPARENT,
                0,
                &format!("shell:set:{setting}:{percent}"),
                &format!("{title} {percent}%"),
            );
        }
    }
}

fn notification_center(p: &mut Painter, ctx: &ShellContext<'_>) {
    let width = 340.min(ctx.width.saturating_sub(24));
    let x = ctx.width as i32 - width as i32 - 12;
    p.region(
        Rect::new(0, 28, ctx.width, ctx.height.saturating_sub(28)),
        "shell:dismiss",
        "Close Notification Center",
    );
    let date = ctx.date();
    // Date widget.
    let today = Rect::new(x, 40, (width - 12) / 2, 156);
    popover(p, today, 20);
    p.strong(
        today.x + 16,
        today.y + 14,
        120,
        &date.weekday_name().to_uppercase(),
        11,
        Color::rgb(255, 59, 48),
    );
    p.left(
        today.x + 15,
        today.y + 28,
        120,
        &date.day.to_string(),
        44,
        INK,
    );
    p.paragraph(
        today.x + 16,
        today.y + 96,
        today.width - 32,
        "No events today",
        12,
        SECONDARY,
    );
    if ctx.installed("calendar") {
        p.region(today, "shell:launch:calendar", "Open Calendar");
    }
    // Clock widget: the simulator has no clock application, so it only reports the
    // world time and absorbs clicks through the popover beneath it.
    let clock = Rect::new(x + today.width as i32 + 12, 40, (width - 12) / 2, 156);
    popover(p, clock, 20);
    p.strong(clock.x + 16, clock.y + 14, 120, "LOCAL TIME", 11, SECONDARY);
    let time = ctx.time12();
    let (digits, meridiem) = time.split_once(' ').unwrap_or((&time, ""));
    let end = p.left(
        clock.x + 15,
        clock.y + 40,
        clock.width - 30,
        digits,
        34,
        INK,
    );
    p.left(
        clock.x + 19 + end as i32,
        clock.y + 58,
        40,
        meridiem,
        13,
        SECONDARY,
    );
    p.left(
        clock.x + 16,
        clock.y + 96,
        clock.width - 32,
        // The world keeps one clock and no place: the widget names no city.
        "Today",
        12,
        SECONDARY,
    );
    // Month widget: the grid follows the paged month, the date widget above stays today.
    let shown = ctx.panel_date();
    let paged = ctx.panel_month != 0;
    let month = Rect::new(x, 208, width, 236);
    popover(p, month, 20);
    let heading = Rect::new(month.x + 12, month.y + 8, 150, 24);
    if paged && ctx.hovered(heading) {
        p.box_(heading, Color(0, 0, 0, 14), 6);
    }
    p.strong(
        month.x + 18,
        month.y + 14,
        140,
        &if paged {
            format!("{} {}", shown.month_name().to_uppercase(), shown.year)
        } else {
            shown.month_name().to_uppercase()
        },
        11,
        Color::rgb(255, 59, 48),
    );
    // Paging chevrons, drawn here and made live below the widget's own region so the
    // later node wins the hit test.
    let chevrons = [
        (
            Rect::new(month.x + width as i32 - 64, month.y + 6, 26, 24),
            "chevron-left",
            "shell:month:prev",
            "Previous month",
        ),
        (
            Rect::new(month.x + width as i32 - 34, month.y + 6, 26, 24),
            "chevron-right",
            "shell:month:next",
            "Next month",
        ),
    ];
    for (hit, symbol, _, _) in chevrons {
        if ctx.hovered(hit) {
            p.box_(hit, Color(0, 0, 0, 14), 6);
        }
        p.symbol(symbol, hit.x + 7, hit.y + 5, 13, SECONDARY);
    }
    let cell = (width - 28) / 7;
    for (i, day) in ["S", "M", "T", "W", "T", "F", "S"].iter().enumerate() {
        p.center(
            month.x + 14 + (i as u32 * cell) as i32,
            month.y + 38,
            cell,
            day,
            11,
            TERTIARY,
        );
    }
    // Each day opens Calendar on that day, one layer above the whole-widget entry
    // point registered below, so a click on a date lands on the date.
    for day in 1..=shown.days_in_month {
        let slot = day - 1 + shown.first_weekday;
        let cx = month.x + 14 + ((slot % 7) as u32 * cell) as i32;
        let cy = month.y + 62 + (slot / 7) as i32 * 28;
        if let Some(open) = ctx.open_day(day) {
            p.region_above(
                Rect::new(cx, cy - 4, cell, 26),
                &open,
                &format!("{} {day}", shown.month_name()),
            );
        }
        // Only the world's own day is marked, and only while the grid is on its month.
        let today = !paged && day == date.day;
        if today {
            p.circle(cx + cell as i32 / 2, cy + 9, 12, Color::rgb(255, 59, 48));
        }
        p.label(
            cx,
            cy + 1,
            cell,
            &day.to_string(),
            12,
            if today { Color::WHITE } else { INK },
            today,
            Align::Center,
        );
    }
    // The feed below the widgets is what applications really posted, and each card
    // opens the one notice it belongs to. Nothing posted says exactly that.
    if ctx.notifications.is_empty() {
        let none = Rect::new(x, 456, width, 44);
        if none.y + 60 < ctx.height as i32 - 80 {
            popover(p, none, 16);
            p.center(
                none.x,
                none.y + 14,
                width,
                "No Notifications",
                13,
                SECONDARY,
            );
        }
    } else {
        let mut ny = 456;
        for (i, notice) in ctx.notifications.iter().enumerate() {
            let card = Rect::new(x, ny, width, 62);
            if card.y + 78 > ctx.height as i32 - 80 {
                break;
            }
            popover(p, card, 16);
            p.asset(
                Rect::new(card.x + 14, card.y + 16, 30, 30),
                &format!("icon/macos/{}", notice.app),
            );
            p.strong(card.x + 54, card.y + 12, width - 90, &notice.title, 13, INK);
            p.left(
                card.x + 54,
                card.y + 32,
                width - 90,
                &notice.body,
                12,
                SECONDARY,
            );
            // Unread is a flag the notice really carries, not a guess from its age.
            if !notice.seen {
                p.circle(card.x + width as i32 - 20, card.y + 20, 4, ACCENT);
            }
            p.region(card, &format!("shell:notice:{i}"), &notice.title);
            ny += 70;
        }
        let clear = Rect::new(x + width as i32 - 92, ny + 4, 80, 26);
        if clear.y + 34 < ctx.height as i32 - 80 {
            if ctx.hovered(clear) {
                p.box_(clear, Color(255, 255, 255, 60), 13);
            }
            p.button(
                clear,
                Color(0, 0, 0, 40),
                13,
                "shell:notifications:seen",
                "Mark all as read",
            );
            p.center(clear.x, clear.y + 6, 80, "Clear", 12, Color::WHITE);
        }
    }
    if ctx.installed("calendar") {
        p.region(month, "shell:launch:calendar", "Open Calendar");
    }
    // Registered after the widget so paging wins over opening Calendar.
    for (hit, _, action, label) in chevrons {
        p.region(hit, action, label);
    }
    if paged {
        p.region(
            heading,
            "shell:month:today",
            &format!("Back to {} {}", date.month_name(), date.year),
        );
    }
}

fn mission_control(p: &mut Painter, ctx: &ShellContext<'_>) {
    let full = Rect::new(0, 0, ctx.width, ctx.height);
    p.glass(full, 0, 30, Color(20, 24, 36, 70), None);
    p.region(full, "shell:dismiss", "Close Mission Control");
    let open: Vec<_> = ctx.windows.iter().rev().take(6).collect();
    if open.is_empty() {
        p.center(
            0,
            ctx.height as i32 / 2 - 10,
            ctx.width,
            "No Open Windows",
            17,
            Color::WHITE,
        );
        // Spaces exist whether or not anything is open on them.
        return spaces_bar(p, ctx);
    }
    let columns = open.len().min(3) as u32;
    let cell = (ctx.width - 120) / columns;
    for (i, w) in open.iter().enumerate() {
        let cx = 60 + (i as u32 % columns * cell) as i32 + 16;
        let cy = 96 + (i as u32 / columns) as i32 * 250;
        let card = Rect::new(cx, cy, cell - 32, 190);
        p.drop_shadow(card, 10, 24, 110, 10);
        p.border(
            card,
            Color::rgb(250, 250, 251),
            10,
            if w.focused {
                ACCENT
            } else {
                Color(0, 0, 0, 60)
            },
        );
        p.box_(
            Rect::new(card.x + 1, card.y + 1, card.width - 2, 26),
            TOOLBAR,
            9,
        );
        p.region(card, &w.action("focus"), &format!("Switch to {}", w.title));
        // The mini lights drive the window they belong to, like the real ones.
        for (n, (op, label, c)) in [
            ("close", "Close", (255, 95, 87)),
            ("minimize", "Minimize", (254, 188, 46)),
            ("maximize", "Zoom", (40, 200, 64)),
        ]
        .into_iter()
        .enumerate()
        {
            let lx = card.x + 14 + n as i32 * 14;
            p.circle(lx, card.y + 14, 4, Color::rgb(c.0, c.1, c.2));
            p.region(
                Rect::new(lx - 7, card.y + 3, 14, 22),
                &w.action(op),
                &format!("{label} {}", w.title),
            );
        }
        p.asset(
            Rect::new(card.x + card.width as i32 / 2 - 32, card.y + 70, 64, 64),
            &format!("icon/macos/{}", w.kind),
        );
        let name = app_name(&w.kind).unwrap_or(&w.title);
        p.label(
            card.x,
            card.y + 200,
            card.width,
            name,
            13,
            Color::WHITE,
            true,
            Align::Center,
        );
    }
    // Last, so the Spaces bar wins the hit test over any card that reaches it.
    spaces_bar(p, ctx);
}

/// Mission Control's Spaces bar: one tile per desktop the machine really keeps, the one
/// on screen marked, and the controls that switch, add and close them.
fn spaces_bar(p: &mut Painter, ctx: &ShellContext<'_>) {
    if ctx.height < 420 {
        return;
    }
    let y = ctx.height as i32 - 74;
    let count = ctx.workspaces.max(1);
    let full = count >= crate::WORKSPACE_LIMIT;
    let span = (count as i32 + i32::from(!full)) * 132 - 12;
    let mut x = ctx.width as i32 / 2 - span / 2;
    // A window view carries no workspace, so only the desktop on screen can show what
    // is on it; the others are named and reachable but not yet previewable.
    let here_windows = ctx.windows.iter().filter(|w| !w.minimized).count().min(3);
    for index in 0..count {
        let tile = Rect::new(x, y, 120, 54);
        let here = index == ctx.workspace;
        let name = format!("Desktop {}", index + 1);
        p.button(
            tile,
            Color(255, 255, 255, if here { 62 } else { 22 }),
            6,
            &format!("shell:workspace:{index}"),
            &name,
        );
        p.border(
            tile,
            Color::TRANSPARENT,
            6,
            if here {
                Color::WHITE
            } else {
                Color(255, 255, 255, 70)
            },
        );
        p.label(
            tile.x,
            y + 18,
            120,
            &name,
            12,
            Color(255, 255, 255, if here { 255 } else { 170 }),
            here,
            Align::Center,
        );
        if here {
            for n in 0..here_windows as i32 {
                p.box_(
                    Rect::new(tile.x + 10 + n * 16, y + 40, 12, 9),
                    Color(255, 255, 255, 170),
                    2,
                );
            }
            // The handler closes the desktop you are on, so only that tile offers it.
            if count > 1 {
                let close = Rect::new(tile.x + 100, y - 8, 20, 20);
                p.circle(close.x + 10, close.y + 10, 9, Color::rgb(60, 60, 64));
                p.button(
                    close,
                    Color::TRANSPARENT,
                    10,
                    "shell:workspace:close",
                    &format!("Close {name}"),
                );
                p.symbol("close", close.x + 5, close.y + 5, 10, Color::WHITE);
            }
        }
        x += 132;
    }
    let add = Rect::new(x, y, 120, 54);
    if full {
        p.border(add, Color(255, 255, 255, 14), 6, Color(255, 255, 255, 50));
        // Eight desktops is the machine's limit; a ninth would be refused.
        p.symbol("plus", add.x + 52, y + 19, 16, Color(255, 255, 255, 110));
        p.disabled("New desktop");
    } else {
        p.button(
            add,
            Color(255, 255, 255, if ctx.hovered(add) { 46 } else { 18 }),
            6,
            "shell:workspace:new",
            "New desktop",
        );
        p.border(add, Color::TRANSPARENT, 6, Color(255, 255, 255, 70));
        p.symbol("plus", add.x + 52, y + 19, 16, Color::WHITE);
    }
}

fn menu(p: &mut Painter, ctx: &ShellContext<'_>, panel: &str) {
    dismiss_layer(p, ctx, "Close menu");
    let front = ctx.windows.iter().find(|w| w.focused);
    // Visual Studio Code's menus live in the Mac menu bar, and run its own commands.
    if let Some(w) = front.filter(|w| w.kind == "code") {
        if let Some((_, _, items)) = crate::apps::code::commands::MENUS
            .iter()
            .find(|(name, _, _)| *name == panel)
        {
            return code_menu(p, ctx, panel, w, items);
        }
    }
    // FreeCAD's File, Edit, View and Help menus, as the window lends them.
    if let Some(w) = front.filter(|w| w.kind == "freecad") {
        if let Some(encoded) = w.chrome(&format!("mac:{panel}")) {
            return freecad_menu(p, ctx, panel, w, encoded);
        }
    }
    // (label, action, shortcut); an empty label is a separator.
    let mut entries: Vec<(String, String, &str)> = match panel {
        "apple" => vec![
            ("About This Mac".into(), "shell:settings".into(), ""),
            (String::new(), String::new(), ""),
            ("System Settings…".into(), "shell:settings".into(), ""),
            ("Applications…".into(), "shell:launcher".into(), ""),
            (String::new(), String::new(), ""),
            ("Show Desktop".into(), "shell:home".into(), ""),
            (String::new(), String::new(), ""),
            ("Lock Screen".into(), "shell:power:lock".into(), "⌃⌘Q"),
            ("Restart…".into(), "shell:power:restart".into(), ""),
            ("Shut Down…".into(), "shell:power:off".into(), ""),
        ],
        "file" => vec![
            ("New Window".into(), "shell:new".into(), "⌘N"),
            (
                "New Finder Window".into(),
                "shell:launch:files".into(),
                "⌥⌘N",
            ),
            ("New Text Document".into(), "shell:launch:editor".into(), ""),
            (String::new(), String::new(), ""),
            ("Save".into(), "shell:save".into(), "⌘S"),
        ]
        .into_iter()
        .chain(front.map(|w| ("Close Window".into(), w.action("close"), "⌘W")))
        .collect(),
        "window" => front
            .map(|w| {
                vec![
                    ("Minimize".into(), w.action("minimize"), "⌘M"),
                    ("Zoom".into(), w.action("maximize"), ""),
                    (String::new(), String::new(), ""),
                ]
            })
            .unwrap_or_default()
            .into_iter()
            .chain(ctx.windows.iter().map(|w| {
                (
                    app_name(&w.kind).map_or_else(|| w.title.clone(), str::to_owned),
                    w.action("focus"),
                    "",
                )
            }))
            .collect(),
        // macOS keeps "Show Bookmarks Sidebar" in View, and this is where Safari's
        // sidebar button leads: the saved pages themselves, each one navigable.
        "view" => vec![
            ("Show Desktop".into(), "shell:home".into(), ""),
            ("Show Applications".into(), "shell:launcher".into(), ""),
            (
                "Mission Control".into(),
                "shell:panel:overview".into(),
                "⌃↑",
            ),
        ]
        .into_iter()
        // TextEdit's Wrap to Window and Safari's text size, each a real setting.
        .chain(front.filter(|w| w.kind == "editor").map(|_| {
            (
                if ctx.settings.word_wrap {
                    "✓ Wrap to Window".to_owned()
                } else {
                    "Wrap to Window".to_owned()
                },
                "shell:toggle:word_wrap".to_owned(),
                "",
            )
        }))
        .chain(
            front
                .filter(|w| w.kind == "browser" && !w.document.is_empty())
                .into_iter()
                .flat_map(|w| {
                    [
                        (String::new(), String::new(), ""),
                        (
                            "Actual Size".to_owned(),
                            "shell:zoom:reset".to_owned(),
                            "⌘0",
                        ),
                        ("Zoom In".to_owned(), "shell:zoom:in".to_owned(), "⌘+"),
                        ("Zoom Out".to_owned(), "shell:zoom:out".to_owned(), "⌘−"),
                    ]
                    .into_iter()
                    .filter(move |(_, action, _)| {
                        action != "shell:zoom:reset" || w.zoom_percent() != 100
                    })
                }),
        )
        .chain(
            front
                .filter(|w| w.kind == "browser" && !w.document.is_empty())
                .map(|_| {
                    (
                        if ctx.bookmarked {
                            "Remove Bookmark".to_owned()
                        } else {
                            "Add Bookmark…".to_owned()
                        },
                        "shell:bookmark".to_owned(),
                        "⌘D",
                    )
                }),
        )
        .chain(
            (!ctx.bookmarks.is_empty())
                .then_some((String::new(), String::new(), ""))
                .into_iter()
                .chain(ctx.bookmarks.iter().take(8).enumerate().map(|(i, b)| {
                    (
                        if b.title.is_empty() {
                            b.url.clone()
                        } else {
                            b.title.clone()
                        },
                        format!("shell:bookmark:open:{i}"),
                        "",
                    )
                })),
        )
        .collect(),
        // Finder's Go menu. In a Finder window each entry moves that window; with none
        // in front it opens a new one there, as Finder does. Recents needs a
        // window to show it in, so it is only offered from one.
        "go" => {
            let finder = front.filter(|w| w.kind == "files");
            let home = ctx.home.trim_end_matches('/');
            let go = |path: String| match finder {
                Some(w) => w.action(&format!("content:files-location:{path}")),
                None => format!("shell:launch:files/{path}"),
            };
            let mut out: Vec<(String, String, &str)> = Vec::new();
            if let Some(w) = finder {
                if w.can_go_back {
                    out.push(("Back".into(), w.action("content:files-back"), "⌘["));
                }
                if w.can_go_forward {
                    out.push(("Forward".into(), w.action("content:files-forward"), "⌘]"));
                }
                if w.caption.is_empty() && w.document != "/" {
                    out.push((
                        "Enclosing Folder".into(),
                        w.action("content:files-up"),
                        "⌘↑",
                    ));
                }
                out.push((String::new(), String::new(), ""));
                out.push(("Recents".into(), w.action("content:files-recents"), "⇧⌘F"));
            }
            if !home.is_empty() && ctx.installed("files") {
                for (label, folder, keys) in [
                    ("Documents", "Documents", "⇧⌘O"),
                    ("Desktop", "Desktop", "⇧⌘D"),
                    ("Downloads", "Downloads", "⌥⌘L"),
                ] {
                    out.push((label.into(), go(format!("{home}/{folder}")), keys));
                }
                out.push(("Home".into(), go(home.to_owned()), "⇧⌘H"));
                out.push(("Computer".into(), go("/".into()), "⇧⌘C"));
            }
            out
        }
        "edit" => vec![(
            "Find Applications…".into(),
            "shell:panel:spotlight".into(),
            "⌘F",
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
            ("New Finder Window".into(), "shell:launch:files".into(), ""),
            ("System Settings…".into(), "shell:settings".into(), ""),
            (
                "Search Applications".into(),
                "shell:panel:spotlight".into(),
                "",
            ),
        ],
    };
    entries.retain(|(_, action, _)| {
        action != "shell:save"
            || front.is_some_and(|w| matches!(w.kind.as_str(), "editor" | "preview" | "pixelmator"))
    });
    while entries.last().is_some_and(|(label, _, _)| label.is_empty()) {
        entries.pop();
    }
    let desired_x = match panel {
        "apple" => 8,
        "context" => ctx.hover.map_or(ctx.width as i32 / 3, |(x, _)| x),
        name => menu_layout(p, ctx)
            .iter()
            .find(|(label, _, _)| label.eq_ignore_ascii_case(name))
            .map_or(ctx.width as i32 - 260, |(_, x, _)| *x),
    };
    let width = 250.min(ctx.width.saturating_sub(16));
    let x = desired_x
        .max(8)
        .min(ctx.width.saturating_sub(width + 8) as i32);
    let top = if panel == "context" {
        ctx.hover
            .map_or(120, |(_, y)| y)
            .clamp(30, ctx.height as i32 - 200)
    } else {
        29
    };
    let height: u32 = entries
        .iter()
        .map(|(label, _, _)| if label.is_empty() { 11 } else { 24 })
        .sum::<u32>()
        + 10;
    popover(p, Rect::new(x, top, width, height), 7);
    let mut y = top + 5;
    for (label, action, shortcut) in &entries {
        if label.is_empty() {
            p.hline(x + 12, y + 5, width - 24, SEPARATOR);
            y += 11;
            continue;
        }
        let row = Rect::new(x + 5, y, width - 10, 24);
        let hover = ctx.hovered(row);
        if hover {
            p.box_(row, ACCENT, 5);
        }
        let ink = if hover { Color::WHITE } else { INK };
        p.left(x + 16, y + 4, width - 80, label, 13, ink);
        if !shortcut.is_empty() {
            p.right(
                x + width as i32 - 76,
                y + 4,
                62,
                shortcut,
                13,
                if hover { Color::WHITE } else { TERTIARY },
            );
        }
        p.region(row, action, label);
        y += 24;
    }
}

/// A Visual Studio Code menu in the Mac menu bar. Each entry dispatches the command
/// into the window; one the editor cannot run now is shown greyed and announced so.
/// A FreeCAD menu in the Mac menu bar: its entries arrive encoded in the window's
/// chrome (label, target, why disabled, shortcut), and each runs FreeCAD's command.
fn freecad_menu(
    p: &mut Painter,
    ctx: &ShellContext<'_>,
    panel: &str,
    w: &WindowView,
    encoded: &str,
) {
    let items: Vec<Vec<&str>> = encoded
        .split('\u{1e}')
        .map(|i| i.split('\u{1f}').collect())
        .collect();
    let desired_x = menu_layout(p, ctx)
        .iter()
        .find(|(label, _, _)| label.eq_ignore_ascii_case(panel))
        .map_or(ctx.width as i32 - 280, |(_, x, _)| *x);
    let width = 300.min(ctx.width.saturating_sub(16));
    let x = desired_x
        .max(8)
        .min(ctx.width.saturating_sub(width + 8) as i32);
    let height: u32 = items
        .iter()
        .map(|i| {
            if i.first().is_none_or(|l| l.is_empty()) {
                11
            } else {
                24
            }
        })
        .sum::<u32>()
        + 10;
    popover(p, Rect::new(x, 29, width, height), 7);
    let mut y = 34;
    for item in &items {
        let (label, target, why, keys) = (
            item.first().copied().unwrap_or(""),
            item.get(1).copied().unwrap_or(""),
            item.get(2).copied().unwrap_or(""),
            item.get(3).copied().unwrap_or(""),
        );
        if label.is_empty() {
            p.hline(x + 12, y + 5, width - 24, SEPARATOR);
            y += 11;
            continue;
        }
        let row = Rect::new(x + 5, y, width - 10, 24);
        let live = why.is_empty() && !target.is_empty();
        let hover = live && ctx.hovered(row);
        if hover {
            p.box_(row, ACCENT, 5);
        }
        let ink = match (hover, live) {
            (true, _) => Color::WHITE,
            (false, true) => INK,
            (false, false) => TERTIARY,
        };
        p.left(x + 16, y + 4, width - 110, label, 13, ink);
        if !keys.is_empty() {
            let keys = keys.replace("Ctrl+", "⌘");
            p.right(
                x + width as i32 - 100,
                y + 4,
                86,
                &keys,
                13,
                if hover { Color::WHITE } else { TERTIARY },
            );
        }
        if live {
            p.region(row, &w.action(&format!("content:{target}")), label);
        } else {
            p.box_(row, Color::TRANSPARENT, 0);
            p.disabled(&format!("{label}: {why}"));
        }
        y += 24;
    }
}

fn code_menu(p: &mut Painter, ctx: &ShellContext<'_>, panel: &str, w: &WindowView, items: &[&str]) {
    use crate::apps::code::commands::{command, display_keys};
    let enabled: Vec<&str> = w.chrome("enabled").unwrap_or("").split(',').collect();
    let desired_x = menu_layout(p, ctx)
        .iter()
        .find(|(label, _, _)| label.eq_ignore_ascii_case(panel))
        .map_or(ctx.width as i32 - 280, |(_, x, _)| *x);
    let width = 280.min(ctx.width.saturating_sub(16));
    let x = desired_x
        .max(8)
        .min(ctx.width.saturating_sub(width + 8) as i32);
    let height: u32 = items
        .iter()
        .map(|id| if *id == "-" { 11 } else { 24 })
        .sum::<u32>()
        + 10;
    popover(p, Rect::new(x, 29, width, height), 7);
    let mut y = 34;
    for id in items {
        if *id == "-" {
            p.hline(x + 12, y + 5, width - 24, SEPARATOR);
            y += 11;
            continue;
        }
        let Some(c) = command(id) else { continue };
        let row = Rect::new(x + 5, y, width - 10, 24);
        let live = enabled.contains(id);
        let hover = live && ctx.hovered(row);
        if hover {
            p.box_(row, ACCENT, 5);
        }
        let ink = match (hover, live) {
            (true, _) => Color::WHITE,
            (false, true) => INK,
            (false, false) => TERTIARY,
        };
        p.left(x + 16, y + 4, width - 100, c.label, 13, ink);
        let keys = display_keys(c.keys, true);
        if !keys.is_empty() {
            p.right(
                x + width as i32 - 96,
                y + 4,
                82,
                &keys,
                13,
                if hover { Color::WHITE } else { TERTIARY },
            );
        }
        if live {
            p.region(row, &w.action(&format!("content:code:cmd:{id}")), c.label);
        } else {
            p.box_(row, Color::TRANSPARENT, 0);
            p.disabled(&format!("{}: not available right now", c.label));
        }
        y += 24;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_scene::DesktopTheme;
    fn context<'a>(
        windows: &'a [WindowView],
        settings: &'a crate::SystemSettings,
    ) -> ShellContext<'a> {
        ShellContext {
            theme: DesktopTheme::Macos,
            width: 1280,
            height: 800,
            clock_us: 0,
            title: "",
            launcher_open: false,
            active: true,
            windows,
            installed_apps: &[],
            panel: None,
            search: "",
            hover: None,
            desktop_selection: None,
            settings,
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
        }
    }
    fn browser(tabs: &[&str], active: usize) -> WindowView {
        WindowView {
            id: 7,
            title: "Safari — http://example.test/".into(),
            kind: "browser".into(),
            rect: Rect::new(0, 0, 900, 600),
            focused: true,
            caption: "Example".into(),
            tabs: tabs.iter().map(|t| (*t).to_owned()).collect(),
            active_tab: active,
            ..Default::default()
        }
    }
    fn actions(p: &Painter) -> Vec<&str> {
        p.scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.as_deref())
            .collect()
    }
    fn hit(p: &Painter, x: i32, y: i32) -> Option<&str> {
        p.scene
            .hit_test(x, y)
            .and_then(|n| n.interaction.as_deref())
    }

    #[test]
    fn safari_tab_strip_selects_closes_and_opens_real_tabs() {
        let windows = vec![browser(&["One", "Two"], 1)];
        let settings = crate::SystemSettings::DEFAULT;
        let ctx = context(&windows, &settings);
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        browser_chrome(&mut p, &ctx, &windows[0]);
        assert_eq!(hit(&p, 228, 62), Some("shell:tab:select:0"));
        assert_eq!(hit(&p, 660, 62), Some("shell:tab:select:1"));
        assert_eq!(hit(&p, 433, 62), Some("shell:tab:close:0"));
        assert_eq!(hit(&p, 841, 21), Some("shell:tab:new"));
        // The sidebar shows the bookmarks macOS keeps in the View menu.
        assert_eq!(hit_labelled(&p, "Bookmarks"), Some("shell:panel:view"));
        // The tab overview is gone entirely; the strip above is the real control.
        assert!(labelled(&p, "Tab overview").is_none());
        // This preview owns no page, so sharing and bookmarking are greyed: no
        // interaction, not focusable, and a click on either answers with neither id.
        for (label, action) in [
            ("Share", "shell:share:chat"),
            ("Add bookmark", "shell:bookmark"),
        ] {
            let node = labelled(&p, label).unwrap_or_else(|| panic!("missing {label}"));
            let semantic = node.semantic.as_ref().unwrap();
            assert!(semantic.disabled && !semantic.focusable && node.interaction.is_none());
            assert_ne!(hit_labelled(&p, label), Some(action), "{label}");
        }
        // With a page loaded both become real, and the star reports whether it is saved.
        let loaded = vec![WindowView {
            document: "http://example.test/".into(),
            ..windows[0].clone()
        }];
        let mut ctx = context(&loaded, &settings);
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        browser_chrome(&mut p, &ctx, &loaded[0]);
        assert_eq!(
            hit_labelled(&p, "Share with Messages"),
            Some("shell:share:chat")
        );
        assert_eq!(hit_labelled(&p, "Add bookmark"), Some("shell:bookmark"));
        ctx.bookmarked = true;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        browser_chrome(&mut p, &ctx, &loaded[0]);
        assert_eq!(hit_labelled(&p, "Remove bookmark"), Some("shell:bookmark"));
        // Nothing to share with means the control is greyed, not refused at the handler.
        let apps: Vec<String> = vec!["browser".into()];
        let mut ctx = context(&loaded, &settings);
        ctx.installed_apps = &apps;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        browser_chrome(&mut p, &ctx, &loaded[0]);
        assert!(labelled(&p, "Share").unwrap().interaction.is_none());
    }

    #[test]
    fn the_view_menu_lists_the_pages_the_machine_really_saved() {
        let settings = crate::SystemSettings::DEFAULT;
        let loaded = vec![WindowView {
            document: "http://example.test/".into(),
            ..browser(&["One"], 0)
        }];
        let saved = [
            crate::Bookmark {
                title: "Intranet".into(),
                url: "http://intranet.internal/".into(),
            },
            crate::Bookmark {
                title: String::new(),
                url: "http://example.test/".into(),
            },
        ];
        let mut ctx = context(&loaded, &settings);
        ctx.panel = Some("view");
        ctx.bookmarks = &saved;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert_eq!(hit_labelled(&p, "Add Bookmark…"), Some("shell:bookmark"));
        assert_eq!(hit_labelled(&p, "Intranet"), Some("shell:bookmark:open:0"));
        // A page saved without a title is listed by its address, never as an empty row.
        assert_eq!(
            hit_labelled(&p, "http://example.test/"),
            Some("shell:bookmark:open:1")
        );
        // Saved already: the same entry takes it back off the list.
        ctx.bookmarked = true;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert_eq!(hit_labelled(&p, "Remove Bookmark"), Some("shell:bookmark"));
        // With nothing saved the menu grows no empty section.
        let mut ctx = context(&loaded, &settings);
        ctx.panel = Some("view");
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert!(!actions(&p)
            .iter()
            .any(|a| a.starts_with("shell:bookmark:open")));
    }

    #[test]
    fn a_tabless_browser_preview_paints_no_tab_controls() {
        let windows = vec![browser(&[], 0)];
        let settings = crate::SystemSettings::DEFAULT;
        let ctx = context(&windows, &settings);
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        browser_chrome(&mut p, &ctx, &windows[0]);
        assert!(!actions(&p)
            .iter()
            .any(|a| a.starts_with("shell:tab:select")));
        assert_eq!(hit(&p, 228, 62), None);
    }

    #[test]
    fn finder_toolbar_navigates_history_home_and_the_computer() {
        let windows = vec![WindowView {
            id: 3,
            title: "Finder".into(),
            kind: "files".into(),
            rect: Rect::new(0, 0, 900, 600),
            focused: true,
            document: "/work".into(),
            can_go_back: true,
            can_go_forward: true,
            ..Default::default()
        }];
        let settings = crate::SystemSettings::DEFAULT;
        let ctx = context(&windows, &settings);
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        window_frame(&mut p, &ctx, &windows[0]);
        for (x, target) in [
            (196, "files-back"),
            (230, "files-forward"),
            (873, "files-search"),
        ] {
            assert_eq!(
                hit(&p, x, 21),
                Some(format!("window:3:content:{target}").as_str())
            );
        }
        // One view control, and it really swaps the tab between icon and list layouts.
        assert_eq!(
            hit_labelled(&p, "Icon or list view"),
            Some("window:3:content:files-view")
        );
        for gone in ["Icon view", "List view"] {
            assert!(labelled(&p, gone).is_none(), "{gone}");
        }
        // A query in force opens the field, and the field can clear it.
        let finding = vec![WindowView {
            query: "notes".into(),
            ..windows[0].clone()
        }];
        let ctx = context(&finding, &settings);
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        window_frame(&mut p, &ctx, &finding[0]);
        assert!(texts(&p).contains(&"notes"));
        assert_eq!(
            hit_labelled(&p, "Clear search"),
            Some("window:3:content:files-search-clear")
        );
        // A fresh window has no history, so both chevrons are greyed rather than live.
        let fresh = vec![WindowView {
            can_go_back: false,
            can_go_forward: false,
            ..windows[0].clone()
        }];
        let ctx = context(&fresh, &settings);
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        window_frame(&mut p, &ctx, &fresh[0]);
        // The toolbar's own drag region answers instead of a chevron that cannot act.
        assert_eq!(hit(&p, 196, 21), Some("window:3:drag"));
        assert_eq!(hit(&p, 230, 21), Some("window:3:drag"));
    }

    #[test]
    fn control_center_flips_real_switches_and_sets_exact_levels() {
        let mut settings = crate::SystemSettings::DEFAULT;
        settings.wifi = false;
        settings.brightness = 35;
        let mut ctx = context(&[], &settings);
        ctx.panel = Some("quick");
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert_eq!(hit(&p, 1030, 72), Some("shell:toggle:wifi"));
        assert_eq!(hit(&p, 1030, 116), Some("shell:toggle:bluetooth"));
        // Focus is its own tile beside the connectivity module, as in Sequoia.
        let focus = labelled(&p, "Focus off").expect("a Focus tile");
        let b = focus.bounds;
        assert_eq!(
            hit(&p, b.x + b.width as i32 / 2, b.y + b.height as i32 / 2),
            Some("shell:toggle:do_not_disturb")
        );
        // The off row reads its true position back.
        assert!(p
            .scene
            .nodes
            .iter()
            .any(|n| n.semantic.as_ref().is_some_and(|s| s.label == "Wi-Fi off")));
        let steps: Vec<&str> = actions(&p)
            .into_iter()
            .filter(|a| a.starts_with("shell:set:"))
            .collect();
        assert_eq!(steps.len() as u32, SLIDER_STEPS * 2);
        for setting in ["brightness", "volume"] {
            for percent in [5, 50, 100] {
                let want = format!("shell:set:{setting}:{percent}");
                let node = p
                    .scene
                    .nodes
                    .iter()
                    .find(|n| n.interaction.as_deref() == Some(want.as_str()))
                    .unwrap_or_else(|| panic!("missing {want}"));
                let b = node.bounds;
                assert_eq!(
                    hit(&p, b.x + b.width as i32 / 2, b.y + 11),
                    Some(want.as_str())
                );
            }
        }
    }

    #[test]
    fn every_transient_surface_closes_on_an_outside_click() {
        let settings = crate::SystemSettings::DEFAULT;
        for panel in [
            "search",
            "settings",
            "quick",
            "notifications",
            "overview",
            "apple",
            "file",
            "window",
        ] {
            let mut ctx = context(&[], &settings);
            ctx.panel = Some(panel);
            let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
            chrome(&mut p, &ctx);
            assert_eq!(
                hit(&p, 60, 620),
                Some("shell:dismiss"),
                "{panel} traps clicks"
            );
            // The menu bar stays live so the pull-downs still track the pointer;
            // Mission Control is the one surface that really does take the screen.
            let above = if panel == "overview" {
                "shell:dismiss"
            } else {
                "shell:panel:apple"
            };
            assert_eq!(hit(&p, 20, 14), Some(above), "{panel}");
        }
    }

    #[test]
    fn spotlight_field_focuses_search_instead_of_doing_nothing() {
        let settings = crate::SystemSettings::DEFAULT;
        let mut ctx = context(&[], &settings);
        ctx.panel = Some("search");
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert_eq!(hit(&p, 640, 185), Some("shell:search"));
    }

    #[test]
    fn menu_bar_status_reports_switches_and_opens_control_center() {
        let mut settings = crate::SystemSettings::DEFAULT;
        settings.wifi = false;
        settings.battery_saver = true;
        let ctx = context(&[], &settings);
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        let labels: Vec<&str> = p
            .scene
            .nodes
            .iter()
            .filter(|n| n.interaction.as_deref() == Some("shell:panel:control"))
            .filter_map(|n| n.semantic.as_ref().map(|s| s.label.as_str()))
            .collect();
        assert!(labels.contains(&"Wi-Fi off"));
        assert!(labels.contains(&"Battery, Low Power Mode"));
        // The application menu name is a control, not a caption.
        assert!(p.scene.nodes.iter().any(|n| n.interaction.as_deref()
            == Some("shell:panel:window")
            && n.semantic
                .as_ref()
                .is_some_and(|s| s.label == "Finder menu")));
    }

    #[test]
    fn apple_menu_offers_real_power_controls() {
        let settings = crate::SystemSettings::DEFAULT;
        let mut ctx = context(&[], &settings);
        ctx.panel = Some("apple");
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        let found = actions(&p);
        for action in ["shell:power:lock", "shell:power:restart", "shell:power:off"] {
            assert!(found.contains(&action), "missing {action}");
        }
    }

    #[test]
    fn locked_and_sleeping_screens_replace_the_shell_and_offer_a_way_back() {
        let settings = crate::SystemSettings::DEFAULT;
        for screen in [crate::ScreenState::Locked, crate::ScreenState::Off] {
            let mut ctx = context(&[], &settings);
            ctx.screen = screen;
            let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
            chrome(&mut p, &ctx);
            assert_eq!(hit(&p, 640, 400), Some("shell:power:wake"));
            // No menu bar or Dock is reachable behind the lock screen.
            assert!(!actions(&p)
                .iter()
                .any(|a| a.starts_with("shell:panel:") || a.starts_with("shell:launch:")));
        }
        let mut ctx = context(&[], &settings);
        ctx.screen = crate::ScreenState::Locked;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        let found = actions(&p);
        assert!(found.contains(&"shell:power:off") && found.contains(&"shell:power:restart"));
    }

    #[test]
    fn mission_control_mini_lights_drive_their_own_window() {
        let windows = vec![WindowView {
            id: 11,
            title: "Terminal".into(),
            kind: "terminal".into(),
            rect: Rect::new(80, 80, 600, 400),
            focused: true,
            ..Default::default()
        }];
        let settings = crate::SystemSettings::DEFAULT;
        let mut ctx = context(&windows, &settings);
        ctx.panel = Some("overview");
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert_eq!(hit(&p, 90, 108), Some("window:11:close"));
        assert_eq!(hit(&p, 104, 108), Some("window:11:minimize"));
        assert_eq!(hit(&p, 118, 108), Some("window:11:maximize"));
        assert_eq!(hit(&p, 400, 200), Some("window:11:focus"));
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
        hit(
            p,
            node.bounds.x + node.bounds.width as i32 / 2,
            node.bounds.y + node.bounds.height as i32 / 2,
        )
    }

    #[test]
    fn every_installed_application_is_reachable_and_the_dock_keeps_its_favourites() {
        let settings = crate::SystemSettings::DEFAULT;
        // Launchpad carries the whole roster; the Dock carries the common half.
        let mut ctx = context(&[], &settings);
        ctx.launcher_open = true;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        let launches = actions(&p);
        for (kind, _) in APPS {
            assert!(
                launches.contains(&format!("shell:launch:{kind}").as_str()),
                "Launchpad hides {kind}"
            );
        }
        let mut dock_only = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        let ctx = context(&[], &settings);
        chrome(&mut dock_only, &ctx);
        let docked = actions(&dock_only);
        assert!(docked.contains(&"shell:launch:notes"));
        // Calculator and Clock are Launchpad-only, and Launchpad is one click away.
        for kind in ["calculator", "clock"] {
            assert!(!docked.contains(&format!("shell:launch:{kind}").as_str()));
        }
        assert!(docked.contains(&"shell:launcher"));
        // A machine without an application must not offer it anywhere.
        let apps: Vec<String> = vec!["files".into(), "notes".into()];
        let mut ctx = context(&[], &settings);
        ctx.installed_apps = &apps;
        ctx.launcher_open = true;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        for action in actions(&p) {
            if let Some(kind) = action.strip_prefix("shell:launch:") {
                assert!(apps.iter().any(|a| a == kind), "{kind} is not installed");
            }
        }
    }
    #[test]
    fn the_month_widget_pages_the_month_notification_center_reports() {
        let settings = crate::SystemSettings::DEFAULT;
        let mut ctx = context(&[], &settings);
        ctx.panel = Some("notifications");
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        // The chevrons sit above the widget's own "Open Calendar" region, not under it.
        assert_eq!(hit_labelled(&p, "Previous month"), Some("shell:month:prev"));
        assert_eq!(hit_labelled(&p, "Next month"), Some("shell:month:next"));
        assert!(texts(&p).contains(&"SEPTEMBER"));
        assert!(!actions(&p).contains(&"shell:month:today"));
        // Paged away, the grid is the month `panel_date` reports and says its year.
        ctx.panel_month = -9;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        let shown = ctx.panel_date();
        assert_eq!(
            (shown.year, shown.month, shown.days_in_month),
            (2025, 12, 31)
        );
        assert!(texts(&p).contains(&"DECEMBER 2025"));
        assert_eq!(
            hit_labelled(&p, "Back to September 2026"),
            Some("shell:month:today")
        );
        let days = texts(&p)
            .into_iter()
            .filter(|t| t.parse::<u64>().is_ok_and(|d| (1..=31).contains(&d)))
            .count();
        // Thirty-one cells plus the date widget's own big "17".
        assert_eq!(days, 32);
        // Nothing in the grid is a target: the widget opens Calendar, the chevrons page.
        let month: Vec<&str> = p
            .scene
            .nodes
            .iter()
            .filter(|n| n.bounds.y >= 208 && n.bounds.y < 444)
            .filter_map(|n| n.interaction.as_deref())
            .collect();
        assert!(!month.is_empty());
        assert!(
            month.iter().all(|a| a.starts_with("shell:month:")
                || matches!(*a, "shell:launch:calendar" | "shell:noop")),
            "{month:?}"
        );
    }

    #[test]
    fn safari_greys_history_and_reload_a_fresh_window_has_not_got() {
        let settings = crate::SystemSettings::DEFAULT;
        let fresh = vec![WindowView {
            id: 7,
            title: "Safari".into(),
            kind: "browser".into(),
            rect: Rect::new(0, 0, 900, 600),
            focused: true,
            ..Default::default()
        }];
        let ctx = context(&fresh, &settings);
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        browser_chrome(&mut p, &ctx, &fresh[0]);
        for (label, action) in [
            ("Back", "back"),
            ("Forward", "forward"),
            ("Reload", "reload"),
        ] {
            let node = labelled(&p, label).unwrap();
            let semantic = node.semantic.as_ref().unwrap();
            assert!(semantic.disabled && !semantic.focusable, "{label}");
            assert!(node.interaction.is_none(), "{label}");
            // A greyed glyph is never what a click on it answers with.
            assert_ne!(
                hit_labelled(&p, label),
                Some(format!("window:7:content:shell:{action}").as_str()),
                "{label}"
            );
        }
        let loaded = vec![WindowView {
            document: "http://example.test/".into(),
            can_go_back: true,
            can_go_forward: true,
            ..fresh[0].clone()
        }];
        let ctx = context(&loaded, &settings);
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        browser_chrome(&mut p, &ctx, &loaded[0]);
        for (label, action) in [
            ("Back", "back"),
            ("Forward", "forward"),
            ("Reload", "reload"),
        ] {
            assert_eq!(
                hit_labelled(&p, label),
                Some(format!("window:7:content:shell:{action}").as_str())
            );
        }
    }

    #[test]
    fn the_settings_sheet_leads_somewhere_and_paints_no_dead_circles() {
        let settings = crate::SystemSettings::DEFAULT;
        let mut ctx = context(&[], &settings);
        ctx.panel = Some("settings");
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        let found = actions(&p);
        assert!(found.contains(&"shell:panel:control"));
        assert!(found.contains(&"shell:panel:overview"));
        // A sheet has one traffic light, not three: the other two were never controls.
        assert_eq!(
            hit_labelled(&p, "Close System Settings"),
            Some("shell:dismiss")
        );
        for gone in ["Minimize", "Zoom"] {
            assert!(labelled(&p, gone).is_none(), "{gone}");
        }
    }

    #[test]
    fn mission_control_shows_the_real_spaces_and_switches_adds_and_closes_them() {
        let settings = crate::SystemSettings::DEFAULT;
        let windows = vec![WindowView {
            id: 11,
            title: "TextEdit".into(),
            kind: "editor".into(),
            rect: Rect::new(120, 90, 500, 360),
            focused: true,
            ..Default::default()
        }];
        // One space: switching to it is still real, and the last one cannot be closed.
        let mut ctx = context(&windows, &settings);
        ctx.panel = Some("overview");
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert_eq!(hit_labelled(&p, "Desktop 1"), Some("shell:workspace:0"));
        assert_eq!(hit_labelled(&p, "New desktop"), Some("shell:workspace:new"));
        assert!(labelled(&p, "Close Desktop 1").is_none());
        // Three spaces, sitting on the third: every tile is reachable by its own index.
        ctx.workspaces = 3;
        ctx.workspace = 2;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        for index in 0..3 {
            assert_eq!(
                hit_labelled(&p, &format!("Desktop {}", index + 1)),
                Some(format!("shell:workspace:{index}").as_str()),
                "desktop {index}"
            );
        }
        assert_eq!(
            hit_labelled(&p, "Close Desktop 3"),
            Some("shell:workspace:close")
        );
        assert!(labelled(&p, "Close Desktop 1").is_none());
        // At the limit the plus is announced unavailable and cannot be hit.
        ctx.workspaces = crate::WORKSPACE_LIMIT;
        ctx.workspace = 0;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        let node = labelled(&p, "New desktop").unwrap();
        assert!(node.semantic.as_ref().unwrap().disabled);
        assert!(node.interaction.is_none() && !node.semantic.as_ref().unwrap().focusable);
        assert_ne!(hit_labelled(&p, "New desktop"), Some("shell:workspace:new"));
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
    fn notification_center_reports_the_notices_the_machine_really_posted() {
        let settings = crate::SystemSettings::DEFAULT;
        let posted = [notice("files", "Screenshot taken", false)];
        let mut ctx = context(&[], &settings);
        ctx.panel = Some("notifications");
        // Nothing posted: the panel says so rather than listing an invented card.
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert!(texts(&p).contains(&"No Notifications"));
        assert!(!actions(&p).iter().any(|a| a.starts_with("shell:notice:")));
        ctx.notifications = &posted;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert!(!texts(&p).contains(&"No Notifications"));
        assert_eq!(hit_labelled(&p, "Screenshot taken"), Some("shell:notice:0"));
        assert_eq!(
            hit_labelled(&p, "Mark all as read"),
            Some("shell:notifications:seen")
        );
    }

    #[test]
    fn the_dock_trash_opens_the_folder_deleted_files_really_go_to() {
        let settings = crate::SystemSettings::DEFAULT;
        let ctx = context(&[], &settings);
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert_eq!(hit_labelled(&p, "Trash"), Some("shell:trash"));
        // Launchpad's page dot is gone: a page that can never turn is not a control.
        let mut ctx = context(&[], &settings);
        ctx.launcher_open = true;
        let mut p = Painter::themed(DesktopTheme::Macos, 1280, 800, 1);
        chrome(&mut p, &ctx);
        assert!(labelled(&p, "Page 1 of 1").is_none());
    }
}
