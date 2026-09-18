//! Platform palette and chrome shared by the native applications, so one app state
//! renders as five different-looking products without five copies of its logic.
use crate::desktop_scene::{scroll::Pane, shared::Align, DesktopTheme, Painter};
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

/// Height of iOS's large-title band: once the content has scrolled this far, the title
/// has gone under the navigation bar and the bar shows it inline instead.
pub const LARGE_TITLE: u32 = 52;

/// A screen being painted: where its content starts, and on a phone the one pane the
/// whole screen scrolls in.
pub struct Screen {
    /// Where content under the header starts, already moved by the scroll on a phone.
    pub top: i32,
    main: Option<Pane>,
}
impl Screen {
    /// The screen scrolls as one (a phone), so columns inside it need no pane of
    /// their own.
    pub fn scrolls(&self) -> bool {
        self.main.is_some()
    }
    /// How far the screen's pane has been scrolled, so a layout sized to the window
    /// (a month grid) keeps its size while it moves.
    pub fn offset(&self) -> i32 {
        self.main
            .as_ref()
            .map_or(0, |m| m.offset.clamp(0, i32::MAX / 8))
    }
    /// A column of the layout that scrolls: its own pane on a desktop, where a
    /// sidebar, a list and a reading pane each scroll by themselves; on a phone the
    /// screen's single pane already carries it.
    pub fn column(&self, p: &mut Painter, name: &str, r: Rect) -> Column {
        self.open_column(p, name, r, false)
    }
    /// A column that opens scrolled to its end.
    pub fn column_from_end(&self, p: &mut Painter, name: &str, r: Rect) -> Column {
        self.open_column(p, name, r, true)
    }
    fn open_column(&self, p: &mut Painter, name: &str, r: Rect, from_end: bool) -> Column {
        if self.main.is_some() {
            return Column {
                top: r.y,
                pane: None,
            };
        }
        let pane = if from_end {
            p.pane_from_end(name, r)
        } else {
            p.pane(name, r)
        };
        Column {
            top: pane.top(),
            pane: Some(pane),
        }
    }
    /// Close the screen's pane. Anything pinned over the content (a composer, a
    /// sheet) is painted after this.
    pub fn end(self, p: &mut Painter) {
        if let Some(main) = self.main {
            p.end_pane(main, None);
        }
    }
}
/// A scrolling column: paint its content from `top`.
pub struct Column {
    pub top: i32,
    pane: Option<Pane>,
}
impl Column {
    pub fn end(self, p: &mut Painter) {
        if let Some(pane) = self.pane {
            p.end_pane(pane, None);
        }
    }
}

/// Open a screen titled `title` whose content runs down to `bottom`. A desktop gets a
/// fixed toolbar with the title; Android a fixed top app bar over one scrolling pane;
/// iOS one scrolling pane whose first row is the large title, which collapses into the
/// navigation bar when it scrolls away (the frame reads that off the published pane).
pub fn screen(
    p: &mut Painter,
    t: DesktopTheme,
    l: &Look,
    w: u32,
    bottom: i32,
    title: &str,
) -> Screen {
    open_screen(p, t, l, w, bottom, title, false)
}
/// A screen that opens scrolled to its end, the newest message of a conversation.
pub fn screen_from_end(
    p: &mut Painter,
    t: DesktopTheme,
    l: &Look,
    w: u32,
    bottom: i32,
    title: &str,
) -> Screen {
    open_screen(p, t, l, w, bottom, title, true)
}
fn open_screen(
    p: &mut Painter,
    t: DesktopTheme,
    l: &Look,
    w: u32,
    bottom: i32,
    title: &str,
    from_end: bool,
) -> Screen {
    let open = |p: &mut Painter, r: Rect| {
        if from_end {
            p.pane_from_end("main", r)
        } else {
            p.pane("main", r)
        }
    };
    match t {
        DesktopTheme::Ios => {
            let pane = open(p, Rect::new(0, 0, w, bottom.max(1) as u32)).titled(title, LARGE_TITLE);
            let y = pane.top();
            p.strong(16, y + 8, w.saturating_sub(32), title, 34, INK);
            Screen {
                top: y + LARGE_TITLE as i32 + 6,
                main: Some(pane),
            }
        }
        DesktopTheme::Android => {
            let top = header(p, t, l, w, title);
            let pane = open(p, Rect::new(0, top, w, (bottom - top).max(1) as u32));
            Screen {
                top: pane.top(),
                main: Some(pane),
            }
        }
        _ => Screen {
            top: header(p, t, l, w, title),
            main: None,
        },
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
