//! KiCad's wxWidgets chrome, drawn in each desktop's native look: menu bars, icon
//! toolbars, modal dialogs, text fields, check boxes, list rows and the status bar.
//! Every control either dispatches a real command or is announced disabled with why.
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use cw_scene::{Color, Rect};

pub const INK: Color = Color::rgb(30, 30, 30);
pub const MUTED: Color = Color::rgb(110, 110, 110);
pub const FAINT: Color = Color::rgb(170, 170, 170);
pub const EDGE: Color = Color::rgb(200, 200, 200);
pub const WHITE: Color = Color::WHITE;
pub const MENU_H: u32 = 24;
pub const TOOL_H: u32 = 34;
pub const STATUS_H: u32 = 24;
pub const SIDE_W: u32 = 34;

/// Platform palette for the native widgets KiCad's toolkit uses.
pub struct Chrome {
    pub bar: Color,
    pub panel: Color,
    pub accent: Color,
    pub selection: Color,
    pub radius: u32,
}
pub fn chrome(t: DesktopTheme) -> Chrome {
    match t {
        DesktopTheme::Windows => Chrome {
            bar: Color::rgb(243, 243, 243),
            panel: Color::rgb(249, 249, 249),
            accent: Color::rgb(0, 95, 184),
            selection: Color::rgb(204, 232, 255),
            radius: 3,
        },
        DesktopTheme::Ubuntu => Chrome {
            bar: Color::rgb(246, 245, 244),
            panel: Color::rgb(250, 250, 250),
            accent: Color::rgb(233, 84, 32),
            selection: Color::rgb(250, 219, 207),
            radius: 5,
        },
        _ => Chrome {
            bar: Color::rgb(236, 236, 236),
            panel: Color::rgb(246, 246, 246),
            accent: Color::rgb(0, 122, 255),
            selection: Color::rgb(203, 225, 252),
            radius: 5,
        },
    }
}

/// One entry of a drop-down menu.
pub struct MenuItem {
    pub label: String,
    pub shortcut: &'static str,
    /// The command it runs, or why it cannot run now.
    pub target: Result<String, &'static str>,
    pub separator_before: bool,
}
impl MenuItem {
    pub fn new(label: &str, shortcut: &'static str, target: Result<String, &'static str>) -> Self {
        Self {
            label: label.into(),
            shortcut,
            target,
            separator_before: false,
        }
    }
    pub fn sep(mut self) -> Self {
        self.separator_before = true;
        self
    }
}

/// The menu bar across the top of a frame; returns the y below it and the x of each
/// title (for placing the open menu under it).
pub fn menubar(
    p: &mut Painter,
    c: &Chrome,
    w: u32,
    titles: &[&str],
    open: Option<&str>,
) -> Vec<i32> {
    p.box_(Rect::new(0, 0, w, MENU_H), c.bar, 0);
    p.hline(0, MENU_H as i32 - 1, w, EDGE);
    let mut x = 6;
    let mut xs = vec![];
    for t in titles {
        let tw = p.measure(t, 13, false) + 16;
        let r = Rect::new(x, 2, tw, MENU_H - 4);
        let on = open == Some(*t);
        p.button(
            r,
            if on { c.selection } else { Color::TRANSPARENT },
            3,
            &format!("kicad:menu:{t}"),
            t,
        );
        p.label(r.x, 5, tw, t, 13, INK, false, Align::Center);
        xs.push(x);
        x += tw as i32;
    }
    xs
}

/// A drop-down menu under the menu bar.
pub fn menu_panel(p: &mut Painter, c: &Chrome, x: i32, items: &[MenuItem]) {
    let width = items
        .iter()
        .map(|i| p.measure(&i.label, 13, false) + p.measure(i.shortcut, 12, false) + 60)
        .max()
        .unwrap_or(160)
        .max(180);
    let height: u32 = items
        .iter()
        .map(|i| 26 + if i.separator_before { 9 } else { 0 })
        .sum::<u32>()
        + 8;
    let r = Rect::new(x, MENU_H as i32 - 1, width, height);
    p.drop_shadow(r, c.radius, 8, 50, 3);
    p.border(r, WHITE, c.radius, EDGE);
    let mut y = r.y + 4;
    for item in items {
        if item.separator_before {
            p.hline(r.x + 8, y + 4, width - 16, EDGE);
            y += 9;
        }
        let row = Rect::new(r.x + 4, y, width - 8, 26);
        match &item.target {
            Ok(target) => {
                p.button(row, Color::TRANSPARENT, 3, target, &item.label);
                p.label(
                    row.x + 22,
                    y + 5,
                    width - 40,
                    &item.label,
                    13,
                    INK,
                    false,
                    Align::Left,
                );
                p.label(
                    row.x,
                    y + 6,
                    width - 18,
                    item.shortcut,
                    12,
                    MUTED,
                    false,
                    Align::Right,
                );
            }
            Err(why) => {
                p.region(row, "kicad:noop", &item.label);
                p.disabled(&format!("{} — {why}", item.label));
                p.label(
                    row.x + 22,
                    y + 5,
                    width - 40,
                    &item.label,
                    13,
                    FAINT,
                    false,
                    Align::Left,
                );
                p.label(
                    row.x,
                    y + 6,
                    width - 18,
                    item.shortcut,
                    12,
                    FAINT,
                    false,
                    Align::Right,
                );
            }
        }
        y += 26;
    }
}

/// A toolbar icon button. `draw` paints the icon into a 20 px square.
pub type Icon = fn(&mut Painter, i32, i32);
/// A toolbar button: its icon, its command or why it is disabled, and its tooltip.
pub type ToolSpec<'a> = (Icon, Result<String, &'static str>, &'a str);
#[allow(clippy::too_many_arguments)]
pub fn tool(
    p: &mut Painter,
    c: &Chrome,
    x: i32,
    y: i32,
    icon: Icon,
    target: Result<String, &'static str>,
    tip: &str,
    active: bool,
) {
    let r = Rect::new(x, y, 28, 28);
    match &target {
        Ok(t) => {
            p.button(
                r,
                if active {
                    c.selection
                } else {
                    Color::TRANSPARENT
                },
                c.radius,
                t,
                tip,
            );
            if active {
                p.border(r, Color::TRANSPARENT, c.radius, c.accent);
            }
            icon(p, x + 4, y + 4);
        }
        Err(why) => {
            p.region(r, "kicad:noop", tip);
            p.disabled(&format!("{tip} — {why}"));
            let mark = p.scene.nodes.len();
            icon(p, x + 4, y + 4);
            // Greyed: every stroke of the icon at a third of its strength.
            for n in &mut p.scene.nodes[mark..] {
                n.opacity = 80;
            }
        }
    }
}
pub fn separator_v(p: &mut Painter, x: i32, y: i32) {
    p.vline(x, y + 4, 20, EDGE);
}

pub fn status_bar(p: &mut Painter, c: &Chrome, w: u32, h: u32, cells: &[String]) {
    let y = h as i32 - STATUS_H as i32;
    p.box_(Rect::new(0, y, w, STATUS_H), c.bar, 0);
    p.hline(0, y, w, EDGE);
    let mut x = 8;
    for (i, cell) in cells.iter().enumerate() {
        let cw = if i + 1 == cells.len() {
            w.saturating_sub(x as u32 + 8)
        } else {
            p.measure(cell, 12, false) + 24
        };
        p.label(x, y + 5, cw, cell, 12, INK, false, Align::Left);
        x += cw as i32;
        if i + 1 < cells.len() {
            p.vline(x - 10, y + 4, STATUS_H - 8, EDGE);
        }
    }
}

/// A modal dialog centred in the frame; returns its content rectangle.
#[allow(clippy::too_many_arguments)]
pub fn dialog(
    p: &mut Painter,
    c: &Chrome,
    t: DesktopTheme,
    w: u32,
    h: u32,
    dw: u32,
    dh: u32,
    title: &str,
) -> Rect {
    let dw = dw.min(w.saturating_sub(16));
    let dh = dh.min(h.saturating_sub(16));
    let r = Rect::new(
        (w as i32 - dw as i32) / 2,
        (h as i32 - dh as i32) / 2,
        dw,
        dh,
    );
    p.drop_shadow(r, c.radius + 2, 18, 70, 6);
    p.border(r, c.panel, c.radius + 2, EDGE);
    let title_h = 32;
    p.box_(
        Rect::new(r.x + 1, r.y + 1, dw - 2, title_h),
        c.bar,
        c.radius + 1,
    );
    p.hline(r.x, r.y + title_h as i32, dw, EDGE);
    let centred = t != DesktopTheme::Windows;
    p.label(
        r.x + if centred { 0 } else { 12 },
        r.y + 8,
        if centred { dw } else { dw - 60 },
        title,
        13,
        INK,
        centred,
        if centred { Align::Center } else { Align::Left },
    );
    let close = Rect::new(r.x + dw as i32 - 30, r.y + 5, 22, 22);
    p.button(close, Color::TRANSPARENT, 11, "kicad:dlg:cancel", "Close");
    p.line(
        vec![(close.x + 6, close.y + 6), (close.x + 16, close.y + 16)],
        MUTED,
        1,
    );
    p.line(
        vec![(close.x + 16, close.y + 6), (close.x + 6, close.y + 16)],
        MUTED,
        1,
    );
    Rect::new(
        r.x + 12,
        r.y + title_h as i32 + 10,
        dw - 24,
        dh - title_h - 20,
    )
}

pub fn button(p: &mut Painter, c: &Chrome, r: Rect, label: &str, target: &str, default: bool) {
    p.button(
        r,
        if default { c.accent } else { WHITE },
        c.radius,
        target,
        label,
    );
    if !default {
        p.border(r, Color::TRANSPARENT, c.radius, EDGE);
    }
    p.label(
        r.x,
        r.y + (r.height as i32 - 18) / 2,
        r.width,
        label,
        13,
        if default { WHITE } else { INK },
        false,
        Align::Center,
    );
}
pub fn button_disabled(p: &mut Painter, c: &Chrome, r: Rect, label: &str, why: &str) {
    p.border(r, Color::rgb(240, 240, 240), c.radius, EDGE);
    p.disabled(&format!("{label} — {why}"));
    p.label(
        r.x,
        r.y + (r.height as i32 - 18) / 2,
        r.width,
        label,
        13,
        FAINT,
        false,
        Align::Center,
    );
}

/// Single-line text field; clicking focuses it.
pub fn field(p: &mut Painter, c: &Chrome, r: Rect, value: &str, name: &str, focused: bool) {
    p.button(
        r,
        WHITE,
        c.radius.min(3),
        &format!("kicad:field:{name}"),
        name,
    );
    if let Some(n) = p.scene.nodes.last_mut() {
        if let Some(s) = &mut n.semantic {
            s.role = "textbox".into();
            s.value = Some(value.to_owned());
        }
    }
    p.border(
        r,
        Color::TRANSPARENT,
        c.radius.min(3),
        if focused { c.accent } else { EDGE },
    );
    let shown = p.label(
        r.x + 6,
        r.y + (r.height as i32 - 18) / 2,
        r.width.saturating_sub(12),
        value,
        13,
        INK,
        false,
        Align::Left,
    );
    if focused {
        let x = r.x + 7 + shown as i32;
        p.vline(
            x.min(r.x + r.width as i32 - 4),
            r.y + 5,
            r.height.saturating_sub(10),
            INK,
        );
    }
}
#[allow(clippy::too_many_arguments)]
pub fn label_field(
    p: &mut Painter,
    c: &Chrome,
    x: i32,
    y: i32,
    lw: u32,
    fw: u32,
    label: &str,
    value: &str,
    name: &str,
    focused: bool,
) {
    p.label(x, y + 5, lw, label, 13, INK, false, Align::Left);
    field(
        p,
        c,
        Rect::new(x + lw as i32, y, fw, 26),
        value,
        name,
        focused,
    );
}
pub fn checkbox(p: &mut Painter, c: &Chrome, x: i32, y: i32, label: &str, on: bool, target: &str) {
    let tw = p.measure(label, 13, false);
    p.region(Rect::new(x, y, 24 + tw, 20), target, label);
    if let Some(n) = p.scene.nodes.last_mut() {
        n.state = Some(cw_scene::NodeState {
            checked: Some(on),
            ..Default::default()
        });
        if let Some(s) = &mut n.semantic {
            s.role = "checkbox".into();
        }
    }
    let b = Rect::new(x, y + 2, 16, 16);
    p.border(
        b,
        if on { c.accent } else { WHITE },
        3,
        if on { c.accent } else { MUTED },
    );
    if on {
        p.line(
            vec![(x + 4, y + 10), (x + 7, y + 13), (x + 12, y + 6)],
            WHITE,
            2,
        );
    }
    p.label(x + 22, y + 1, tw + 2, label, 13, INK, false, Align::Left);
}
pub fn radio(p: &mut Painter, c: &Chrome, x: i32, y: i32, label: &str, on: bool, target: &str) {
    let tw = p.measure(label, 13, false);
    p.region(Rect::new(x, y, 24 + tw, 20), target, label);
    if let Some(n) = p.scene.nodes.last_mut() {
        n.state = Some(cw_scene::NodeState {
            checked: Some(on),
            ..Default::default()
        });
        if let Some(s) = &mut n.semantic {
            s.role = "radio".into();
        }
    }
    p.ring(x + 8, y + 10, 8, 1, if on { c.accent } else { MUTED });
    if on {
        p.circle(x + 8, y + 10, 4, c.accent);
    }
    p.label(x + 22, y + 1, tw + 2, label, 13, INK, false, Align::Left);
}
/// Tabs across the top of a notebook; returns the y below them.
pub fn tabs(
    p: &mut Painter,
    c: &Chrome,
    x: i32,
    y: i32,
    labels: &[&str],
    active: usize,
    prefix: &str,
) -> i32 {
    let mut tx = x;
    for (i, l) in labels.iter().enumerate() {
        let tw = p.measure(l, 13, false) + 24;
        let r = Rect::new(tx, y, tw, 26);
        if i == active {
            p.border(r, WHITE, 3, EDGE);
        }
        p.button(r, Color::TRANSPARENT, 3, &format!("{prefix}{i}"), l);
        p.label(
            r.x,
            y + 5,
            tw,
            l,
            13,
            if i == active { INK } else { MUTED },
            i == active,
            Align::Center,
        );
        tx += tw as i32 + 2;
    }
    let _ = c;
    p.hline(x, y + 26, (tx - x).max(0) as u32, EDGE);
    y + 30
}
/// A selectable row in a list; returns nothing, draws highlight when selected.
pub fn row(
    p: &mut Painter,
    c: &Chrome,
    r: Rect,
    text: &str,
    target: &str,
    selected: bool,
    color: Color,
) {
    p.button(
        r,
        if selected {
            c.selection
        } else {
            Color::TRANSPARENT
        },
        2,
        target,
        text,
    );
    p.label(
        r.x + 6,
        r.y + (r.height as i32 - 18) / 2,
        r.width.saturating_sub(10),
        text,
        13,
        color,
        false,
        Align::Left,
    );
}
