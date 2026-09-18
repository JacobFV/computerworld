//! The workbench, painted in the Dark Modern or Light Modern theme: activity bar, side
//! bar, editor group with tabs and breadcrumbs, the panel, the status bar and the
//! overlays. Geometry is computed here and nowhere else; every click target carries the
//! numbers the model needs to resolve it, so a click lands where it was painted.
use super::commands::{self, display_keys, MENUS};
use super::frame;
use super::icons;
use super::problems::Severity;
use super::syntax::{self, Language, Span};
use super::{basename, Focus, PanelTab, Platform, QuickMode, Tab, TabKind, View, Workbench};
use crate::desktop_scene::{shared::Align, Painter};
use cw_scene::{text_cell, Color, Primitive, Rect};

const ACT_W: u32 = 48;
const STATUS_H: u32 = 22;
const TABS_H: u32 = 35;
const CRUMBS_H: u32 = 22;
const ROW: u32 = 22;
const TERM_SIZE: u16 = 13;
/// An icon painter: (painter, x, y, size, colour).
type Draw = fn(&mut Painter, i32, i32, u32, Color);
/// A terminal row: coloured runs, and the command decoration (success or failure).
type TermLine = (Vec<(String, Color)>, Option<bool>);

/// Editor cell: the monospace advance at this font size and VS Code's line height
/// (1.5 × the font size on macOS, 1.35 × elsewhere).
pub fn cell(size: u16, platform: Platform) -> (u32, u32) {
    let (w, h) = text_cell(size);
    let ratio = if platform == Platform::Mac { 150 } else { 135 };
    (w, (u32::from(size) * ratio).div_ceil(100).max(h))
}
pub fn terminal_cell() -> (u32, u32) {
    (text_cell(TERM_SIZE).0, text_cell(TERM_SIZE).1 + 1)
}

fn hex(v: u32) -> Color {
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
pub struct Pal {
    pub editor: Color,
    pub fg: Color,
    pub side: Color,
    pub activity: Color,
    pub activity_fg: Color,
    pub activity_dim: Color,
    pub border: Color,
    pub accent: Color,
    pub tab_active: Color,
    pub tab_inactive: Color,
    pub tab_fg: Color,
    pub tab_dim: Color,
    pub input: Color,
    pub input_border: Color,
    pub placeholder: Color,
    pub line_no: Color,
    pub line_no_active: Color,
    pub line_highlight: Color,
    pub selection: Color,
    pub match_other: Color,
    pub match_current: Color,
    pub list_active: Color,
    pub list_inactive: Color,
    pub widget: Color,
    pub widget_border: Color,
    pub quick: Color,
    pub desc: Color,
    pub badge: Color,
    pub badge_fg: Color,
    pub button_fg: Color,
    pub error: Color,
    pub warning: Color,
    pub menu: Color,
    pub modified: Color,
    pub added: Color,
    pub deleted: Color,
    pub untracked: Color,
    pub link: Color,
    pub caret: Color,
    pub slider: Color,
    pub bracket: Color,
    pub term_green: Color,
    pub term_red: Color,
    pub option_on: Color,
}
pub fn pal(dark: bool) -> Pal {
    if dark {
        Pal {
            editor: hex(0x1F1F1F),
            fg: hex(0xCCCCCC),
            side: hex(0x181818),
            activity: hex(0x181818),
            activity_fg: hex(0xD7D7D7),
            activity_dim: hex(0x868686),
            border: hex(0x2B2B2B),
            accent: hex(0x0078D4),
            tab_active: hex(0x1F1F1F),
            tab_inactive: hex(0x181818),
            tab_fg: hex(0xFFFFFF),
            tab_dim: hex(0x9D9D9D),
            input: hex(0x313131),
            input_border: hex(0x3C3C3C),
            placeholder: hex(0x989898),
            line_no: hex(0x6E7681),
            line_no_active: hex(0xCCCCCC),
            line_highlight: hex(0x282828),
            selection: hex(0x264F78),
            match_other: Color(234, 92, 0, 85),
            match_current: hex(0x9E6A03),
            list_active: hex(0x04395E),
            list_inactive: hex(0x37373D),
            widget: hex(0x202020),
            widget_border: hex(0x313131),
            quick: hex(0x222222),
            desc: hex(0x9D9D9D),
            badge: hex(0x616161),
            badge_fg: hex(0xF8F8F8),
            button_fg: hex(0xFFFFFF),
            error: hex(0xF14C4C),
            warning: hex(0xCCA700),
            menu: hex(0x1F1F1F),
            modified: hex(0xE2C08D),
            added: hex(0x81B88B),
            deleted: hex(0xC74E39),
            untracked: hex(0x73C991),
            link: hex(0x2AAAFF),
            caret: hex(0xAEAFAD),
            slider: Color(121, 121, 121, 102),
            bracket: hex(0x888888),
            term_green: hex(0x23D18B),
            term_red: hex(0xF14C4C),
            option_on: Color(36, 137, 219, 130),
        }
    } else {
        Pal {
            editor: hex(0xFFFFFF),
            fg: hex(0x3B3B3B),
            side: hex(0xF8F8F8),
            activity: hex(0xF8F8F8),
            activity_fg: hex(0x1F1F1F),
            activity_dim: hex(0x616161),
            border: hex(0xE5E5E5),
            accent: hex(0x005FB8),
            tab_active: hex(0xFFFFFF),
            tab_inactive: hex(0xF8F8F8),
            tab_fg: hex(0x3B3B3B),
            tab_dim: hex(0x616161),
            input: hex(0xFFFFFF),
            input_border: hex(0xCECECE),
            placeholder: hex(0x767676),
            line_no: hex(0x6E7681),
            line_no_active: hex(0x171184),
            line_highlight: hex(0xEEEEEE),
            selection: hex(0xADD6FF),
            match_other: Color(234, 92, 0, 85),
            match_current: hex(0xA8AC94),
            list_active: hex(0xE8E8E8),
            list_inactive: hex(0xE4E6F1),
            widget: hex(0xF8F8F8),
            widget_border: hex(0xE5E5E5),
            quick: hex(0xF8F8F8),
            desc: hex(0x616161),
            badge: hex(0xCCCCCC),
            badge_fg: hex(0x3B3B3B),
            button_fg: hex(0xFFFFFF),
            error: hex(0xE51400),
            warning: hex(0xBF8803),
            menu: hex(0xFFFFFF),
            modified: hex(0x895503),
            added: hex(0x587C0C),
            deleted: hex(0xAD0707),
            untracked: hex(0x007100),
            link: hex(0x0066BF),
            caret: hex(0x000000),
            slider: Color(100, 100, 100, 102),
            bracket: hex(0xB9B9B9),
            term_green: hex(0x00BC00),
            term_red: hex(0xCD3131),
            option_on: hex(0xBED6ED),
        }
    }
}

fn label(p: &mut Painter, x: i32, y: i32, w: u32, text: &str, size: u16, c: Color) -> u32 {
    if w == 0 {
        return 0;
    }
    p.label(x, y, w, text, size, c, false, Align::Left)
}
fn bold(p: &mut Painter, x: i32, y: i32, w: u32, text: &str, size: u16, c: Color) -> u32 {
    if w == 0 {
        return 0;
    }
    p.label(x, y, w, text, size, c, true, Align::Left)
}
fn mono(p: &mut Painter, x: i32, y: i32, text: &str, size: u16, c: Color) {
    if text.is_empty() {
        return;
    }
    let (cw, ch) = text_cell(size);
    p.node(
        Rect::new(x, y, cw * text.chars().count() as u32 + 2, ch + 2),
        Primitive::Text {
            text: text.into(),
            size,
            color: c,
        },
        None,
    );
}
/// A control the model refuses right now: drawn, and announced disabled with why.
fn inert(p: &mut Painter, r: Rect, why: &str) {
    p.box_(r, Color::TRANSPARENT, 0);
    p.disabled(why);
}
fn icon_button(p: &mut Painter, r: Rect, target: &str, label: &str, enabled: Result<(), &str>) {
    match enabled {
        Ok(()) => p.region(r, target, label),
        Err(why) => inert(p, r, &format!("{label}: {why}")),
    }
}
fn badge(p: &mut Painter, pal: &Pal, x: i32, y: i32, n: usize, bg: Color, fg: Color) -> u32 {
    let text = if n > 999 {
        "999+".to_owned()
    } else {
        n.to_string()
    };
    let w = (p.measure(&text, 11, false) + 10).max(18);
    p.box_(Rect::new(x, y, w, 18), bg, 9);
    p.label(x, y + 1, w, &text, 11, fg, false, Align::Center);
    let _ = pal;
    w
}
fn button(p: &mut Painter, pal: &Pal, r: Rect, text: &str, target: &str) {
    p.button(r, pal.accent, 2, target, text);
    p.label(
        r.x,
        r.y + (r.height as i32 - 18) / 2 + 1,
        r.width,
        text,
        13,
        pal.button_fg,
        false,
        Align::Center,
    );
}
#[allow(clippy::too_many_arguments)]
fn input_box(
    p: &mut Painter,
    pal: &Pal,
    r: Rect,
    value: &str,
    placeholder: &str,
    focused: bool,
    target: &str,
    label_text: &str,
) {
    p.border(
        r,
        pal.input,
        2,
        if focused {
            pal.accent
        } else {
            pal.input_border
        },
    );
    p.region(r, target, label_text);
    let (text, color) = if value.is_empty() {
        (placeholder, pal.placeholder)
    } else {
        (value, pal.fg)
    };
    let shown = label(
        p,
        r.x + 6,
        r.y + (r.height as i32 - 18) / 2 + 1,
        r.width.saturating_sub(12),
        text,
        13,
        color,
    );
    if focused {
        let x = if value.is_empty() {
            r.x + 6
        } else {
            r.x + 6 + shown as i32
        };
        p.box_(
            Rect::new(
                x.min(r.x + r.width as i32 - 4),
                r.y + 4,
                1,
                r.height.saturating_sub(8),
            ),
            pal.fg,
            0,
        );
    }
}
fn toggle(p: &mut Painter, pal: &Pal, r: Rect, text: &str, on: bool, target: &str, name: &str) {
    if on {
        p.border(r, pal.option_on, 3, pal.accent);
    }
    p.button(r, Color::TRANSPARENT, 3, target, name);
    p.label(r.x, r.y + 2, r.width, text, 11, pal.fg, on, Align::Center);
}
fn zigzag(p: &mut Painter, x0: i32, x1: i32, y: i32, c: Color) {
    let mut pts = vec![];
    let mut x = x0;
    let mut up = false;
    while x <= x1 {
        pts.push((x, if up { y - 2 } else { y }));
        up = !up;
        x += 2;
    }
    if pts.len() > 1 {
        p.line(pts, c, 1);
    }
}

pub fn render(app: &Workbench, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let (w, h) = (env.width, env.height);
    let pal = pal(app.settings.dark);
    p.scene.background = pal.editor;
    let status_y = h.saturating_sub(STATUS_H) as i32;
    let body_h = h.saturating_sub(STATUS_H);
    // Activity bar.
    p.box_(Rect::new(0, 0, ACT_W, body_h), pal.activity, 0);
    p.vline(ACT_W as i32 - 1, 0, body_h, pal.border);
    activity(app, p, &pal, body_h);
    let side_w = if app.sidebar && w >= 420 {
        (w * 22 / 100).clamp(170, 300)
    } else {
        0
    };
    if side_w > 0 {
        let r = Rect::new(ACT_W as i32, 0, side_w, body_h);
        p.box_(r, pal.side, 0);
        p.vline(r.x + side_w as i32, 0, body_h, pal.border);
        sidebar(app, p, &pal, r);
    }
    let mx = ACT_W as i32 + side_w as i32 + i32::from(side_w > 0);
    let mw = w.saturating_sub(mx as u32);
    let panel_h = if app.panel_open {
        (body_h * 38 / 100)
            .clamp(120, 340)
            .min(body_h.saturating_sub(120))
    } else {
        0
    };
    let group = Rect::new(mx, 0, mw, body_h.saturating_sub(panel_h));
    editor_group(app, p, &pal, group);
    if panel_h > 0 {
        let r = Rect::new(mx, group.height as i32, mw, panel_h);
        panel(app, p, &pal, r);
    }
    status_bar(app, p, &pal, Rect::new(0, status_y, w, STATUS_H));
    if let Some(notice) = &app.notice {
        let nw = 380.min(w.saturating_sub(20));
        let r = Rect::new(w as i32 - nw as i32 - 10, status_y - 70, nw, 60);
        p.drop_shadow(r, 4, 8, 90, 2);
        p.border(r, pal.menu, 4, pal.border);
        p.symbol("info", r.x + 12, r.y + 12, 16, pal.link);
        p.paragraph(
            r.x + 38,
            r.y + 10,
            nw.saturating_sub(74),
            notice,
            13,
            pal.fg,
        );
        let close = Rect::new(r.x + nw as i32 - 30, r.y + 8, 22, 22);
        p.symbol("close", close.x + 5, close.y + 5, 12, pal.fg);
        p.region(close, "code:notice-close", "Clear Notification");
    }
    if let Some(menu) = &app.menu {
        menu_overlay(app, p, &pal, menu, w, h);
    }
    if app.quick.is_some() {
        quick_overlay(app, p, &pal, w, h);
    }
    if let Some(dialog) = &app.dialog {
        p.box_(Rect::new(0, 0, w, h), Color(0, 0, 0, 76), 0);
        let dw = 440.min(w.saturating_sub(20));
        let lines = (p.measure(&dialog.detail, 13, false) / dw.saturating_sub(80).max(1))
            + 1
            + dialog.detail.matches('\n').count() as u32;
        let dh = 110 + lines * 18;
        let r = Rect::new(
            (w as i32 - dw as i32) / 2,
            (h as i32 - dh as i32) / 3,
            dw,
            dh,
        );
        p.drop_shadow(r, 6, 16, 120, 4);
        p.border(r, pal.menu, 6, pal.widget_border);
        icons::warning(p, r.x + 20, r.y + 22, 28, pal.warning);
        bold(
            p,
            r.x + 64,
            r.y + 20,
            dw.saturating_sub(84),
            &dialog.message,
            13,
            pal.fg,
        );
        p.paragraph(
            r.x + 64,
            r.y + 44,
            dw.saturating_sub(84),
            &dialog.detail,
            13,
            pal.desc,
        );
        let mut x = r.x + dw as i32 - 16;
        for (i, (text, _)) in dialog.buttons.iter().enumerate().rev() {
            let bw = p.measure(text, 13, false) + 28;
            x -= bw as i32;
            let b = Rect::new(x, r.y + dh as i32 - 44, bw, 28);
            if i == 0 {
                button(p, &pal, b, text, &format!("code:dialog:{i}"));
            } else {
                p.button(b, Color::TRANSPARENT, 2, &format!("code:dialog:{i}"), text);
                p.border(b, Color::TRANSPARENT, 2, pal.input_border);
                p.label(b.x, b.y + 5, bw, text, 13, pal.fg, false, Align::Center);
            }
            x -= 8;
        }
    }
}

fn activity(app: &Workbench, p: &mut Painter, pal: &Pal, body_h: u32) {
    for (i, (view, name, label_text)) in [
        (View::Explorer, "explorer", "Explorer (Ctrl+Shift+E)"),
        (View::Search, "search", "Search (Ctrl+Shift+F)"),
        (View::Scm, "scm", "Source Control (Ctrl+Shift+G)"),
        (View::Run, "run", "Run and Debug (Ctrl+Shift+D)"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = i as i32 * 48;
        let on = app.sidebar && app.view == view;
        let c = if on {
            pal.activity_fg
        } else {
            pal.activity_dim
        };
        if on {
            p.box_(Rect::new(0, y, 2, 48), pal.accent, 0);
        }
        let (x0, y0) = (12, y + 12);
        match view {
            View::Explorer => icons::files(p, x0, y0, 24, c),
            View::Search => p.symbol("search", x0, y0, 24, c),
            View::Scm => icons::scm(p, x0, y0, 24, c),
            View::Run => icons::debug(p, x0, y0, 24, c),
        }
        if view == View::Scm {
            let n = app.scm.changes.len() + app.scm.staged.len();
            if n > 0 {
                let text = n.to_string();
                let bw = (p.measure(&text, 9, false) + 8).max(16);
                p.box_(
                    Rect::new(38 - bw as i32 / 2 + 2, y + 26, bw, 16),
                    pal.accent,
                    8,
                );
                p.label(
                    38 - bw as i32 / 2 + 2,
                    y + 27,
                    bw,
                    &text,
                    9,
                    Color::WHITE,
                    false,
                    Align::Center,
                );
            }
        }
        p.region(
            Rect::new(0, y, ACT_W, 48),
            &format!("code:activity:{name}"),
            label_text,
        );
    }
    let gear = Rect::new(0, body_h as i32 - 48, ACT_W, 48);
    p.symbol(
        "gear",
        12,
        gear.y + 12,
        24,
        if app.menu.as_deref() == Some("manage") {
            pal.activity_fg
        } else {
            pal.activity_dim
        },
    );
    p.region(gear, "code:menu:manage", "Manage");
}

fn section_title(p: &mut Painter, pal: &Pal, r: Rect, title: &str) {
    label(
        p,
        r.x + 20,
        r.y + 10,
        r.width.saturating_sub(100),
        title,
        11,
        pal.fg,
    );
}

fn sidebar(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    match app.view {
        View::Explorer => explorer(app, p, pal, r),
        View::Search => search_view(app, p, pal, r),
        View::Scm => scm_view(app, p, pal, r),
        View::Run => run_view(app, p, pal, r),
    }
}

fn no_folder(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect, y: i32) {
    let h = p.paragraph(
        r.x + 20,
        y,
        r.width.saturating_sub(40),
        "You have not yet opened a folder.",
        13,
        pal.fg,
    );
    button(
        p,
        pal,
        Rect::new(r.x + 20, y + h as i32 + 12, r.width.saturating_sub(40), 28),
        "Open Folder",
        "code:cmd:workbench.action.files.openFolder",
    );
    let _ = app;
}

fn explorer(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    section_title(p, pal, r, "EXPLORER");
    let Some(folder) = app.workspace() else {
        bold(
            p,
            r.x + 8,
            r.y + 40,
            r.width.saturating_sub(16),
            "NO FOLDER OPENED",
            11,
            pal.fg,
        );
        no_folder(app, p, pal, r, r.y + 66);
        return;
    };
    // The folder's section header, with the Explorer's own title actions.
    let hy = r.y + 35;
    p.symbol("chevron-down", r.x + 4, hy + 4, 14, pal.fg);
    bold(
        p,
        r.x + 20,
        hy + 3,
        r.width.saturating_sub(120),
        &basename(folder).to_uppercase(),
        11,
        pal.fg,
    );
    let actions: [(&str, &str, Draw); 4] = [
        ("explorer.newFile", "New File...", icons::new_file),
        ("explorer.newFolder", "New Folder...", icons::new_folder),
        (
            "workbench.files.action.refreshFilesExplorer",
            "Refresh Explorer",
            |p, x, y, s, c| p.symbol("reload", x, y, s, c),
        ),
        (
            "workbench.files.action.collapseExplorerFolders",
            "Collapse Folders in Explorer",
            icons::collapse,
        ),
    ];
    for (i, (id, name, draw)) in actions.into_iter().enumerate() {
        let b = Rect::new(r.x + r.width as i32 - 96 + i as i32 * 22, hy, 22, 22);
        draw(p, b.x + 3, b.y + 3, 16, pal.fg);
        icon_button(p, b, &format!("code:cmd:{id}"), name, app.enabled(id));
    }
    let top = hy + 22;
    p.region(
        Rect::new(
            r.x,
            top,
            r.width,
            r.height.saturating_sub((top - r.y) as u32),
        ),
        "code:explorer",
        "Files Explorer",
    );
    let mut rows = app.tree_rows();
    // The inline name box sits where the new entry will appear.
    let inline_at = app.inline.as_ref().map(|i| match &i.from {
        Some(from) => rows.iter().position(|row| &row.0 == from).unwrap_or(0),
        None if i.parent.is_empty() => 0,
        None => rows
            .iter()
            .position(|row| row.0 == i.parent)
            .map_or(0, |p| p + 1),
    });
    let changed = |rel: &str| {
        app.scm
            .changes
            .iter()
            .chain(&app.scm.staged)
            .find(|(_, p)| p == rel)
            .map(|(s, _)| *s)
    };
    // The tree scrolls under its header; rows out of view are counted, not painted.
    let pane = p.pane(
        "explorer",
        Rect::new(
            r.x,
            top,
            r.width,
            r.height.saturating_sub((top - r.y) as u32).max(1),
        ),
    );
    let mut y = pane.top();
    let mut index = 0;
    rows.truncate(2000);
    let mut i = 0;
    while i <= rows.len() {
        if let (Some(at), Some(inline)) = (inline_at, &app.inline) {
            if at == index && (inline.from.is_none() || i < rows.len()) {
                let depth = if inline.parent.is_empty() {
                    0
                } else {
                    inline.parent.matches('/').count() + 1
                };
                let x = r.x + 8 + depth as i32 * 8 + 16;
                let bx = Rect::new(
                    x,
                    y + 1,
                    (r.x + r.width as i32 - x - 6).max(40) as u32,
                    ROW - 2,
                );
                input_box(
                    p,
                    pal,
                    bx,
                    &inline.value,
                    "",
                    app.focus == super::Focus::Inline,
                    "code:inline",
                    "File name",
                );
                y += ROW as i32;
                index += 1;
                if inline.from.is_some() {
                    i += 1;
                    continue;
                }
            }
        }
        let Some((rel, depth, dir)) = rows.get(i).cloned() else {
            break;
        };
        if !pane.shows(y, ROW) {
            y += ROW as i32;
            index += 1;
            i += 1;
            continue;
        }
        let row = Rect::new(r.x, y, r.width, ROW);
        let selected = app.selected.as_deref() == Some(rel.as_str());
        let open = app.active_tab().and_then(|t| app.rel(&t.path)).as_deref() == Some(rel.as_str());
        if selected {
            let focused = app.focus == super::Focus::Explorer;
            p.box_(
                row,
                if focused {
                    pal.list_active
                } else {
                    pal.list_inactive
                },
                0,
            );
            if focused {
                p.border(row, Color::TRANSPARENT, 0, pal.accent);
            }
        } else if open {
            p.box_(row, pal.list_inactive, 0);
        }
        let x = r.x + 8 + depth as i32 * 8;
        let name = basename(&rel);
        let status = changed(&rel);
        let color = match status {
            Some('M') => pal.modified,
            Some('A') => pal.added,
            Some('U') => pal.untracked,
            Some('D') => pal.deleted,
            _ => pal.fg,
        };
        if dir {
            let chevron = if app.expanded.contains(&rel) {
                "chevron-down"
            } else {
                "chevron-right"
            };
            p.symbol(chevron, x, y + 4, 14, pal.fg);
        } else {
            let (glyph, c) = icons::file_glyph(Language::from_path(name), name);
            let gw = p.measure(glyph, 10, true);
            p.label(
                x + 16 + (16 - gw as i32) / 2,
                y + 4,
                18,
                glyph,
                10,
                c,
                true,
                Align::Left,
            );
        }
        let tx = x + if dir { 18 } else { 36 };
        let room = (r.x + r.width as i32 - tx - 24).max(0) as u32;
        label(p, tx, y + 3, room, name, 13, color);
        if let Some(s) = status.filter(|_| !dir) {
            label(
                p,
                r.x + r.width as i32 - 20,
                y + 3,
                14,
                &s.to_string(),
                12,
                color,
            );
        }
        p.region(row, &format!("code:tree:{rel}"), name);
        y += ROW as i32;
        index += 1;
        i += 1;
    }
    if app.truncated {
        label(
            p,
            r.x + 20,
            y + 4,
            r.width.saturating_sub(28),
            "Only the first 4000 entries are shown.",
            12,
            pal.desc,
        );
        y += ROW as i32;
    }
    let extent = (y - pane.top()) as u32 + ROW;
    p.end_pane(pane, Some(extent));
}

fn search_view(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    section_title(p, pal, r, "SEARCH");
    let s = &app.search;
    for (i, (target, name, draw)) in [
        ("code:search:clear", "Clear Search Results", 0),
        ("code:search:collapse", "Collapse All", 1),
    ]
    .into_iter()
    .enumerate()
    {
        let b = Rect::new(r.x + r.width as i32 - 52 + i as i32 * 24, r.y + 7, 22, 22);
        if draw == 0 {
            p.symbol("close", b.x + 5, b.y + 5, 12, pal.fg);
        } else {
            icons::collapse(p, b.x + 3, b.y + 3, 16, pal.fg);
        }
        if s.query.is_empty() && s.results.is_empty() {
            inert(p, b, &format!("{name}: there are no results"));
        } else {
            p.region(b, target, name);
        }
    }
    let x = r.x + 22;
    let iw = r.width.saturating_sub(30);
    let chev = Rect::new(r.x + 4, r.y + 38, 16, if s.show_replace { 56 } else { 26 });
    p.symbol(
        if s.show_replace {
            "chevron-down"
        } else {
            "chevron-right"
        },
        chev.x,
        chev.y + 5,
        14,
        pal.fg,
    );
    p.region(chev, "code:search:toggle-replace", "Toggle Replace");
    let q = Rect::new(x, r.y + 38, iw, 26);
    input_box(
        p,
        pal,
        Rect::new(q.x, q.y, iw.saturating_sub(72), 26),
        &s.query,
        "Search",
        app.focus == Focus::Search,
        "code:search-input",
        "Search",
    );
    p.border(
        Rect::new(q.x + iw.saturating_sub(72) as i32 - 1, q.y, 72, 26),
        pal.input,
        0,
        if app.focus == Focus::Search {
            pal.accent
        } else {
            pal.input_border
        },
    );
    for (i, (name, text, on, title)) in [
        ("case", "Aa", s.opts.case, "Match Case"),
        ("word", "ab", s.opts.word, "Match Whole Word"),
        ("regex", ".*", s.opts.regex, "Use Regular Expression"),
    ]
    .into_iter()
    .enumerate()
    {
        let b = Rect::new(q.x + iw as i32 - 70 + i as i32 * 23, q.y + 3, 20, 20);
        toggle(p, pal, b, text, on, &format!("code:search:{name}"), title);
    }
    let mut y = q.y + 32;
    if s.show_replace {
        let rr = Rect::new(x, y - 2, iw.saturating_sub(28), 26);
        input_box(
            p,
            pal,
            rr,
            &s.replace,
            "Replace",
            app.focus == Focus::SearchReplace,
            "code:search-replace-input",
            "Replace",
        );
        let b = Rect::new(x + iw as i32 - 24, y + 1, 22, 22);
        icons::replace_all(p, b.x + 3, b.y + 3, 16, pal.fg);
        if s.results.is_empty() {
            inert(p, b, "Replace All: there are no results");
        } else {
            p.region(b, "code:search-replace-all", "Replace All");
        }
        y += 30;
    }
    let summary = if let Some(e) = &s.error {
        Some((e.clone(), pal.error))
    } else if s.searched && s.results.is_empty() {
        Some(("No results found. Review your settings for configured exclusions and check your gitignore files.".to_owned(), pal.desc))
    } else if !s.results.is_empty() {
        let files = s.results.len();
        let total = s.total();
        Some((
            format!(
                "{total} result{} in {files} file{}",
                if total == 1 { "" } else { "s" },
                if files == 1 { "" } else { "s" }
            ),
            pal.desc,
        ))
    } else if app.workspace().is_none() {
        Some(("Open a folder to search its files.".to_owned(), pal.desc))
    } else {
        None
    };
    if let Some((text, c)) = summary {
        let hh = p.paragraph(r.x + 22, y + 2, r.width.saturating_sub(30), &text, 12, c);
        y += hh as i32 + 8;
    }
    // The results tree scrolls; rows out of view are counted, not painted.
    let pane = p.pane(
        "search",
        Rect::new(r.x, y, r.width, (r.y + r.height as i32 - y).max(1) as u32),
    );
    let first = y;
    y = pane.top();
    for (fi, file) in s.results.iter().enumerate() {
        let collapsed = s.collapsed.contains(&file.path);
        let row = Rect::new(r.x, y, r.width, ROW);
        p.symbol(
            if collapsed {
                "chevron-right"
            } else {
                "chevron-down"
            },
            r.x + 6,
            y + 4,
            14,
            pal.fg,
        );
        let name = basename(&file.path);
        let (glyph, gc) = icons::file_glyph(Language::from_path(name), name);
        p.label(r.x + 22, y + 4, 18, glyph, 10, gc, true, Align::Left);
        let nw = label(
            p,
            r.x + 42,
            y + 3,
            r.width.saturating_sub(90),
            name,
            13,
            pal.fg,
        );
        let dir = super::parent(&file.path);
        label(
            p,
            r.x + 48 + nw as i32,
            y + 4,
            r.width.saturating_sub(96 + nw),
            dir,
            12,
            pal.desc,
        );
        badge(
            p,
            pal,
            r.x + r.width as i32 - 30,
            y + 2,
            file.hits.len(),
            pal.badge,
            pal.badge_fg,
        );
        p.region(row, &format!("code:search-file:{}", file.path), name);
        y += ROW as i32;
        if collapsed {
            continue;
        }
        for (hi, hit) in file.hits.iter().enumerate() {
            if !pane.shows(y, ROW) {
                y += ROW as i32;
                continue;
            }
            let row = Rect::new(r.x, y, r.width, ROW);
            // Show the match with some context before it, as the results tree does.
            let pre = &hit.preview[..hit.start.min(hit.preview.len())];
            let lead = pre.trim_start();
            let skip = lead.chars().count().saturating_sub(12);
            let before: String = lead.chars().skip(skip).collect();
            let matched = hit.preview.get(hit.start..hit.end).unwrap_or("");
            let after = hit.preview.get(hit.end..).unwrap_or("");
            let mut x = r.x + 40;
            let room = |x: i32| (r.x + r.width as i32 - x - 6).max(0) as u32;
            x += label(p, x, y + 3, room(x), &before, 13, pal.fg) as i32;
            let mw = p.measure(matched, 13, false).min(room(x));
            p.box_(Rect::new(x, y + 3, mw, 16), pal.match_other, 0);
            x += label(p, x, y + 3, room(x), matched, 13, pal.fg) as i32;
            label(p, x, y + 3, room(x), after, 13, pal.fg);
            p.region(
                row,
                &format!("code:search-result:{fi}:{hi}"),
                &format!("{}:{}", file.path, hit.line),
            );
            y += ROW as i32;
        }
    }
    let extent = (y - pane.top()) as u32 + ROW / 2;
    let _ = first;
    p.end_pane(pane, Some(extent));
}

fn scm_view(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    section_title(p, pal, r, "SOURCE CONTROL");
    if app.workspace().is_none() {
        no_folder(app, p, pal, r, r.y + 44);
        return;
    }
    let scm = &app.scm;
    let b = Rect::new(r.x + r.width as i32 - 30, r.y + 7, 22, 22);
    p.symbol("reload", b.x + 3, b.y + 3, 16, pal.fg);
    icon_button(
        p,
        b,
        "code:cmd:git.refresh",
        "Refresh",
        app.enabled("git.refresh"),
    );
    match scm.repo {
        None => {
            label(
                p,
                r.x + 20,
                r.y + 44,
                r.width.saturating_sub(28),
                "Scanning folder for Git repositories...",
                13,
                pal.desc,
            );
        }
        Some(false) => {
            let hh = p.paragraph(
                r.x + 20,
                r.y + 44,
                r.width.saturating_sub(40),
                "The folder currently open doesn't have a git repository. You can initialize a repository which will enable source control features powered by Git.",
                13,
                pal.fg,
            );
            button(
                p,
                pal,
                Rect::new(
                    r.x + 20,
                    r.y + 56 + hh as i32,
                    r.width.saturating_sub(40),
                    28,
                ),
                "Initialize Repository",
                "code:cmd:git.init",
            );
        }
        Some(true) => {
            let mut y = r.y + 40;
            let message = Rect::new(r.x + 12, y, r.width.saturating_sub(24), 28);
            let placeholder = format!(
                "Message ({} to commit on '{}')",
                display_keys("Ctrl+Enter", app.platform == Platform::Mac),
                scm.branch
            );
            let first_line = scm.message.lines().last().unwrap_or("");
            input_box(
                p,
                pal,
                message,
                if scm.message.is_empty() {
                    ""
                } else {
                    first_line
                },
                &placeholder,
                app.focus == Focus::ScmMessage,
                "code:scm-message",
                "Commit message",
            );
            y += 34;
            let commit = Rect::new(r.x + 12, y, r.width.saturating_sub(24), 28);
            match app.enabled("git.commit") {
                Ok(()) => {
                    p.button(commit, pal.accent, 2, "code:cmd:git.commit", "Commit");
                    p.symbol(
                        "check",
                        commit.x + commit.width as i32 / 2 - 38,
                        commit.y + 7,
                        14,
                        pal.button_fg,
                    );
                    p.label(
                        commit.x + 12,
                        commit.y + 6,
                        commit.width,
                        "Commit",
                        13,
                        pal.button_fg,
                        false,
                        Align::Center,
                    );
                }
                Err(why) => {
                    p.box_(
                        commit,
                        Color(pal.accent.0, pal.accent.1, pal.accent.2, 110),
                        2,
                    );
                    p.disabled(&format!("Commit: {why}"));
                    p.label(
                        commit.x,
                        commit.y + 6,
                        commit.width,
                        "Commit",
                        13,
                        Color(255, 255, 255, 150),
                        false,
                        Align::Center,
                    );
                }
            }
            y += 38;
            if let Some(err) = &scm.error {
                let hh = p.paragraph(r.x + 12, y, r.width.saturating_sub(24), err, 12, pal.error);
                y += hh as i32 + 6;
            }
            let groups = [
                ("Staged Changes", &scm.staged, true),
                ("Changes", &scm.changes, false),
            ];
            for (title, items, staged) in groups {
                if items.is_empty() && staged {
                    continue;
                }
                p.symbol("chevron-down", r.x + 4, y + 4, 14, pal.fg);
                label(
                    p,
                    r.x + 20,
                    y + 3,
                    r.width.saturating_sub(90),
                    title,
                    13,
                    pal.fg,
                );
                if !staged {
                    let b = Rect::new(r.x + r.width as i32 - 58, y, 22, 22);
                    p.symbol("plus", b.x + 4, b.y + 4, 14, pal.fg);
                    icon_button(
                        p,
                        b,
                        "code:cmd:git.stageAll",
                        "Stage All Changes",
                        app.enabled("git.stageAll"),
                    );
                }
                badge(
                    p,
                    pal,
                    r.x + r.width as i32 - 30,
                    y + 2,
                    items.len(),
                    pal.badge,
                    pal.badge_fg,
                );
                y += ROW as i32;
                for (status, path) in items.iter() {
                    if y > r.y + r.height as i32 - ROW as i32 {
                        return;
                    }
                    let name = basename(path);
                    let color = match status {
                        'M' => pal.modified,
                        'D' => pal.deleted,
                        'A' => pal.added,
                        'U' => pal.untracked,
                        _ => pal.fg,
                    };
                    let (glyph, gc) = icons::file_glyph(Language::from_path(name), name);
                    p.label(r.x + 22, y + 4, 18, glyph, 10, gc, true, Align::Left);
                    let row = Rect::new(
                        r.x,
                        y,
                        r.width.saturating_sub(if staged { 0 } else { 50 }),
                        ROW,
                    );
                    let nw = label(
                        p,
                        r.x + 42,
                        y + 3,
                        r.width.saturating_sub(110),
                        name,
                        13,
                        if *status == 'D' { pal.deleted } else { pal.fg },
                    );
                    label(
                        p,
                        r.x + 48 + nw as i32,
                        y + 4,
                        r.width.saturating_sub(116 + nw),
                        super::parent(path),
                        12,
                        pal.desc,
                    );
                    label(
                        p,
                        r.x + r.width as i32 - 18,
                        y + 3,
                        14,
                        &status.to_string(),
                        12,
                        color,
                    );
                    if *status == 'D' {
                        inert(p, row, &format!("{name} was deleted"));
                    } else {
                        p.region(row, &format!("code:scm-open:{path}"), name);
                    }
                    if !staged {
                        let b = Rect::new(r.x + r.width as i32 - 44, y, 22, 22);
                        p.symbol("plus", b.x + 4, b.y + 4, 14, pal.fg);
                        p.region(b, &format!("code:scm-stage:{path}"), "Stage Changes");
                    }
                    y += ROW as i32;
                }
            }
        }
    }
}

fn run_view(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    section_title(p, pal, r, "RUN AND DEBUG");
    let mut y = r.y + 44;
    let runnable = app.enabled("workbench.action.debug.run");
    let b = Rect::new(r.x + 20, y, r.width.saturating_sub(40), 28);
    match runnable {
        Ok(()) => button(
            p,
            pal,
            b,
            "Run Active File",
            "code:cmd:workbench.action.debug.run",
        ),
        Err(why) => {
            p.box_(b, Color(pal.accent.0, pal.accent.1, pal.accent.2, 110), 2);
            p.disabled(&format!("Run Active File: {why}"));
            p.label(
                b.x,
                b.y + 6,
                b.width,
                "Run Active File",
                13,
                Color(255, 255, 255, 150),
                false,
                Align::Center,
            );
        }
    }
    y += 40;
    let hh = p.paragraph(
        r.x + 20,
        y,
        r.width.saturating_sub(40),
        "Runs the file in the integrated terminal with the machine's own python3, node or bash. There is no debug adapter on this machine, so breakpoints are not available.",
        12,
        pal.desc,
    );
    y += hh as i32 + 14;
    if let Some((command, code)) = &app.last_run {
        bold(
            p,
            r.x + 20,
            y,
            r.width.saturating_sub(40),
            "LAST RUN",
            11,
            pal.fg,
        );
        label(
            p,
            r.x + 20,
            y + 20,
            r.width.saturating_sub(40),
            command,
            12,
            pal.fg,
        );
        label(
            p,
            r.x + 20,
            y + 38,
            r.width.saturating_sub(40),
            &format!("Exit code {code}"),
            12,
            if *code == 0 { pal.untracked } else { pal.error },
        );
    }
}

fn editor_group(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    p.box_(Rect::new(r.x, r.y, r.width, TABS_H), pal.tab_inactive, 0);
    let Some(active) = app.active_tab() else {
        p.box_(r, pal.editor, 0);
        p.region(r, "code:welcome", "Editor area");
        if app.workspace().is_none() {
            welcome(app, p, pal, r);
        } else {
            watermark(app, p, pal, r);
        }
        return;
    };
    tabs(app, p, pal, Rect::new(r.x, r.y, r.width, TABS_H));
    let mut top = r.y + TABS_H as i32;
    if active.kind == TabKind::File {
        breadcrumbs(app, p, pal, active, Rect::new(r.x, top, r.width, CRUMBS_H));
        top += CRUMBS_H as i32;
    }
    let body = Rect::new(
        r.x,
        top,
        r.width,
        r.height.saturating_sub((top - r.y) as u32),
    );
    match active.kind {
        TabKind::Settings => settings_editor(app, p, pal, body),
        _ if active.error.is_some() => {
            p.box_(body, pal.editor, 0);
            let text = active.error.as_deref().unwrap_or("");
            let message = if text.contains("not found") || text.contains("No such") {
                "The editor could not be opened because the file was not found.".to_owned()
            } else {
                format!("The editor could not be opened: {text}")
            };
            p.paragraph(
                body.x + 40,
                body.y + 40,
                body.width.saturating_sub(80),
                &message,
                13,
                pal.fg,
            );
        }
        _ if !active.loaded => {
            p.box_(body, pal.editor, 0);
        }
        _ => text_editor(app, p, pal, active, body),
    }
}

fn welcome(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    let x = r.x + 48.min(r.width as i32 / 10);
    let y = r.y + 50;
    label(
        p,
        x,
        y,
        r.width.saturating_sub(60),
        "Visual Studio Code",
        30,
        pal.fg,
    );
    label(
        p,
        x,
        y + 44,
        r.width.saturating_sub(60),
        "Editing evolved",
        18,
        pal.desc,
    );
    label(p, x, y + 96, 200, "Start", 16, pal.fg);
    for (i, (text, id, draw)) in [
        ("New File...", "workbench.action.files.newUntitledFile", 0),
        ("Open Folder...", "workbench.action.files.openFolder", 1),
        ("Show All Commands", "workbench.action.showCommands", 2),
    ]
    .into_iter()
    .enumerate()
    {
        let ry = y + 126 + i as i32 * 26;
        match draw {
            0 => icons::new_file(p, x, ry + 2, 16, pal.link),
            1 => p.symbol("folder", x, ry + 2, 16, pal.link),
            _ => p.symbol("search", x, ry + 2, 16, pal.link),
        }
        let tw = label(p, x + 24, ry, 260, text, 13, pal.link);
        p.region(
            Rect::new(x, ry - 2, tw + 28, 22),
            &format!("code:cmd:{id}"),
            text,
        );
    }
    let _ = app;
}

fn watermark(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    let size = (r.width.min(r.height) / 3).clamp(60, 220);
    let cx = r.x + r.width as i32 / 2;
    let top = r.y + (r.height as i32 - size as i32 - 150) / 2;
    let faint = if app.settings.dark {
        Color(255, 255, 255, 14)
    } else {
        Color(0, 0, 0, 14)
    };
    icons::logo(p, cx - size as i32 / 2, top, size, Some(faint));
    let mac = app.platform == Platform::Mac;
    for (i, id) in [
        "workbench.action.showCommands",
        "workbench.action.quickOpen",
        "workbench.action.findInFiles",
        "workbench.action.terminal.toggleTerminal",
        "workbench.action.files.openFolder",
    ]
    .into_iter()
    .enumerate()
    {
        let Some(c) = commands::command(id) else {
            continue;
        };
        let y = top + size as i32 + 24 + i as i32 * 26;
        let name = match id {
            "workbench.action.terminal.toggleTerminal" => "Toggle Terminal",
            "workbench.action.findInFiles" => "Find in Files",
            "workbench.action.files.openFolder" => "Open Folder",
            _ => c.title,
        };
        p.label(cx - 190, y, 180, name, 13, pal.desc, false, Align::Right);
        label(p, cx + 12, y, 200, &display_keys(c.keys, mac), 13, pal.desc);
        p.region(
            Rect::new(cx - 190, y - 3, 400, 22),
            &format!("code:cmd:{id}"),
            name,
        );
    }
}

fn tab_width(p: &Painter, tab: &Tab) -> u32 {
    (p.measure(&tab.name(), 13, false) + 26 + 10 + 28).clamp(80, 260)
}

fn tabs(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    p.hline(r.x, r.y + TABS_H as i32 - 1, r.width, pal.border);
    let actions = 36;
    let mut x = r.x;
    for (i, tab) in app.tabs.iter().enumerate() {
        let tw = tab_width(p, tab);
        if x + tw as i32 > r.x + r.width as i32 - actions {
            break;
        }
        let active = app.active == Some(i);
        let t = Rect::new(x, r.y, tw, TABS_H);
        p.box_(
            t,
            if active {
                pal.tab_active
            } else {
                pal.tab_inactive
            },
            0,
        );
        if active {
            p.box_(Rect::new(x, r.y, tw, 1), pal.accent, 0);
        } else {
            p.hline(x, r.y + TABS_H as i32 - 1, tw, pal.border);
        }
        p.vline(x + tw as i32 - 1, r.y, TABS_H, pal.border);
        p.region(t, &format!("code:tab:{i}"), &tab.name());
        let fg = if active { pal.tab_fg } else { pal.tab_dim };
        match tab.kind {
            TabKind::Settings => p.symbol("gear", x + 10, r.y + 10, 14, fg),
            _ => {
                let name = tab.name();
                let (glyph, c) = icons::file_glyph(tab.lang, &name);
                p.label(x + 8, r.y + 11, 20, glyph, 10, c, true, Align::Left);
            }
        }
        // A preview tab's title is set apart the way VS Code italicises it.
        let title = if tab.preview { pal.tab_dim } else { fg };
        label(
            p,
            x + 30,
            r.y + 9,
            tw.saturating_sub(64),
            &tab.name(),
            13,
            title,
        );
        let close = Rect::new(x + tw as i32 - 28, r.y + 7, 20, 20);
        if tab.dirty() {
            p.circle(close.x + 10, close.y + 10, 4, fg);
        } else {
            p.symbol("close", close.x + 4, close.y + 4, 12, fg);
        }
        p.region(
            close,
            &format!("code:tab-close:{i}"),
            &format!("Close {}", tab.name()),
        );
        x += tw as i32;
    }
    // Editor actions: Run for a file that can run.
    let run = Rect::new(r.x + r.width as i32 - 32, r.y + 6, 24, 22);
    let id = match app.active_tab().map(|t| t.lang) {
        Some(Language::Python) => "python.execInTerminal",
        _ => "workbench.action.terminal.runActiveFile",
    };
    if app.enabled(id).is_ok() {
        icons::play(p, run.x + 4, run.y + 3, 16, pal.fg);
        p.region(
            run,
            &format!("code:cmd:{id}"),
            if id.starts_with("python") {
                "Run Python File"
            } else {
                "Run Active File"
            },
        );
    }
}

fn breadcrumbs(app: &Workbench, p: &mut Painter, pal: &Pal, tab: &Tab, r: Rect) {
    p.box_(r, pal.editor, 0);
    let (parts, root): (Vec<&str>, bool) = match app.rel(&tab.path) {
        Some(rel) => (
            rel.split('/')
                .collect::<Vec<_>>()
                .into_iter()
                .map(|s| {
                    let start = tab.path.len() - rel.len();
                    let off = rel.find(s).unwrap_or(0);
                    &tab.path[start + off..start + off + s.len()]
                })
                .collect(),
            true,
        ),
        None => (tab.path.trim_start_matches('/').split('/').collect(), false),
    };
    let mut x = r.x + 20;
    let mut acc = String::new();
    for (i, part) in parts.iter().enumerate() {
        if x > r.x + r.width as i32 - 40 {
            break;
        }
        let last = i + 1 == parts.len();
        if last {
            let (glyph, c) = icons::file_glyph(tab.lang, part);
            p.label(x, r.y + 5, 18, glyph, 9, c, true, Align::Left);
            x += 18;
        }
        let tw = label(
            p,
            x,
            r.y + 3,
            (r.x + r.width as i32 - x).max(0) as u32,
            part,
            13,
            if last { pal.fg } else { pal.desc },
        );
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(part);
        if !last && root {
            p.region(
                Rect::new(x - 2, r.y, tw + 4, r.height),
                &format!("code:crumb:{acc}"),
                part,
            );
        }
        x += tw as i32 + 4;
        if !last {
            p.symbol("chevron-right", x, r.y + 5, 12, pal.desc);
            x += 16;
        }
    }
}

fn settings_editor(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    p.box_(r, pal.editor, 0);
    let x = r.x + 40;
    let w = r.width.saturating_sub(80);
    label(p, x, r.y + 16, w, "User", 13, pal.fg);
    p.box_(Rect::new(x, r.y + 38, 36, 1), pal.accent, 0);
    p.hline(r.x + 20, r.y + 40, r.width.saturating_sub(40), pal.border);
    let mut y = r.y + 56;
    let s = app.settings;
    // Four settings share the height there is; the description goes before it squeezes.
    let step = ((r.height.saturating_sub(84)) / 4).clamp(52, 96) as i32;
    let roomy = step >= 72;
    let mut row =
        |p: &mut Painter, title: &str, detail: &str, choices: Vec<(String, String, bool)>| {
            let mut cx = x;
            let first = title.find(": ").map_or(0, |i| i + 2);
            label(p, x, y, w, &title[..first], 13, pal.fg);
            bold(
                p,
                x + p.measure(&title[..first], 13, false) as i32,
                y,
                w,
                &title[first..],
                13,
                pal.fg,
            );
            if roomy {
                label(p, x, y + 22, w, detail, 13, pal.desc);
            }
            let chips = y + if roomy { 44 } else { 22 };
            for (text, target, on) in choices {
                let bw = p.measure(&text, 13, false) + 20;
                let b = Rect::new(cx, chips, bw, 24);
                p.border(
                    b,
                    if on { pal.option_on } else { pal.input },
                    2,
                    if on { pal.accent } else { pal.input_border },
                );
                if target.is_empty() {
                    inert(p, b, &format!("{title}: {text}"));
                } else {
                    p.region(b, &target, &text);
                }
                p.label(b.x, b.y + 3, bw, &text, 13, pal.fg, false, Align::Center);
                cx += bw as i32 + 6;
            }
            y += step;
        };
    row(
        p,
        "Workbench: Color Theme",
        "Specifies the color theme used in the workbench.",
        vec![
            (
                "Default Dark Modern".into(),
                "code:settings:theme:dark".into(),
                s.dark,
            ),
            (
                "Default Light Modern".into(),
                "code:settings:theme:light".into(),
                !s.dark,
            ),
        ],
    );
    row(
        p,
        "Editor: Font Size",
        &format!(
            "Controls the font size in pixels. Currently {}.",
            s.font_size
        ),
        vec![
            ("−".into(), "code:settings:font:-".into(), false),
            (s.font_size.to_string(), String::new(), false),
            ("+".into(), "code:settings:font:+".into(), false),
        ],
    );
    row(
        p,
        "Editor: Tab Size",
        "The number of spaces a tab is equal to.",
        [2, 4, 8]
            .into_iter()
            .map(|n| {
                (
                    n.to_string(),
                    format!("code:settings:tab:{n}"),
                    s.tab_size == n,
                )
            })
            .collect(),
    );
    row(
        p,
        "Editor: Word Wrap",
        "Controls how lines should wrap.",
        vec![
            ("off".into(), "code:settings:wrap".into(), !s.word_wrap),
            ("on".into(), "code:settings:wrap".into(), s.word_wrap),
        ],
    );
    label(
        p,
        x,
        y,
        w,
        &format!("Saved to {}", app.settings_path()),
        12,
        pal.desc,
    );
}

fn text_editor(app: &Workbench, p: &mut Painter, pal: &Pal, tab: &Tab, r: Rect) {
    p.box_(r, pal.editor, 0);
    let size = app.settings.font_size;
    let (cw, rh) = cell(size, app.platform);
    let ch = text_cell(size).1;
    let text = &tab.doc.text;
    let lines = tab.doc.line_count();
    let digits = lines.to_string().len().max(2) as u32;
    let gutter = 18 + digits * cw + 26;
    let tx = r.x + gutter as i32;
    let tw = r.width.saturating_sub(gutter + 14);
    let vc = (tw / cw).max(1) as usize;
    let cap = (r.height / rh).max(1) as usize;
    let wrap = if app.settings.word_wrap { vc } else { 0 };
    let rows = crate::editor_rows(text, wrap);
    let mut cursor = tab.doc.cursor.min(text.len());
    while !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let (vrow, vcol) = crate::editor_caret_cell(text, cursor, wrap);
    let mut first = tab.scroll.min(rows.len().saturating_sub(1));
    if tab.follow {
        if vrow < first {
            first = vrow;
        } else if vrow >= first + cap {
            first = vrow + 1 - cap;
        }
    }
    let hs = if wrap == 0 && vcol + 2 > vc {
        vcol + 10 - vc.min(vcol + 10)
    } else {
        0
    };
    let focused = app.focus == Focus::Editor;
    let spans = syntax::lines(tab.lang, text);
    let (sel_a, sel_b) = tab.doc.selection();
    let matches = if app.find.is_some() {
        app_find_matches(app, tab)
    } else {
        vec![]
    };
    let bracket = tab.doc.matching_bracket();
    let problems: Vec<_> = app
        .problems_all()
        .into_iter()
        .filter(|pr| pr.path == tab.path)
        .collect();
    // Line starts, to map rows back to logical lines.
    let mut line_starts = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let line_of = |pos: usize| line_starts.partition_point(|s| *s <= pos).saturating_sub(1);
    let caret_line = line_of(cursor);
    let col_in = |from: usize, to: usize| text[from..to].chars().count();
    for (i, &(s, e)) in rows.iter().enumerate().skip(first).take(cap) {
        let y = r.y + ((i - first) as u32 * rh) as i32;
        let ty = y + (rh as i32 - ch as i32) / 2;
        let line = line_of(s);
        let starts_line = line_starts[line] == s;
        let ls = line_starts[line];
        let col0 = col_in(ls, s);
        let x_of = |pos: usize| -> i32 {
            let c = col_in(s, pos) as i64 - hs as i64;
            tx + (c.clamp(0, vc as i64) as i32) * cw as i32
        };
        if i == vrow && sel_a == sel_b {
            if app.settings.dark {
                p.border(
                    Rect::new(
                        r.x + gutter as i32 - 4,
                        y,
                        r.width.saturating_sub(gutter + 10),
                        rh,
                    ),
                    Color::TRANSPARENT,
                    0,
                    pal.line_highlight,
                );
            } else {
                p.box_(
                    Rect::new(
                        r.x + gutter as i32 - 4,
                        y,
                        r.width.saturating_sub(gutter + 10),
                        rh,
                    ),
                    pal.line_highlight,
                    0,
                );
            }
        }
        // Selection, including the newline at the end of a fully selected row.
        if sel_a != sel_b && sel_a <= e && sel_b >= s {
            let a = sel_a.max(s);
            let b = sel_b.min(e);
            let x0 = x_of(a);
            let mut x1 = x_of(b);
            if sel_b > e && e < text.len() {
                x1 += cw as i32 / 2 + 2;
            }
            if x1 > x0 {
                p.box_(Rect::new(x0, y, (x1 - x0) as u32, rh), pal.selection, 0);
            }
        }
        for &(a, b) in &matches {
            if a < e && b > s || (a == b && a >= s && a < e) {
                let current = (a, b) == (sel_a, sel_b);
                let x0 = x_of(a.max(s));
                let x1 = x_of(b.min(e));
                p.box_(
                    Rect::new(x0, y, (x1 - x0).max(2) as u32, rh),
                    if current {
                        pal.match_current
                    } else {
                        pal.match_other
                    },
                    0,
                );
            }
        }
        if let Some((open, close)) = bracket {
            for at in [open, close] {
                if at >= s && at < e {
                    let x0 = x_of(at);
                    p.border(
                        Rect::new(x0, y, cw, rh),
                        Color(0, 100, 0, 26),
                        0,
                        pal.bracket,
                    );
                }
            }
        }
        if starts_line {
            let n = (line + 1).to_string();
            let nx = r.x + 18 + (digits as usize - n.len()) as i32 * cw as i32;
            mono(
                p,
                nx,
                ty,
                &n,
                size,
                if line == caret_line {
                    pal.line_no_active
                } else {
                    pal.line_no
                },
            );
        }
        // Tokens: coloured runs, plain text between them.
        let row_spans: Vec<Span> = spans.get(line).cloned().unwrap_or_default();
        let (rs, re) = (s - ls, e - ls);
        let line_text = &text[ls..line_starts
            .get(line + 1)
            .map_or(text.len(), |n| n - 1)
            .max(ls)];
        let mut runs: Vec<(usize, usize, Color)> = Vec::new();
        let mut at = rs;
        let default = syntax::color(syntax::Tok::Text, app.settings.dark);
        for sp in row_spans.iter().filter(|sp| sp.end > rs && sp.start < re) {
            let a = sp.start.max(rs);
            let b = sp.end.min(re);
            if a > at {
                runs.push((at, a, default));
            }
            runs.push((a, b, syntax::color(sp.tok, app.settings.dark)));
            at = b;
        }
        if at < re {
            runs.push((at, re, default));
        }
        for (a, b, color) in runs {
            if a >= b || !line_text.is_char_boundary(a) || !line_text.is_char_boundary(b) {
                continue;
            }
            let start_col = col_in(ls, ls + a) - col0;
            let run: Vec<char> = line_text[a..b]
                .chars()
                .map(|c| if c == '\t' { ' ' } else { c })
                .collect();
            let end_col = start_col + run.len();
            if end_col <= hs || start_col >= hs + vc {
                continue;
            }
            let from = hs.saturating_sub(start_col);
            let to = run.len().min(hs + vc - start_col);
            let visible: String = run[from..to].iter().collect();
            let x = tx + ((start_col + from - hs) as u32 * cw) as i32;
            mono(p, x, ty, &visible, size, color);
        }
        // Squiggles under what a tool reported on this line.
        for pr in problems.iter().filter(|pr| pr.line == line + 1) {
            let lead = line_text.len() - line_text.trim_start().len();
            let from = (pr.col.saturating_sub(1)).max(line_text[..lead].chars().count());
            let len = line_text.chars().count();
            if len == 0 {
                continue;
            }
            let from_byte = line_text
                .char_indices()
                .nth(from.min(len - 1))
                .map_or(0, |(i, _)| i)
                + ls;
            if from_byte >= e || ls + line_text.len() < s {
                continue;
            }
            let x0 = x_of(from_byte.max(s));
            let x1 = x_of((ls + line_text.trim_end().len()).min(e));
            let c = if pr.severity == Severity::Error {
                pal.error
            } else {
                pal.warning
            };
            zigzag(p, x0, x1.max(x0 + 6), y + rh as i32 - 2, c);
        }
    }
    // The text area carries the view's geometry so a click resolves to what is under it.
    let target = format!("code:editor:{first}:{hs}:{wrap}:{cap}");
    p.region(
        Rect::new(tx, r.y, tw.max(1), r.height),
        &target,
        &format!("{} editor text", tab.name()),
    );
    if focused && vrow >= first && vrow < first + cap && vcol >= hs && vcol <= hs + vc {
        let x = tx + ((vcol - hs) as u32 * cw) as i32;
        let y = r.y + ((vrow - first) as u32 * rh) as i32;
        p.box_(Rect::new(x, y, 2, rh), pal.caret, 0);
    }
    // Scrollbar: the slider is where the view is; the track pages it.
    if rows.len() > cap {
        let track = Rect::new(r.x + r.width as i32 - 14, r.y, 14, r.height);
        let total = rows.len().max(1) as u64;
        let slider_h = ((u64::from(r.height) * cap as u64 / total) as u32)
            .max(20)
            .min(r.height);
        let slider_y = r.y
            + (u64::from(r.height - slider_h) * first as u64 / (total - cap as u64).max(1)) as i32;
        p.box_(Rect::new(track.x, slider_y, 14, slider_h), pal.slider, 0);
        if slider_y > track.y {
            p.region(
                Rect::new(track.x, track.y, 14, (slider_y - track.y) as u32),
                &format!("code:scroll:{}", first.saturating_sub(cap)),
                "Page up",
            );
        }
        let below = slider_y + slider_h as i32;
        if below < track.y + r.height as i32 {
            p.region(
                Rect::new(
                    track.x,
                    below,
                    14,
                    (track.y + r.height as i32 - below) as u32,
                ),
                &format!(
                    "code:scroll:{}",
                    (first + cap).min(rows.len().saturating_sub(1))
                ),
                "Page down",
            );
        }
    }
    if let Some(find) = &app.find {
        find_widget(app, p, pal, find, &matches, (sel_a, sel_b), r);
    }
}
fn app_find_matches(app: &Workbench, tab: &Tab) -> Vec<(usize, usize)> {
    match &app.find {
        Some(f) => super::find_all(&tab.doc.text, &f.query, f.opts).unwrap_or_default(),
        None => vec![],
    }
}

fn find_widget(
    app: &Workbench,
    p: &mut Painter,
    pal: &Pal,
    find: &super::Find,
    matches: &[(usize, usize)],
    sel: (usize, usize),
    r: Rect,
) {
    let fw = 430.min(r.width.saturating_sub(40));
    let fh = if find.replace_open { 64 } else { 34 };
    let box_ = Rect::new(r.x + r.width as i32 - fw as i32 - 18, r.y, fw, fh);
    p.drop_shadow(box_, 2, 6, 90, 2);
    p.border(box_, pal.widget, 0, pal.widget_border);
    let chev = Rect::new(box_.x + 2, box_.y + 4, 16, fh - 8);
    p.symbol(
        if find.replace_open {
            "chevron-down"
        } else {
            "chevron-right"
        },
        chev.x + 1,
        box_.y + 9,
        14,
        pal.fg,
    );
    p.region(chev, "code:find:toggle-replace", "Toggle Replace");
    let iw = fw.saturating_sub(22 + 150);
    let q = Rect::new(box_.x + 22, box_.y + 4, iw, 26);
    input_box(
        p,
        pal,
        Rect::new(q.x, q.y, iw.saturating_sub(70), 26),
        &find.query,
        "Find",
        app.focus == Focus::Find,
        "code:find-input",
        "Find",
    );
    p.border(
        Rect::new(q.x + iw.saturating_sub(70) as i32 - 1, q.y, 70, 26),
        pal.input,
        0,
        if app.focus == Focus::Find {
            pal.accent
        } else {
            pal.input_border
        },
    );
    for (i, (name, text, on, title)) in [
        ("case", "Aa", find.opts.case, "Match Case (Alt+C)"),
        ("word", "ab", find.opts.word, "Match Whole Word (Alt+W)"),
        (
            "regex",
            ".*",
            find.opts.regex,
            "Use Regular Expression (Alt+R)",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let b = Rect::new(q.x + iw as i32 - 68 + i as i32 * 22, q.y + 3, 20, 20);
        toggle(p, pal, b, text, on, &format!("code:find:{name}"), title);
    }
    let bad = !find.query.is_empty() && super::find_all("", &find.query, find.opts).is_err();
    let status = if bad {
        "Invalid".to_owned()
    } else if find.query.is_empty() || matches.is_empty() {
        "No results".to_owned()
    } else {
        match matches.iter().position(|m| *m == sel) {
            Some(i) => format!("{} of {}", i + 1, matches.len()),
            None => format!("? of {}", matches.len()),
        }
    };
    let sx = q.x + iw as i32 + 6;
    label(
        p,
        sx,
        q.y + 5,
        72,
        &status,
        12,
        if matches.is_empty() && !find.query.is_empty() {
            pal.error
        } else {
            pal.fg
        },
    );
    for (i, (target, up, name)) in [
        ("code:find:prev", true, "Previous Match (Shift+Enter)"),
        ("code:find:next", false, "Next Match (Enter)"),
    ]
    .into_iter()
    .enumerate()
    {
        let b = Rect::new(sx + 74 + i as i32 * 24, q.y + 2, 22, 22);
        icons::arrow(
            p,
            b.x + 3,
            b.y + 3,
            16,
            up,
            if matches.is_empty() { pal.desc } else { pal.fg },
        );
        if matches.is_empty() {
            inert(p, b, &format!("{name}: no results"));
        } else {
            p.region(b, target, name);
        }
    }
    let close = Rect::new(box_.x + fw as i32 - 26, q.y + 2, 22, 22);
    p.symbol("close", close.x + 5, close.y + 5, 12, pal.fg);
    p.region(close, "code:find:close", "Close (Escape)");
    if find.replace_open {
        let rr = Rect::new(q.x, q.y + 30, iw, 26);
        input_box(
            p,
            pal,
            rr,
            &find.replace,
            "Replace",
            app.focus == Focus::Replace,
            "code:replace-input",
            "Replace",
        );
        for (i, (target, name)) in [
            ("code:find:replace", "Replace (Enter)"),
            ("code:find:replace-all", "Replace All (Ctrl+Alt+Enter)"),
        ]
        .into_iter()
        .enumerate()
        {
            let b = Rect::new(rr.x + iw as i32 + 6 + i as i32 * 24, rr.y + 2, 22, 22);
            if i == 0 {
                icons::replace_one(p, b.x + 3, b.y + 3, 16, pal.fg);
            } else {
                icons::replace_all(p, b.x + 3, b.y + 3, 16, pal.fg);
            }
            if matches.is_empty() {
                inert(p, b, &format!("{name}: no results"));
            } else {
                p.region(b, target, name);
            }
        }
    }
}

fn panel(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    p.box_(r, pal.side, 0);
    p.hline(r.x, r.y, r.width, pal.border);
    let (errors, warnings) = app.counts();
    let mut x = r.x + 10;
    for (tab, title) in [
        (PanelTab::Problems, "PROBLEMS"),
        (PanelTab::Output, "OUTPUT"),
        (PanelTab::Terminal, "TERMINAL"),
    ] {
        let on = app.panel == tab;
        let tw = p.measure(title, 11, false);
        let mut width = tw + 16;
        label(
            p,
            x + 8,
            r.y + 10,
            tw + 2,
            title,
            11,
            if on { pal.fg } else { pal.desc },
        );
        if tab == PanelTab::Problems && errors + warnings > 0 {
            width += badge(
                p,
                pal,
                x + tw as i32 + 14,
                r.y + 8,
                errors + warnings,
                pal.badge,
                pal.badge_fg,
            ) + 4;
        }
        if on {
            p.box_(Rect::new(x + 8, r.y + 30, tw, 1), pal.accent, 0);
        }
        let name = match tab {
            PanelTab::Problems => "problems",
            PanelTab::Output => "output",
            PanelTab::Terminal => "terminal",
        };
        p.region(
            Rect::new(x, r.y + 4, width, 28),
            &format!("code:panel:{name}"),
            title,
        );
        x += width as i32 + 4;
    }
    // Panel actions, right to left.
    let mut ax = r.x + r.width as i32 - 30;
    let close = Rect::new(ax, r.y + 7, 22, 22);
    p.symbol("close", close.x + 5, close.y + 5, 12, pal.fg);
    p.region(close, "code:panel-close", "Close Panel");
    ax -= 28;
    if app.panel == PanelTab::Terminal {
        for (symbol, id, name) in [
            ("trash", "workbench.action.terminal.kill", "Kill Terminal"),
            ("plus", "workbench.action.terminal.new", "New Terminal"),
        ] {
            let b = Rect::new(ax, r.y + 7, 22, 22);
            p.symbol(symbol, b.x + 4, b.y + 4, 14, pal.fg);
            icon_button(p, b, &format!("code:cmd:{id}"), name, app.enabled(id));
            ax -= 26;
        }
    } else if app.panel == PanelTab::Output {
        let b = Rect::new(ax - 60, r.y + 7, 80, 22);
        p.border(b, pal.input, 2, pal.input_border);
        label(p, b.x + 6, b.y + 3, 70, "Git", 12, pal.fg);
        p.disabled("Output channel: Git is the only channel on this machine");
    }
    let body = Rect::new(r.x, r.y + 35, r.width, r.height.saturating_sub(35));
    match app.panel {
        PanelTab::Terminal => terminal(app, p, pal, body),
        PanelTab::Problems => problems_panel(app, p, pal, body),
        PanelTab::Output => {
            let (cw, rh) = terminal_cell();
            let cap = (body.height / rh).max(1) as usize;
            let skip = app.output.len().saturating_sub(cap);
            let cols = (body.width.saturating_sub(24) / cw) as usize;
            for (i, line) in app.output.iter().skip(skip).enumerate() {
                let shown: String = line.chars().take(cols).collect();
                mono(
                    p,
                    body.x + 12,
                    body.y + (i as u32 * rh) as i32,
                    &shown,
                    TERM_SIZE,
                    pal.fg,
                );
            }
        }
    }
}

fn problems_panel(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    let all = app.problems_all();
    if all.is_empty() {
        label(
            p,
            r.x + 20,
            r.y + 4,
            r.width.saturating_sub(30),
            "No problems have been detected in the workspace.",
            13,
            pal.fg,
        );
        return;
    }
    let mut y = r.y;
    let mut last_file = String::new();
    for (i, pr) in all.iter().enumerate() {
        if y > r.y + r.height as i32 - ROW as i32 {
            break;
        }
        if pr.path != last_file {
            last_file = pr.path.clone();
            let name = basename(&pr.path);
            p.symbol("chevron-down", r.x + 6, y + 4, 14, pal.fg);
            let (glyph, gc) = icons::file_glyph(Language::from_path(name), name);
            p.label(r.x + 24, y + 4, 18, glyph, 10, gc, true, Align::Left);
            let nw = label(p, r.x + 44, y + 3, 300, name, 13, pal.fg);
            let dir = app.rel(&pr.path).map_or_else(
                || super::parent(&pr.path).to_owned(),
                |rel| super::parent(&rel).to_owned(),
            );
            let dw = label(p, r.x + 50 + nw as i32, y + 4, 300, &dir, 12, pal.desc);
            let n = all.iter().filter(|q| q.path == pr.path).count();
            badge(
                p,
                pal,
                r.x + 58 + (nw + dw) as i32,
                y + 2,
                n,
                pal.badge,
                pal.badge_fg,
            );
            y += ROW as i32;
        }
        let row = Rect::new(r.x, y, r.width, ROW);
        let c = if pr.severity == Severity::Error {
            pal.error
        } else {
            pal.warning
        };
        if pr.severity == Severity::Error {
            icons::error(p, r.x + 30, y + 3, 16, c);
        } else {
            icons::warning(p, r.x + 30, y + 3, 16, c);
        }
        let mw = label(
            p,
            r.x + 52,
            y + 3,
            r.width.saturating_sub(260),
            &pr.message,
            13,
            pal.fg,
        );
        label(
            p,
            r.x + 58 + mw as i32,
            y + 4,
            240,
            &format!("{}  [Ln {}, Col {}]", pr.source, pr.line, pr.col),
            12,
            pal.desc,
        );
        p.region(row, &format!("code:problem:{i}"), &pr.message);
        y += ROW as i32;
    }
}

fn terminal(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    let Some(term) = app.terminal() else {
        return;
    };
    let list_w = if app.terminals.len() > 1 { 150 } else { 0 };
    if list_w > 0 {
        let lx = r.x + r.width as i32 - list_w as i32;
        p.vline(lx, r.y, r.height, pal.border);
        for (i, t) in app.terminals.iter().enumerate() {
            let row = Rect::new(lx + 1, r.y + i as i32 * ROW as i32, list_w - 1, ROW);
            if i == app.term {
                p.box_(row, pal.list_inactive, 0);
            }
            p.symbol("terminal", row.x + 8, row.y + 4, 14, pal.fg);
            let name = if i == 0 {
                t.name.clone()
            } else {
                format!("{} ({})", t.name, i + 1)
            };
            label(p, row.x + 28, row.y + 3, list_w - 34, &name, 13, pal.fg);
            p.region(row, &format!("code:term-tab:{i}"), &name);
        }
    }
    let (cw, rh) = terminal_cell();
    let x0 = r.x + 20;
    let width = r.width.saturating_sub(list_w + 36);
    let cols = (width / cw).max(1) as usize;
    p.region(
        Rect::new(r.x, r.y, r.width.saturating_sub(list_w), r.height),
        "code:terminal",
        "Terminal",
    );
    let posix = app.platform != Platform::Windows;
    // (text, colour, decoration: None, Some(true) success, Some(false) failure)
    let mut lines: Vec<TermLine> = Vec::new();
    let wrap = |lines: &mut Vec<TermLine>, text: &str, c: Color| {
        for raw in text.trim_end_matches('\n').split('\n') {
            let chars: Vec<char> = raw.chars().collect();
            if chars.is_empty() {
                lines.push((vec![], None));
            }
            for chunk in chars.chunks(cols) {
                lines.push((vec![(chunk.iter().collect(), c)], None));
            }
        }
    };
    let prompt_color = if posix { pal.term_green } else { pal.fg };
    for entry in &term.transcript {
        let prompt = crate::prompt_or_sigil(&entry.prompt).to_owned();
        let mut echo = vec![
            (format!("{prompt} "), prompt_color),
            (entry.command.clone(), pal.fg),
        ];
        let total: usize = echo.iter().map(|(t, _)| t.chars().count()).sum();
        if total > cols {
            let joined: String = echo.iter().map(|(t, _)| t.clone()).collect();
            let start = lines.len();
            wrap(&mut lines, &joined, pal.fg);
            if let Some(first) = lines.get_mut(start) {
                first.1 = Some(!entry.failed());
            }
        } else {
            lines.push((std::mem::take(&mut echo), Some(!entry.failed())));
        }
        if !entry.stdout.is_empty() {
            wrap(&mut lines, &entry.stdout, pal.fg);
        }
        if !entry.stderr.is_empty() {
            wrap(&mut lines, &entry.stderr, pal.term_red);
        }
        if entry.failed() {
            wrap(&mut lines, &entry.status(), pal.term_red);
        }
    }
    let prompt = format!("{} ", crate::prompt_or_sigil(&term.prompt));
    let capacity = (r.height.saturating_sub(8) / rh).max(1) as usize;
    let visible = capacity.saturating_sub(1).max(1);
    let hidden = lines.len().saturating_sub(visible);
    let scroll = term.scroll.min(hidden);
    let first = hidden - scroll;
    let mut y = r.y + 2;
    for (spans, deco) in lines.iter().skip(first).take(visible) {
        if let Some(ok) = deco {
            if *ok {
                p.ring(r.x + 9, y + rh as i32 / 2, 4, 1, pal.desc);
            } else {
                p.circle(r.x + 9, y + rh as i32 / 2, 4, pal.term_red);
            }
        }
        let mut x = x0;
        for (text, c) in spans {
            mono(p, x, y, text, TERM_SIZE, *c);
            x += (text.chars().count() as u32 * cw) as i32;
        }
        y += rh as i32;
    }
    if scroll == 0 {
        let prompt_w = (prompt.chars().count() as u32 * cw) as i32;
        mono(p, x0, y, &prompt, TERM_SIZE, prompt_color);
        mono(p, x0 + prompt_w, y, &term.input, TERM_SIZE, pal.fg);
        p.region(
            Rect::new(
                x0 + prompt_w,
                y,
                width.saturating_sub(prompt_w as u32).max(1),
                rh,
            ),
            "code:terminal-line",
            "Terminal input",
        );
        let before = term
            .input
            .get(..term.cursor.min(term.input.len()))
            .unwrap_or("")
            .chars()
            .count() as i32;
        let caret = Rect::new(x0 + prompt_w + before * cw as i32, y, cw, rh);
        if app.focus == Focus::Terminal {
            p.box_(caret, Color(pal.fg.0, pal.fg.1, pal.fg.2, 200), 0);
        } else {
            p.border(caret, Color::TRANSPARENT, 0, pal.fg);
        }
    }
    if hidden > 0 {
        let track_h = r.height.saturating_sub(4);
        let total = lines.len().max(1) as u32;
        let thumb = (track_h * visible as u32 / total).clamp(20, track_h.max(20));
        let travel = track_h.saturating_sub(thumb);
        let top = r.y + 2 + (travel as usize * (hidden - scroll) / hidden.max(1)) as i32;
        let bx = r.x + r.width as i32 - list_w as i32 - 14;
        p.box_(Rect::new(bx, top, 12, thumb), pal.slider, 0);
        if scroll < hidden {
            p.region(
                Rect::new(bx, r.y, 12, (top - r.y).max(1) as u32),
                &format!("code:term-scroll:{}", scroll + visible),
                "Scroll up",
            );
        }
        if scroll > 0 {
            let below = top + thumb as i32;
            p.region(
                Rect::new(bx, below, 12, (r.y + r.height as i32 - below).max(1) as u32),
                &format!("code:term-scroll:{}", scroll.saturating_sub(visible)),
                "Scroll down",
            );
        }
    }
}

fn status_bar(app: &Workbench, p: &mut Painter, pal: &Pal, r: Rect) {
    let bg = if app.workspace().is_none() && app.settings.dark {
        hex(0x1F1F1F)
    } else {
        pal.side
    };
    p.box_(r, bg, 0);
    p.hline(r.x, r.y, r.width, pal.border);
    let fg = pal.fg;
    let mut x = r.x + 8;
    let y = r.y + 3;
    if app.scm.repo == Some(true) {
        icons::scm(p, x, y + 1, 14, fg);
        let tw = label(p, x + 18, y, 160, &app.scm.branch, 12, fg);
        p.region(
            Rect::new(x - 4, r.y, tw + 26, r.height),
            "code:status:branch",
            &format!("{} (Git) - Checkout Branch/Tag...", app.scm.branch),
        );
        x += tw as i32 + 30;
    }
    let (errors, warnings) = app.counts();
    let start = x;
    icons::error(p, x, y + 1, 14, fg);
    x += 18 + label(p, x + 18, y, 40, &errors.to_string(), 12, fg) as i32 + 6;
    icons::warning(p, x, y + 1, 14, fg);
    x += 18 + label(p, x + 18, y, 40, &warnings.to_string(), 12, fg) as i32 + 4;
    p.region(
        Rect::new(start - 4, r.y, (x - start + 8) as u32, r.height),
        "code:status:problems",
        &format!("Errors: {errors}, Warnings: {warnings}"),
    );
    let Some(tab) = app
        .active_tab()
        .filter(|t| t.kind != TabKind::Settings && t.loaded)
    else {
        return;
    };
    let (line, col) = tab.doc.position();
    let selected = tab.doc.selected_text().chars().count();
    let items: Vec<(String, Option<&str>)> = vec![
        (
            if selected > 0 {
                format!("Ln {}, Col {} ({selected} selected)", line + 1, col + 1)
            } else {
                format!("Ln {}, Col {}", line + 1, col + 1)
            },
            Some("position"),
        ),
        (format!("Spaces: {}", app.settings.tab_size), Some("indent")),
        ("UTF-8".into(), None),
        ((if tab.crlf { "CRLF" } else { "LF" }).into(), Some("eol")),
        (tab.lang.name().into(), Some("language")),
    ];
    let mut rx = r.x + r.width as i32 - 10;
    for (text, target) in items.into_iter().rev() {
        let tw = p.measure(&text, 12, false);
        rx -= tw as i32;
        label(p, rx, y, tw + 2, &text, 12, fg);
        let hit = Rect::new(rx - 5, r.y, tw + 10, r.height);
        match target {
            Some(t) => p.region(hit, &format!("code:status:{t}"), &text),
            None => inert(p, hit, "Encoding: files are read and written as UTF-8"),
        }
        rx -= 14;
    }
}

fn menu_overlay(app: &Workbench, p: &mut Painter, pal: &Pal, menu: &str, w: u32, h: u32) {
    let Some((_, _, items)) = MENUS.iter().find(|(name, _, _)| *name == menu) else {
        return;
    };
    let mac = app.platform == Platform::Mac;
    p.region(Rect::new(0, 0, w, h), "code:menu-close", "Close menu");
    let width = items
        .iter()
        .filter_map(|id| commands::command(id))
        .map(|c| {
            p.measure(c.label, 13, false) + p.measure(&display_keys(c.keys, mac), 12, false) + 90
        })
        .max()
        .unwrap_or(200)
        .max(240);
    let height: u32 = items
        .iter()
        .map(|i| if *i == "-" { 9 } else { 26 })
        .sum::<u32>()
        + 8;
    let (x, y) = if menu == "manage" {
        (
            ACT_W as i32 + 2,
            (h as i32 - STATUS_H as i32 - height as i32 - 4).max(0),
        )
    } else {
        let x = frame::menu_bar(p, w + 2)
            .into_iter()
            .find(|(name, ..)| *name == menu)
            .map_or(36, |(_, _, x, _)| x - 1);
        (x, 0)
    };
    let r = Rect::new(x, y, width, height);
    p.drop_shadow(r, 5, 10, 110, 3);
    p.border(
        r,
        pal.menu,
        5,
        if app.settings.dark {
            hex(0x454545)
        } else {
            hex(0xCECECE)
        },
    );
    let mut iy = y + 4;
    for id in items.iter() {
        if *id == "-" {
            p.hline(x + 10, iy + 4, width.saturating_sub(20), pal.border);
            iy += 9;
            continue;
        }
        let Some(c) = commands::command(id) else {
            continue;
        };
        let row = Rect::new(x + 4, iy, width - 8, 26);
        let enabled = app.enabled(id);
        let color = if enabled.is_ok() { pal.fg } else { pal.desc };
        let keys = display_keys(c.keys, mac);
        let keys_w = p.measure(&keys, 12, false);
        label(
            p,
            row.x + 22,
            iy + 4,
            width.saturating_sub(keys_w + 60),
            c.label,
            13,
            color,
        );
        p.label(
            row.x,
            iy + 5,
            row.width - 14,
            &keys,
            12,
            pal.desc,
            false,
            Align::Right,
        );
        match enabled {
            Ok(()) => p.region(row, &format!("code:cmd:{id}"), c.label),
            Err(why) => inert(p, row, &format!("{}: {why}", c.label)),
        }
        iy += 26;
    }
}

fn quick_overlay(app: &Workbench, p: &mut Painter, pal: &Pal, w: u32, h: u32) {
    let Some(quick) = &app.quick else { return };
    let mac = app.platform == Platform::Mac;
    p.region(
        Rect::new(0, 0, w, h),
        "code:quick-close",
        "Close Quick Input",
    );
    let qw = 600.min(w.saturating_sub(40)).max(200.min(w));
    let x = (w as i32 - qw as i32) / 2;
    let items = app.quick_items();
    let visible = items.len().min(12);
    let list_h = if items.is_empty() {
        26
    } else {
        visible as u32 * ROW
    };
    let r = Rect::new(x, 8, qw, 44 + list_h + 6);
    p.drop_shadow(r, 6, 12, 120, 3);
    p.border(r, pal.quick, 6, pal.widget_border);
    let placeholder = match quick.mode {
        QuickMode::Open if quick.value.is_empty() => {
            "Search files by name (append : to go to line)"
        }
        QuickMode::Theme => "Select Color Theme",
        QuickMode::Language => "Select Language Mode",
        QuickMode::TabSize => "Select Tab Size for the Current File",
        QuickMode::Eol => "Select End of Line Sequence",
        QuickMode::Branch => "Select a ref to checkout",
        _ => "",
    };
    let ok = quick.mode == QuickMode::OpenFolder;
    let input = Rect::new(x + 8, 16, qw.saturating_sub(if ok { 70 } else { 16 }), 26);
    input_box(
        p,
        pal,
        input,
        &quick.value,
        placeholder,
        app.focus == Focus::Quick,
        "code:quick-input",
        "Quick input",
    );
    if ok {
        button(
            p,
            pal,
            Rect::new(x + qw as i32 - 58, 16, 50, 26),
            "OK",
            "code:quick-ok",
        );
    }
    let mut y = 50;
    if items.is_empty() {
        label(p, x + 12, y + 3, qw - 20, "No matching results", 13, pal.fg);
        return;
    }
    let start = quick.selected.saturating_sub(visible.saturating_sub(1));
    for (i, item) in items.iter().enumerate().skip(start).take(visible) {
        let row = Rect::new(x + 4, y, qw - 8, ROW);
        if i == quick.selected {
            p.box_(row, pal.list_active, 3);
            if !app.settings.dark {
                p.border(row, Color::TRANSPARENT, 3, pal.accent);
            }
        }
        let mut lx = x + 14;
        if quick.mode == QuickMode::Open
            && !quick.value.starts_with('>')
            && !quick.value.starts_with(':')
        {
            let (glyph, c) = icons::file_glyph(Language::from_path(&item.label), &item.label);
            p.label(lx, y + 4, 18, glyph, 10, c, true, Align::Left);
            lx += 22;
        }
        // The label, with the characters the query matched picked out.
        let chars: Vec<char> = item.label.chars().collect();
        let mut run = String::new();
        let mut run_hit = false;
        let flush = |p: &mut Painter, run: &mut String, hit: bool, lx: &mut i32| {
            if run.is_empty() {
                return;
            }
            let room = (x + qw as i32 - *lx - 150).max(0) as u32;
            let wdt = p.label(
                *lx,
                y + 3,
                room,
                run,
                13,
                if hit { pal.link } else { pal.fg },
                hit,
                Align::Left,
            );
            *lx += wdt as i32;
            run.clear();
        };
        for (ci, c) in chars.iter().enumerate() {
            let hit = item.hits.contains(&ci);
            if hit != run_hit {
                flush(p, &mut run, run_hit, &mut lx);
                run_hit = hit;
            }
            run.push(*c);
        }
        flush(p, &mut run, run_hit, &mut lx);
        if !item.detail.is_empty() {
            label(
                p,
                lx + 8,
                y + 4,
                (x + qw as i32 - lx - 160).max(0) as u32,
                &item.detail,
                12,
                pal.desc,
            );
        }
        if !item.keys.is_empty() {
            let keys = display_keys(&item.keys, mac);
            let kw = p.measure(&keys, 11, false) + 10;
            let k = Rect::new(x + qw as i32 - kw as i32 - 12, y + 3, kw, 16);
            p.border(
                k,
                if app.settings.dark {
                    Color(128, 128, 128, 44)
                } else {
                    Color(221, 221, 221, 102)
                },
                3,
                if app.settings.dark {
                    Color(51, 51, 51, 153)
                } else {
                    Color(204, 204, 204, 102)
                },
            );
            p.label(k.x, k.y + 1, kw, &keys, 11, pal.fg, false, Align::Center);
        }
        p.region(row, &format!("code:quick:{i}"), &item.label);
        y += ROW as i32;
    }
}
