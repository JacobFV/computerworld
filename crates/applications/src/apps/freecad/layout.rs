//! Where FreeCAD's main window puts its parts, shared by drawing and pointer handling.
use crate::desktop_scene::DesktopTheme;
use cw_scene::Rect;

pub const MENU_H: u32 = 22;
pub const TOOLBAR_H: u32 = 32;
pub const STATUS_H: u32 = 24;
pub const REPORT_H: u32 = 96;
pub const TAB_H: u32 = 24;
pub const ROW_H: u32 = 22;
/// Navigation cube square, and its margin from the view's top-right corner.
pub const CUBE: u32 = 120;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub w: u32,
    pub h: u32,
    /// The in-window menu bar; macOS shows FreeCAD's menus in the global menu bar.
    pub menu: Option<Rect>,
    pub toolbar: Rect,
    /// The Combo View dock on the left.
    pub combo: Rect,
    pub view: Rect,
    pub report: Option<Rect>,
    pub status: Rect,
}

pub fn layout(theme: DesktopTheme, w: u32, h: u32, report: bool) -> Layout {
    let menu_h = if theme == DesktopTheme::Macos {
        0
    } else {
        MENU_H
    };
    let menu = (menu_h > 0).then(|| Rect::new(0, 0, w, menu_h));
    let toolbar = Rect::new(0, menu_h as i32, w, TOOLBAR_H);
    let top = menu_h + TOOLBAR_H;
    let status = Rect::new(0, h.saturating_sub(STATUS_H) as i32, w, STATUS_H);
    let report_h = if report { REPORT_H.min(h / 4) } else { 0 };
    let report = (report_h > 0).then(|| {
        Rect::new(
            0,
            (h.saturating_sub(STATUS_H + report_h)) as i32,
            w,
            report_h,
        )
    });
    let body_h = h.saturating_sub(top + STATUS_H + report_h).max(1);
    let combo_w = (w * 27 / 100).clamp(210, 320).min(w / 2);
    let combo = Rect::new(0, top as i32, combo_w, body_h);
    let view = Rect::new(
        combo_w as i32 + 4,
        top as i32,
        w.saturating_sub(combo_w + 4).max(1),
        body_h,
    );
    Layout {
        w,
        h,
        menu,
        toolbar,
        combo,
        view,
        report,
        status,
    }
}

impl Layout {
    /// The navigation cube's square, relative to the 3D view.
    pub fn cube(&self) -> Rect {
        Rect::new(self.view.width as i32 - CUBE as i32 - 6, 6, CUBE, CUBE)
    }
}
