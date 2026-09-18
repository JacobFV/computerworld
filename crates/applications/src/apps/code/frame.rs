//! Visual Studio Code's custom title bar, which replaces the platform's own on every
//! desktop: traffic lights on macOS, the menu bar and window buttons on Windows and
//! Linux, and the command center in the middle. Every control here forwards to the
//! window's content, so it reaches the same command the palette and shortcuts do.
use super::icons;
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter, ShellContext, WindowView};
use cw_scene::{Color, Rect};

/// Height of the title bar; `window_content_rect_for_kind` starts the client below it.
pub const TITLE_H: u32 = 35;

pub struct TitleColors {
    pub bg: Color,
    pub fg: Color,
    pub border: Color,
    pub hover: Color,
}
pub fn title_colors(dark: bool, focused: bool) -> TitleColors {
    match (dark, focused) {
        (true, true) => TitleColors {
            bg: Color::rgb(0x18, 0x18, 0x18),
            fg: Color::rgb(0xCC, 0xCC, 0xCC),
            border: Color::rgb(0x2B, 0x2B, 0x2B),
            hover: Color(255, 255, 255, 26),
        },
        (true, false) => TitleColors {
            bg: Color::rgb(0x1F, 0x1F, 0x1F),
            fg: Color::rgb(0x9D, 0x9D, 0x9D),
            border: Color::rgb(0x2B, 0x2B, 0x2B),
            hover: Color(255, 255, 255, 26),
        },
        (false, focused) => TitleColors {
            bg: Color::rgb(0xF8, 0xF8, 0xF8),
            fg: if focused {
                Color::rgb(0x1E, 0x1E, 0x1E)
            } else {
                Color::rgb(0x8B, 0x94, 0x9E)
            },
            border: Color::rgb(0xE5, 0xE5, 0xE5),
            hover: Color(0, 0, 0, 20),
        },
    }
}

/// Width of the command center for a window this wide.
pub fn command_center_width(width: u32) -> u32 {
    (width * 38 / 100).clamp(140, 600)
}
/// Menu bar titles that fit beside the command center: (name, label, x from the
/// window's left edge, width). Shared with the content, which drops each menu down
/// beneath its title.
pub fn menu_bar(p: &Painter, width: u32) -> Vec<(&'static str, &'static str, i32, u32)> {
    let limit = (width.saturating_sub(command_center_width(width)) / 2) as i32 - 8;
    let mut x = 36;
    let mut out = vec![];
    for (name, label, _) in super::commands::MENUS {
        if *name == "manage" {
            continue;
        }
        let w = p.measure(label, 13, false) + 16;
        if x + w as i32 > limit {
            break;
        }
        out.push((*name, *label, x, w));
        x += w as i32;
    }
    out
}

pub fn title_bar(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let r = w.rect;
    let colors = title_colors(w.dark_chrome, w.focused);
    let radius = crate::desktop_scene::corner_radius(ctx.theme, w.maximized).saturating_sub(1);
    let bar = Rect::new(r.x + 1, r.y + 1, r.width.saturating_sub(2), TITLE_H - 1);
    p.box_(bar, colors.bg, radius);
    p.box_(
        Rect::new(bar.x, r.y + 18, bar.width, TITLE_H - 18),
        colors.bg,
        0,
    );
    p.hline(bar.x, r.y + TITLE_H as i32 - 1, bar.width, colors.border);
    let mac = ctx.theme == DesktopTheme::Macos;
    let controls = if mac { 0 } else { 138 };
    p.region(
        Rect::new(r.x, r.y, r.width.saturating_sub(controls), TITLE_H),
        &w.action("drag"),
        &format!("Move {}", w.title),
    );
    let action = |name: &str| w.action(&format!("content:code:{name}"));
    if mac {
        crate::desktop_scene::macos::traffic_lights(p, ctx, w, 4);
    } else {
        icons::logo(p, r.x + 10, r.y + 9, 16, None);
        let open = w.chrome("menu").unwrap_or("");
        for (name, label, x, width) in menu_bar(p, r.width) {
            let hit = Rect::new(r.x + x, r.y + 5, width, 24);
            if open == name || ctx.hovered(hit) {
                p.box_(hit, colors.hover, 5);
            }
            p.label(
                hit.x,
                hit.y + 4,
                width,
                label,
                13,
                colors.fg,
                false,
                Align::Center,
            );
            p.region(hit, &action(&format!("menu:{name}")), label);
        }
    }
    // The command center: what the window is working on, and the way into Quick Open.
    let cw = command_center_width(r.width);
    let cc = Rect::new(
        r.x + (r.width.saturating_sub(cw) / 2) as i32,
        r.y + 6,
        cw,
        22,
    );
    let (fill, edge) = if w.dark_chrome {
        (Color(255, 255, 255, 13), Color(255, 255, 255, 30))
    } else {
        (Color(0, 0, 0, 5), Color(0, 0, 0, 34))
    };
    p.border(
        cc,
        if ctx.hovered(cc) { colors.hover } else { fill },
        6,
        edge,
    );
    let text = if w.caption.is_empty() {
        "Search".to_owned()
    } else {
        w.caption.clone()
    };
    let tw = p.measure(&text, 12, false).min(cw.saturating_sub(40));
    let tx = cc.x + (cw as i32 - tw as i32) / 2 + 9;
    p.symbol("search", tx - 20, cc.y + 4, 14, colors.fg);
    p.label(
        tx,
        cc.y + 3,
        tw + 2,
        &text,
        12,
        colors.fg,
        false,
        Align::Left,
    );
    let target = if w.caption.is_empty() {
        "cmd:workbench.action.showCommands"
    } else {
        "cmd:workbench.action.quickOpen"
    };
    p.region(cc, &action(target), "Search files by name");
    // Layout toggles light the pane they show.
    let right = r.x + r.width as i32 - controls as i32;
    for (i, (panel, id, label)) in [
        (
            false,
            "workbench.action.toggleSidebarVisibility",
            "Toggle Primary Side Bar",
        ),
        (true, "workbench.action.togglePanel", "Toggle Panel"),
    ]
    .into_iter()
    .enumerate()
    {
        let hit = Rect::new(right - 64 + i as i32 * 28, r.y + 6, 24, 22);
        if ctx.hovered(hit) {
            p.box_(hit, colors.hover, 5);
        }
        let on = w.chrome(if panel { "panel" } else { "sidebar" }) == Some("1");
        icons::layout(p, hit.x + 4, hit.y + 3, 16, panel, on, colors.fg);
        p.region(hit, &action(&format!("cmd:{id}")), label);
    }
    if mac {
        return;
    }
    // VS Code draws its own window buttons on Windows and Linux.
    for (i, (verb, label)) in [
        ("minimize", "Minimize"),
        ("maximize", if w.maximized { "Restore" } else { "Maximize" }),
        ("close", "Close"),
    ]
    .into_iter()
    .enumerate()
    {
        let x = r.x + r.width as i32 - 1 - 138 + i as i32 * 46;
        let hit = Rect::new(x, r.y + 1, 46, TITLE_H - 2);
        let hovered = ctx.hovered(hit);
        let glyph = if hovered && verb == "close" {
            p.box_(hit, Color::rgb(0xE8, 0x11, 0x23), 0);
            Color::WHITE
        } else {
            if hovered {
                p.box_(hit, colors.hover, 0);
            }
            colors.fg
        };
        let (gx, gy) = (x + 18, r.y + 12);
        match verb {
            "minimize" => p.line(vec![(gx, gy + 5), (gx + 10, gy + 5)], glyph, 1),
            "maximize" if w.maximized => {
                p.border(
                    Rect::new(gx + 2, gy - 1, 8, 8),
                    Color::TRANSPARENT,
                    0,
                    glyph,
                );
                p.box_(Rect::new(gx, gy + 1, 8, 8), colors.bg, 0);
                p.border(Rect::new(gx, gy + 1, 8, 8), Color::TRANSPARENT, 0, glyph);
            }
            "maximize" => p.border(Rect::new(gx, gy, 10, 10), Color::TRANSPARENT, 0, glyph),
            _ => {
                p.line(vec![(gx, gy), (gx + 10, gy + 10)], glyph, 1);
                p.line(vec![(gx + 10, gy), (gx, gy + 10)], glyph, 1);
            }
        }
        p.region(hit, &w.action(verb), label);
    }
}
