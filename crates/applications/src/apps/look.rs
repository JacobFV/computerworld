//! Platform palette and chrome shared by the native applications, so one app state
//! renders as five different-looking products without five copies of its logic.
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use cw_scene::{Color, Rect};

pub const INK: Color = Color::rgb(29, 29, 31);
pub const MUTED: Color = Color::rgb(112, 114, 120);
pub const FAINT: Color = Color::rgb(160, 162, 168);
pub const LINE: Color = Color(0, 0, 0, 26);

pub struct Look {
    pub accent: Color,
    pub surface: Color,
    pub chrome: Color,
    pub selection: Color,
    /// Corner radius of the platform's cards and buttons.
    pub radius: u32,
    pub row: u32,
    /// Title size of a screen heading.
    pub title: u16,
}
pub fn look(t: DesktopTheme) -> Look {
    match t {
        DesktopTheme::Macos => Look {
            accent: Color::rgb(0, 122, 255),
            surface: Color::WHITE,
            chrome: Color::rgb(246, 246, 247),
            selection: Color(0, 122, 255, 36),
            radius: 6,
            row: 28,
            title: 20,
        },
        DesktopTheme::Windows => Look {
            accent: Color::rgb(0, 95, 184),
            surface: Color::WHITE,
            chrome: Color::rgb(249, 249, 249),
            selection: Color::rgb(229, 241, 251),
            radius: 4,
            row: 32,
            title: 20,
        },
        DesktopTheme::Ubuntu => Look {
            accent: Color::rgb(233, 84, 32),
            surface: Color::WHITE,
            chrome: Color::rgb(246, 246, 246),
            selection: Color(233, 84, 32, 34),
            radius: 8,
            row: 38,
            title: 19,
        },
        DesktopTheme::Ios => Look {
            accent: Color::rgb(0, 122, 255),
            surface: Color::rgb(242, 242, 247),
            chrome: Color(249, 249, 249, 245),
            selection: Color(0, 122, 255, 28),
            radius: 12,
            row: 48,
            title: 30,
        },
        DesktopTheme::Android => Look {
            accent: Color::rgb(76, 102, 43),
            surface: Color::rgb(253, 252, 245),
            chrome: Color::rgb(240, 240, 230),
            selection: Color(76, 102, 43, 34),
            radius: 16,
            row: 56,
            title: 24,
        },
    }
}

/// Screen header: a large title on phones, a compact toolbar on desktops. Returns the
/// y coordinate content starts at.
pub fn header(p: &mut Painter, t: DesktopTheme, l: &Look, w: u32, title: &str) -> i32 {
    if t.mobile() {
        p.strong(16, 14, w.saturating_sub(120), title, l.title, INK);
        let bottom = if t == DesktopTheme::Android { 62 } else { 58 };
        p.hline(0, bottom, w, LINE);
        bottom + 1
    } else {
        p.box_(Rect::new(0, 0, w, 40), l.chrome, 0);
        p.hline(0, 40, w, LINE);
        p.strong(14, 11, w.saturating_sub(180), title, 14, INK);
        41
    }
}

/// Pill button that really dispatches. `primary` fills it with the platform accent.
pub fn action(p: &mut Painter, l: &Look, r: Rect, label: &str, target: &str, primary: bool) {
    p.button(
        r,
        if primary {
            l.accent
        } else {
            Color::TRANSPARENT
        },
        l.radius,
        target,
        label,
    );
    if !primary {
        p.border(r, Color::TRANSPARENT, l.radius, LINE);
    }
    p.label(
        r.x,
        r.y + (r.height as i32 - 18) / 2,
        r.width,
        label,
        13,
        if primary { Color::WHITE } else { l.accent },
        true,
        Align::Center,
    );
}

/// Centred message shown in place of content: loading, offline, refused or simply empty.
pub fn notice(p: &mut Painter, w: u32, y: i32, text: &str) {
    p.center(0, y, w, text, 14, MUTED);
}

/// A control the platform paints but cannot honour in this state: announced disabled with
/// its reason, so nothing on screen promises an action that would only be refused.
pub fn inert(p: &mut Painter, l: &Look, r: Rect, label: &str, why: &str) {
    p.border(r, Color::TRANSPARENT, l.radius, LINE);
    p.disabled(why);
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

/// Segmented chip that really dispatches; `on` is the engaged segment.
pub fn chip(p: &mut Painter, l: &Look, r: Rect, label: &str, target: &str, on: bool) {
    p.button(
        r,
        if on { l.selection } else { Color::TRANSPARENT },
        l.radius,
        target,
        label,
    );
    if !on {
        p.border(r, Color::TRANSPARENT, l.radius, LINE);
    }
    p.label(
        r.x,
        r.y + (r.height as i32 - 16) / 2,
        r.width,
        label,
        12,
        if on { l.accent } else { INK },
        on,
        Align::Center,
    );
}
