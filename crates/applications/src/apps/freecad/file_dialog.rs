//! FreeCAD 1.0 asks the platform for its file dialogs, so each desktop shows its own:
//! the Mac's NSSavePanel/NSOpenPanel sheet, Windows 11's common item dialog, and
//! GNOME's GTK file chooser. All three draw the same `FileDialog` state over the
//! machine's real folders; the sidebars list the same standard places Finder, Explorer
//! and Files list (`desktop_scene::standard_places`). Places a dialog cannot show here
//! are left out rather than painted as dead rows: Recents/Recent and Starred are the
//! desktop's lists, which an application does not see, and Explorer's Gallery collects
//! images, none of which a FreeCAD file type accepts.
use super::files::{filters, FileDialog, Purpose};
use super::layout::Layout;
use super::*;
use crate::desktop_scene::shared::Align;
use crate::desktop_scene::{kind_label, standard_places, PlaceKind, SideItem};
use cw_scene::{Color, Rect};

fn over(pointer: Option<(i32, i32)>, r: Rect) -> bool {
    pointer.is_some_and(|(x, y)| r.contains(x, y))
}

/// A sidebar row as a dialog shows it: label, glyph, tint, pinned, target, lit.
struct Row {
    label: String,
    symbol: &'static str,
    tint: Option<Color>,
    pinned: bool,
    target: String,
    current: bool,
}
enum Side {
    Heading(&'static str),
    Gap,
    Row(Row),
}

/// The standard places, as targets this dialog can carry out.
fn places(theme: DesktopTheme, env: &crate::AppEnv<'_>, d: &FileDialog) -> Vec<Side> {
    let at =
        |path: &str| !d.home_view && d.folder.trim_end_matches('/') == path.trim_end_matches('/');
    let mut out: Vec<Side> = vec![];
    for item in standard_places(theme, &env.files) {
        match item {
            SideItem::Heading(h) => out.push(Side::Heading(h)),
            SideItem::Gap => out.push(Side::Gap),
            SideItem::Place(place) => {
                let (target, current) = match &place.kind {
                    PlaceKind::Home(path) | PlaceKind::Folder(path) | PlaceKind::Trash(path) => {
                        (format!("freecad:file:place:{path}"), at(path))
                    }
                    PlaceKind::Root => ("freecad:file:place:/".to_owned(), at("/")),
                    PlaceKind::QuickAccess => ("freecad:file:home".to_owned(), d.home_view),
                    PlaceKind::Recents | PlaceKind::Starred | PlaceKind::Gallery => continue,
                };
                out.push(Side::Row(Row {
                    label: place.label,
                    symbol: place.symbol,
                    tint: place.tint,
                    pinned: place.pinned,
                    target,
                    current,
                }));
            }
        }
    }
    // A heading with nothing under it (Favorites once Recents is left out) stays.
    out
}

/// The folder and every folder above it, root first: (label, path).
fn crumbs(folder: &str, root: &str) -> Vec<(String, String)> {
    let mut out = vec![(root.to_owned(), "/".to_owned())];
    let mut prefix = String::new();
    for part in folder.split('/').filter(|s| !s.is_empty()) {
        prefix.push('/');
        prefix.push_str(part);
        out.push((part.to_owned(), prefix.clone()));
    }
    out
}
fn folder_name(folder: &str, root: &str) -> String {
    folder
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(root)
        .to_owned()
}

/// What the name box shows: what is being typed, else the dialog's name.
fn name_text(cad: &Cad, d: &FileDialog) -> (String, bool) {
    match &cad.field {
        Some(Field {
            target: FieldTarget::FileName,
            text,
            ..
        }) => (text.clone(), true),
        _ => (d.name.clone(), false),
    }
}
fn folder_text(cad: &Cad) -> (String, bool) {
    match &cad.field {
        Some(Field {
            target: FieldTarget::FolderName,
            text,
            ..
        }) => (text.clone(), true),
        _ => (String::new(), false),
    }
}
fn type_label(d: &FileDialog) -> String {
    match d.purpose {
        Purpose::Import => {
            let exts: Vec<String> = filters(d.purpose)
                .iter()
                .map(|f| format!("*.{}", f.1))
                .collect();
            format!("Supported formats ({})", exts.join(" "))
        }
        _ => filters(d.purpose)
            .get(d.filter)
            .map(|f| f.0.to_owned())
            .unwrap_or_default(),
    }
}
/// The file type pop-up/combo is live when there is a choice to make. Import lists
/// every format FreeCAD reads at once, as its own dialog's first filter does.
fn type_choice(d: &FileDialog) -> Result<(), String> {
    if d.purpose == Purpose::Import {
        Err("Import shows every format FreeCAD reads at once".into())
    } else if filters(d.purpose).len() < 2 {
        Err("FreeCAD documents are the only type this dialog saves".into())
    } else {
        Ok(())
    }
}
/// What the New Folder prompt says under the name: the problem with what is typed
/// (checked as it is typed), else why the last Create was refused.
fn folder_error(cad: &Cad, d: &FileDialog, text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return d.prompt.as_ref().and_then(|p| p.error.clone());
    }
    cad.folder_name_problem()
}
/// Can the main button act now.
fn can_ok(cad: &Cad, d: &FileDialog) -> Result<(), String> {
    let (name, _) = name_text(cad, d);
    let folder_chosen = d.selected.as_ref().is_some_and(|s| s.ends_with('/'));
    if d.loading {
        return Err("The folder is still loading".into());
    }
    if d.purpose.saving() {
        if d.home_view {
            return Err("Choose a folder to save in".into());
        }
        if name.trim().is_empty() {
            return Err("Type a file name".into());
        }
        return Ok(());
    }
    if folder_chosen || !name.trim().is_empty() {
        Ok(())
    } else {
        Err("Choose a file".into())
    }
}

/// A painted control: its plate, its label, and the target — or, when it cannot act,
/// the reason, announced.
#[allow(clippy::too_many_arguments)]
fn control(
    p: &mut Painter,
    r: Rect,
    fill: Color,
    border: Option<Color>,
    radius: u32,
    label: &str,
    size: u16,
    ink: Color,
    bold: bool,
    target: &str,
    enabled: Result<(), String>,
) {
    let dim = |c: Color| Color(c.0, c.1, c.2, c.3 / 2 + 20);
    match border {
        Some(b) => p.border(r, fill, radius, b),
        None => p.box_(r, fill, radius),
    }
    if !label.is_empty() {
        p.label(
            r.x,
            r.y + (r.height as i32 - (size as i32 + size as i32 / 2 + 2)) / 2,
            r.width,
            label,
            size,
            if enabled.is_ok() { ink } else { dim(ink) },
            bold,
            Align::Center,
        );
    }
    let name = if label.is_empty() { target } else { label };
    match enabled {
        Ok(()) => p.region(r, target, name),
        Err(why) => {
            p.box_(r, Color::TRANSPARENT, radius);
            p.disabled(&format!("{name}: {why}"));
        }
    }
}
/// A glyph-only button.
#[allow(clippy::too_many_arguments)]
fn icon_button(
    p: &mut Painter,
    r: Rect,
    symbol: &str,
    size: u32,
    ink: Color,
    hover: Color,
    pointer: Option<(i32, i32)>,
    target: &str,
    label: &str,
    enabled: Result<(), String>,
) {
    let live = enabled.is_ok();
    if live && over(pointer, r) {
        p.box_(r, hover, 5);
    }
    let c = if live {
        ink
    } else {
        Color(ink.0, ink.1, ink.2, 90)
    };
    p.symbol(
        symbol,
        r.x + (r.width as i32 - size as i32) / 2,
        r.y + (r.height as i32 - size as i32) / 2,
        size,
        c,
    );
    match enabled {
        Ok(()) => p.region(r, target, label),
        Err(why) => {
            p.box_(r, Color::TRANSPARENT, 0);
            p.disabled(&format!("{label}: {why}"));
        }
    }
}
/// A one-line text box with a caret when it has the keyboard.
#[allow(clippy::too_many_arguments)]
fn text_box(
    p: &mut Painter,
    r: Rect,
    text: &str,
    focused: bool,
    fill: Color,
    edge: Color,
    focus: Color,
    radius: u32,
    size: u16,
    target: &str,
    label: &str,
) {
    p.border(r, fill, radius, if focused { focus } else { edge });
    if focused {
        // The platforms draw a focus ring outside the box.
        p.border(
            Rect::new(r.x - 1, r.y - 1, r.width + 2, r.height + 2),
            Color::TRANSPARENT,
            radius + 1,
            Color(focus.0, focus.1, focus.2, 120),
        );
    }
    let ty = r.y + (r.height as i32 - (size as i32 + size as i32 / 2 + 2)) / 2;
    let tw = p.left(
        r.x + 7,
        ty,
        r.width.saturating_sub(14),
        text,
        size,
        Color::rgb(20, 20, 20),
    );
    if focused {
        p.vline(
            r.x + 8 + tw as i32,
            r.y + 4,
            r.height.saturating_sub(8),
            Color::rgb(20, 20, 20),
        );
    }
    p.region(r, target, label);
}
/// Rows of the listing, clipped to `area`, painted by `row`.
fn listing(
    p: &mut Painter,
    area: Rect,
    d: &FileDialog,
    row_h: u32,
    mut row: impl FnMut(&mut Painter, Rect, &str, bool),
) {
    let mark = p.scene.nodes.len();
    let mut y = area.y;
    for e in d.visible() {
        if y + row_h as i32 > area.y + area.height as i32 {
            break;
        }
        let r = Rect::new(area.x, y, area.width, row_h);
        let selected = d.selected.as_deref() == Some(e.as_str())
            || (!e.ends_with('/') && d.selected.is_none() && *e == d.name);
        row(p, r, e, selected);
        p.region(
            r,
            &format!("freecad:file:entry:{e}"),
            e.trim_end_matches('/'),
        );
        y += row_h as i32;
    }
    for n in &mut p.scene.nodes[mark..] {
        n.clip = Some(area);
    }
}
/// "Loading…", a listing error, or an empty folder, in the list's own voice.
fn list_notice(p: &mut Painter, area: Rect, d: &FileDialog, empty: &str, ink: Color) {
    let text = if d.loading {
        "Loading…".to_owned()
    } else if let Some(e) = &d.error {
        e.clone()
    } else if d.visible().is_empty() {
        empty.to_owned()
    } else {
        return;
    };
    p.center(
        area.x,
        area.y + area.height as i32 / 2 - 10,
        area.width,
        &text,
        13,
        ink,
    );
}
/// The whole window dims and holds the pointer while the dialog is up.
fn modal(p: &mut Painter, l: &Layout, dim: u8) {
    p.z += 200;
    p.box_(Rect::new(0, 0, l.w, l.h), Color(0, 0, 0, dim), 0);
    p.region(
        Rect::new(0, 0, l.w, l.h),
        "freecad:dialog:block",
        "Finish the dialog first",
    );
}
/// A second modal layer over the dialog itself (New Folder sheet, Replace alert).
fn over_dialog(p: &mut Painter, r: Rect, dim: u8) {
    p.z += 20;
    p.box_(r, Color(0, 0, 0, dim), 0);
    p.region(r, "freecad:dialog:block", "Answer the question first");
    p.z += 1;
}

pub fn draw(cad: &Cad, p: &mut Painter, l: &Layout, d: &FileDialog, env: &crate::AppEnv<'_>) {
    match env.theme {
        DesktopTheme::Macos => mac(cad, p, l, d, env),
        DesktopTheme::Windows => windows(cad, p, l, d, env),
        _ => gnome(cad, p, l, d, env),
    }
}

// ---------------------------------------------------------------------------------
// macOS: NSSavePanel / NSOpenPanel, as a sheet hanging from the top of the window.

const MAC_BLUE: Color = Color::rgb(0, 122, 255);
const MAC_INK: Color = Color::rgb(29, 29, 31);
const MAC_DIM: Color = Color::rgb(128, 128, 133);
const MAC_LINE: Color = Color(0, 0, 0, 30);

fn mac_button(
    p: &mut Painter,
    r: Rect,
    label: &str,
    target: &str,
    default: bool,
    ok: Result<(), String>,
) {
    if default && ok.is_ok() {
        control(
            p,
            r,
            MAC_BLUE,
            None,
            6,
            label,
            13,
            Color::WHITE,
            false,
            target,
            ok,
        );
    } else {
        control(
            p,
            r,
            Color::WHITE,
            Some(Color(0, 0, 0, 40)),
            6,
            label,
            13,
            MAC_INK,
            false,
            target,
            ok,
        );
    }
}
/// A pop-up button: the choice and the double chevron.
fn mac_popup(
    p: &mut Painter,
    r: Rect,
    text: &str,
    target: &str,
    label: &str,
    ok: Result<(), String>,
) {
    let live = ok.is_ok();
    p.border(r, Color::WHITE, 6, Color(0, 0, 0, 40));
    p.left(
        r.x + 9,
        r.y + (r.height as i32 - 21) / 2,
        r.width.saturating_sub(34),
        text,
        13,
        if live { MAC_INK } else { MAC_DIM },
    );
    let chip = Rect::new(r.x + r.width as i32 - 20, r.y + 3, 16, r.height - 6);
    p.box_(
        chip,
        if live {
            MAC_BLUE
        } else {
            Color::rgb(200, 200, 204)
        },
        4,
    );
    p.symbol(
        "chevron-up",
        chip.x + 4,
        chip.y + chip.height as i32 / 2 - 8,
        8,
        Color::WHITE,
    );
    p.symbol(
        "chevron-down",
        chip.x + 4,
        chip.y + chip.height as i32 / 2,
        8,
        Color::WHITE,
    );
    match ok {
        Ok(()) => p.region(r, target, label),
        Err(why) => {
            p.box_(r, Color::TRANSPARENT, 6);
            p.disabled(&format!("{label}: {why}"));
        }
    }
}

fn mac(cad: &Cad, p: &mut Painter, l: &Layout, d: &FileDialog, env: &crate::AppEnv<'_>) {
    let pointer = env.pointer;
    modal(p, l, 30);
    let saving = d.purpose.saving();
    let collapsed = saving && d.collapsed;
    let w = l.w.saturating_sub(40).min(720);
    let h = if collapsed {
        200
    } else {
        l.h.saturating_sub(30).min(500)
    };
    // A sheet slides out from under the title bar, centred on the window.
    let r = Rect::new((l.w as i32 - w as i32) / 2, 0, w, h);
    p.drop_shadow(r, 10, 22, 90, 8);
    p.border(r, Color::rgb(236, 236, 236), 10, Color(0, 0, 0, 50));
    let mut y = r.y + 16;
    let root = "Macintosh HD";
    if saving {
        // "Save As:" and "Tags:" sit centred at the top of a save panel.
        let lw = 70;
        let fw = (w as i32 - 2 * lw - 60).clamp(160, 300) as u32;
        let fx = r.x + (w as i32 - fw as i32) / 2 + 20;
        p.right(fx - lw - 8, y + 3, lw as u32, "Save As:", 13, MAC_INK);
        let (name, focused) = name_text(cad, d);
        text_box(
            p,
            Rect::new(fx, y, fw, 24),
            &name,
            focused,
            Color::WHITE,
            Color(0, 0, 0, 40),
            MAC_BLUE,
            5,
            13,
            "freecad:field:file-name",
            "Save As",
        );
        // The disclosure button folds the browser away and back.
        icon_button(
            p,
            Rect::new(fx + fw as i32 + 10, y, 24, 24),
            if collapsed {
                "chevron-down"
            } else {
                "chevron-up"
            },
            12,
            MAC_INK,
            Color(0, 0, 0, 20),
            pointer,
            "freecad:file:collapse",
            if collapsed {
                "Show the folder browser"
            } else {
                "Hide the folder browser"
            },
            Ok(()),
        );
        y += 32;
        p.right(fx - lw - 8, y + 3, lw as u32, "Tags:", 13, MAC_INK);
        let tags = Rect::new(fx, y, fw, 24);
        p.border(tags, Color(255, 255, 255, 140), 5, Color(0, 0, 0, 25));
        p.disabled("Tags: Finder tags are not kept by this machine's file system");
        y += 36;
    }
    let (fx, fw) = (r.x + 20, w - 40);
    if collapsed {
        // Where: the folder pop-up alone.
        p.right(r.x + w as i32 / 2 - 190, y + 3, 100, "Where:", 13, MAC_INK);
        mac_popup(
            p,
            Rect::new(r.x + w as i32 / 2 - 82, y, 260, 24),
            &folder_name(&d.folder, root),
            "freecad:choice:open:filepath",
            "Where",
            Ok(()),
        );
        y += 36;
    } else {
        // Toolbar: Back, Forward, the folder pop-up.
        let back = Rect::new(fx, y, 28, 24);
        p.border(
            Rect::new(fx, y, 57, 24),
            Color::WHITE,
            6,
            Color(0, 0, 0, 30),
        );
        icon_button(
            p,
            back,
            "chevron-left",
            12,
            MAC_INK,
            Color(0, 0, 0, 20),
            pointer,
            "freecad:file:back",
            "Back",
            if d.back.is_empty() {
                Err("There is nowhere to go back to".into())
            } else {
                Ok(())
            },
        );
        p.vline(fx + 28, y + 4, 16, MAC_LINE);
        icon_button(
            p,
            Rect::new(fx + 29, y, 28, 24),
            "chevron-right",
            12,
            MAC_INK,
            Color(0, 0, 0, 20),
            pointer,
            "freecad:file:forward",
            "Forward",
            if d.forward.is_empty() {
                Err("There is nowhere to go forward to".into())
            } else {
                Ok(())
            },
        );
        mac_popup(
            p,
            Rect::new(r.x + w as i32 / 2 - 110, y, 220, 24),
            &folder_name(&d.folder, root),
            "freecad:choice:open:filepath",
            "Folder",
            Ok(()),
        );
        y += 34;
        // Browser: sidebar and the folder's list.
        let bh = (r.y + h as i32 - y - 96).max(80) as u32;
        let area = Rect::new(fx, y, fw, bh);
        p.border(area, Color::WHITE, 0, MAC_LINE);
        let side = Rect::new(area.x + 1, area.y + 1, 160, bh - 2);
        p.box_(side, Color::rgb(232, 231, 234), 0);
        p.vline(side.x + side.width as i32, side.y, side.height, MAC_LINE);
        let mut sy = side.y + 4;
        for item in places(DesktopTheme::Macos, env, d) {
            if sy + 24 > side.y + side.height as i32 {
                break;
            }
            match item {
                Side::Heading(t) => {
                    sy += 4;
                    p.strong(side.x + 10, sy, side.width - 20, t, 11, MAC_DIM);
                    sy += 20;
                }
                Side::Gap => sy += 8,
                Side::Row(row) => {
                    let rr = Rect::new(side.x + 6, sy, side.width - 12, 24);
                    p.button(
                        rr,
                        if row.current {
                            Color(0, 0, 0, 24)
                        } else {
                            Color::TRANSPARENT
                        },
                        5,
                        &row.target,
                        &row.label,
                    );
                    p.symbol(row.symbol, rr.x + 8, rr.y + 4, 16, MAC_BLUE);
                    p.left(rr.x + 32, rr.y + 2, rr.width - 36, &row.label, 13, MAC_INK);
                    sy += 24;
                }
            }
        }
        // List view: Name and Kind (the listing carries no dates or sizes).
        let list = Rect::new(
            side.x + side.width as i32 + 1,
            area.y + 1,
            area.width - side.width - 3,
            bh - 2,
        );
        let kind_w = (list.width / 3).min(180);
        p.left(
            list.x + 30,
            list.y + 3,
            list.width - kind_w - 40,
            "Name",
            11,
            MAC_DIM,
        );
        p.left(
            list.x + list.width as i32 - kind_w as i32,
            list.y + 3,
            kind_w,
            "Kind",
            11,
            MAC_DIM,
        );
        p.hline(list.x, list.y + 22, list.width, MAC_LINE);
        let rows = Rect::new(list.x, list.y + 24, list.width, list.height - 24);
        list_notice(p, rows, d, "", MAC_DIM);
        let mut odd = false;
        listing(p, rows, d, 22, |p, rr, e, sel| {
            if sel {
                p.box_(
                    Rect::new(rr.x + 4, rr.y, rr.width - 8, rr.height),
                    MAC_BLUE,
                    4,
                );
            } else if odd {
                p.box_(rr, Color::rgb(244, 245, 245), 0);
            }
            odd = !odd;
            let ink = if sel { Color::WHITE } else { MAC_INK };
            if e.ends_with('/') {
                p.symbol(
                    "folder",
                    rr.x + 10,
                    rr.y + 3,
                    16,
                    if sel {
                        Color::WHITE
                    } else {
                        Color::rgb(58, 160, 240)
                    },
                );
            } else {
                p.symbol(
                    "document",
                    rr.x + 10,
                    rr.y + 3,
                    16,
                    if sel { Color::WHITE } else { MAC_DIM },
                );
            }
            p.left(
                rr.x + 32,
                rr.y + 2,
                rr.width - kind_w - 40,
                e.trim_end_matches('/'),
                13,
                ink,
            );
            p.left(
                rr.x + rr.width as i32 - kind_w as i32,
                rr.y + 2,
                kind_w - 6,
                &kind_label(DesktopTheme::Macos, e),
                13,
                if sel { Color::WHITE } else { MAC_DIM },
            );
        });
        y += bh as i32 + 12;
    }
    // File Format (save) or the file type the panel opens.
    p.right(
        r.x + w as i32 / 2 - 150,
        y + 3,
        100,
        if saving { "File Format:" } else { "Format:" },
        13,
        MAC_INK,
    );
    mac_popup(
        p,
        Rect::new(r.x + w as i32 / 2 - 42, y, 250, 24),
        &type_label(d),
        "freecad:choice:open:filetype",
        "File Format",
        type_choice(d),
    );
    // Bottom bar: New Folder at the left; Cancel and the default button at the right.
    let by = r.y + h as i32 - 40;
    p.hline(r.x, by - 10, w, MAC_LINE);
    // An open panel makes no folders; a save panel does.
    if saving && !collapsed {
        mac_button(
            p,
            Rect::new(r.x + 20, by, 100, 26),
            "New Folder",
            "freecad:file:folder-prompt:untitled folder",
            false,
            if d.home_view || d.loading {
                Err("Open a folder first".into())
            } else {
                Ok(())
            },
        );
    }
    let ok = Rect::new(r.x + w as i32 - 110, by, 90, 26);
    mac_button(
        p,
        ok,
        d.purpose.button(),
        "freecad:file:ok",
        true,
        can_ok(cad, d),
    );
    mac_button(
        p,
        Rect::new(ok.x - 100, by, 90, 26),
        "Cancel",
        "freecad:file:cancel",
        false,
        Ok(()),
    );

    if d.prompt.is_some() {
        // The New Folder sheet drops from the top of the panel.
        over_dialog(p, r, 25);
        let sw = 360;
        let s = Rect::new(r.x + (w as i32 - sw) / 2, r.y, sw as u32, 170);
        p.drop_shadow(s, 10, 16, 80, 6);
        p.border(s, Color::rgb(240, 240, 240), 10, Color(0, 0, 0, 50));
        p.strong(s.x + 20, s.y + 16, s.width - 40, "New Folder", 13, MAC_INK);
        p.left(
            s.x + 20,
            s.y + 40,
            s.width - 40,
            &format!(
                "Name of new folder inside “{}”:",
                folder_name(&d.folder, root)
            ),
            12,
            MAC_INK,
        );
        let (text, focused) = folder_text(cad);
        text_box(
            p,
            Rect::new(s.x + 20, s.y + 64, s.width - 40, 24),
            &text,
            focused,
            Color::WHITE,
            Color(0, 0, 0, 40),
            MAC_BLUE,
            5,
            13,
            "freecad:field:folder-name",
            "Name of new folder",
        );
        if let Some(e) = folder_error(cad, d, &text) {
            p.left(
                s.x + 20,
                s.y + 94,
                s.width - 40,
                &e,
                11,
                Color::rgb(215, 0, 21),
            );
        }
        let create = Rect::new(s.x + s.width as i32 - 100, s.y + 128, 80, 26);
        mac_button(
            p,
            create,
            "Create",
            "freecad:file:folder-create",
            true,
            cad.folder_name_problem().map_or(Ok(()), Err),
        );
        mac_button(
            p,
            Rect::new(create.x - 90, create.y, 80, 26),
            "Cancel",
            "freecad:file:folder-cancel",
            false,
            Ok(()),
        );
    }
    if let Some(name) = &d.confirm {
        // NSSavePanel's replace alert: Cancel is the default button.
        over_dialog(p, r, 25);
        let aw = 300;
        let a = Rect::new(r.x + (w as i32 - aw) / 2, r.y + 30, aw as u32, 250);
        p.drop_shadow(a, 12, 20, 90, 8);
        p.border(a, Color::rgb(242, 242, 242), 12, Color(0, 0, 0, 50));
        super::icons::draw(p, "FreeCAD", a.x + aw / 2 - 28, a.y + 18, 56, true);
        let th = p.paragraph(
            a.x + 18,
            a.y + 86,
            a.width - 36,
            &format!("“{name}” already exists. Do you want to replace it?"),
            13,
            MAC_INK,
        );
        p.paragraph(
            a.x + 18,
            a.y + 92 + th as i32,
            a.width - 36,
            &format!(
                "A file or folder with the same name already exists in the folder {}. Replacing it will overwrite its current contents.",
                folder_name(&d.folder, root)
            ),
            11,
            MAC_INK,
        );
        let bw = (a.width - 46) / 2;
        let cancel = Rect::new(a.x + 18, a.y + a.height as i32 - 42, bw, 26);
        mac_button(p, cancel, "Cancel", "freecad:file:keep", true, Ok(()));
        mac_button(
            p,
            Rect::new(cancel.x + bw as i32 + 10, cancel.y, bw, 26),
            "Replace",
            "freecad:file:replace",
            false,
            Ok(()),
        );
    }
}

// ---------------------------------------------------------------------------------
// Windows 11: the common item dialog (File Explorer's Save As / Open).

const WIN_ACCENT: Color = Color::rgb(0, 95, 184);
const WIN_INK: Color = Color::rgb(26, 26, 26);
const WIN_DIM: Color = Color::rgb(96, 96, 96);
const WIN_LINE: Color = Color::rgb(229, 229, 229);
const WIN_HOVER: Color = Color(0, 0, 0, 12);

fn win_button(
    p: &mut Painter,
    r: Rect,
    label: &str,
    target: &str,
    accent: bool,
    ok: Result<(), String>,
) {
    if accent && ok.is_ok() {
        control(
            p,
            r,
            WIN_ACCENT,
            None,
            4,
            label,
            12,
            Color::WHITE,
            false,
            target,
            ok,
        );
    } else {
        control(
            p,
            r,
            Color::rgb(251, 251, 251),
            Some(Color::rgb(210, 210, 210)),
            4,
            label,
            12,
            WIN_INK,
            false,
            target,
            ok,
        );
    }
}
/// A combo box: text and the chevron.
fn win_combo(
    p: &mut Painter,
    r: Rect,
    text: &str,
    target: &str,
    label: &str,
    ok: Result<(), String>,
) {
    let live = ok.is_ok();
    p.border(r, Color::WHITE, 4, Color::rgb(210, 210, 210));
    p.left(
        r.x + 8,
        r.y + (r.height as i32 - 19) / 2,
        r.width.saturating_sub(34),
        text,
        12,
        if live { WIN_INK } else { WIN_DIM },
    );
    p.symbol(
        "chevron-down",
        r.x + r.width as i32 - 22,
        r.y + r.height as i32 / 2 - 5,
        10,
        WIN_DIM,
    );
    match ok {
        Ok(()) => p.region(r, target, label),
        Err(why) => {
            p.box_(r, Color::TRANSPARENT, 4);
            p.disabled(&format!("{label}: {why}"));
        }
    }
}

fn windows(cad: &Cad, p: &mut Painter, l: &Layout, d: &FileDialog, env: &crate::AppEnv<'_>) {
    let pointer = env.pointer;
    modal(p, l, 0);
    let saving = d.purpose.saving();
    let w = l.w.saturating_sub(40).min(940);
    let h = l.h.saturating_sub(30).min(600);
    let r = Rect::new(
        (l.w as i32 - w as i32) / 2,
        (l.h as i32 - h as i32) / 2,
        w,
        h,
    );
    p.drop_shadow(r, 8, 24, 80, 8);
    p.border(r, Color::rgb(243, 243, 243), 8, Color(0, 0, 0, 50));
    // Title bar.
    let title = if saving { "Save As" } else { "Open" };
    super::icons::draw(p, "FreeCAD", r.x + 12, r.y + 9, 16, true);
    p.left(r.x + 36, r.y + 7, w - 100, title, 12, WIN_INK);
    let close = Rect::new(r.x + w as i32 - 46, r.y + 1, 45, 30);
    if over(pointer, close) {
        p.box_(close, Color::rgb(196, 43, 28), 0);
    }
    p.symbol(
        "close",
        close.x + 17,
        close.y + 10,
        10,
        if over(pointer, close) {
            Color::WHITE
        } else {
            WIN_INK
        },
    );
    p.region(close, "freecad:file:cancel", "Close");
    // Navigation row: Back, Forward, Up and the breadcrumb address bar.
    let ny = r.y + 38;
    let nav =
        |p: &mut Painter, x: i32, sym: &str, target: &str, label: &str, ok: Result<(), String>| {
            icon_button(
                p,
                Rect::new(x, ny, 32, 30),
                sym,
                14,
                WIN_INK,
                WIN_HOVER,
                pointer,
                target,
                label,
                ok,
            );
        };
    nav(
        p,
        r.x + 10,
        "arrow-left",
        "freecad:file:back",
        "Back",
        if d.back.is_empty() {
            Err("There is nowhere to go back to".into())
        } else {
            Ok(())
        },
    );
    nav(
        p,
        r.x + 44,
        "arrow-right",
        "freecad:file:forward",
        "Forward",
        if d.forward.is_empty() {
            Err("There is nowhere to go forward to".into())
        } else {
            Ok(())
        },
    );
    nav(
        p,
        r.x + 78,
        "arrow-up",
        "freecad:file:up",
        "Up",
        if d.home_view || d.folder == "/" {
            Err("Already at the top".into())
        } else {
            Ok(())
        },
    );
    let bar = Rect::new(r.x + 118, ny, w - 128, 30);
    p.border(bar, Color::WHITE, 4, Color::rgb(215, 215, 215));
    let mut x = bar.x + 8;
    if d.home_view {
        p.symbol("home", x, ny + 7, 16, WIN_ACCENT);
        x += 22;
        p.symbol("chevron-right", x, ny + 10, 10, WIN_DIM);
        x += 14;
        let tw = p.measure("Home", 12, false) + 12;
        let seg = Rect::new(x, ny + 3, tw, 24);
        control(
            p,
            seg,
            Color::TRANSPARENT,
            None,
            3,
            "Home",
            12,
            WIN_INK,
            false,
            "freecad:file:home",
            Ok(()),
        );
    } else {
        p.symbol("desktop", x, ny + 7, 16, WIN_DIM);
        x += 22;
        let segs = crumbs(&d.folder, "This PC");
        // A long trail keeps its deepest folders, as Explorer's does.
        let avail = bar.width as i32 - 40;
        let widths: Vec<i32> = segs
            .iter()
            .map(|(s, _)| p.measure(s, 12, false) as i32 + 26)
            .collect();
        let mut first = 0;
        while first + 1 < segs.len() && widths[first..].iter().sum::<i32>() > avail {
            first += 1;
        }
        for (i, (label, path)) in segs.iter().enumerate().skip(first) {
            p.symbol("chevron-right", x, ny + 10, 10, WIN_DIM);
            x += 14;
            let tw = widths[i] as u32 - 14;
            let seg = Rect::new(x, ny + 3, tw, 24);
            if over(pointer, seg) {
                p.box_(seg, WIN_HOVER, 3);
            }
            control(
                p,
                seg,
                Color::TRANSPARENT,
                None,
                3,
                label,
                12,
                WIN_INK,
                false,
                &format!("freecad:file:crumb:{path}"),
                Ok(()),
            );
            x += tw as i32;
        }
    }
    // Command bar: Organize (nothing behind it here) and New folder.
    let cy = ny + 38;
    p.hline(r.x, cy - 3, w, WIN_LINE);
    let organize = Rect::new(r.x + 10, cy, 90, 28);
    p.left(organize.x + 8, cy + 5, 60, "Organize", 12, WIN_DIM);
    p.symbol("chevron-down", organize.x + 70, cy + 10, 9, WIN_DIM);
    p.box_(organize, Color::TRANSPARENT, 4);
    p.disabled("Organize: cut, copy, rename and delete belong to File Explorer, not this dialog");
    let nf = Rect::new(r.x + 106, cy, 100, 28);
    let nf_ok = if d.home_view || d.loading {
        Err("Open a folder first".to_owned())
    } else {
        Ok(())
    };
    if nf_ok.is_ok() && over(pointer, nf) {
        p.box_(nf, WIN_HOVER, 4);
    }
    p.left(
        nf.x + 10,
        cy + 5,
        90,
        "New folder",
        12,
        if nf_ok.is_ok() { WIN_INK } else { WIN_DIM },
    );
    match nf_ok {
        Ok(()) => p.region(nf, "freecad:file:new-folder", "New folder"),
        Err(why) => {
            p.box_(nf, Color::TRANSPARENT, 4);
            p.disabled(&format!("New folder: {why}"));
        }
    }
    // Body: navigation pane and the details view.
    let top = cy + 36;
    p.hline(r.x, top - 4, w, WIN_LINE);
    let bottom_h: i32 = if saving { 118 } else { 96 };
    let body_h = (r.y + h as i32 - bottom_h - top).max(60) as u32;
    let pane = Rect::new(r.x + 1, top, 200, body_h);
    p.box_(pane, Color::rgb(249, 249, 249), 0);
    p.vline(pane.x + pane.width as i32, top, body_h, WIN_LINE);
    let mut sy = top + 4;
    for item in places(DesktopTheme::Windows, env, d) {
        if sy + 30 > top + body_h as i32 {
            break;
        }
        match item {
            Side::Heading(_) => {}
            Side::Gap => {
                p.hline(pane.x + 12, sy + 5, pane.width - 24, WIN_LINE);
                sy += 11;
            }
            Side::Row(row) => {
                let rr = Rect::new(pane.x + 6, sy, pane.width - 12, 30);
                if row.current {
                    p.box_(rr, Color::rgb(229, 241, 251), 4);
                    p.box_(Rect::new(rr.x, rr.y + 8, 3, 14), WIN_ACCENT, 2);
                } else if over(pointer, rr) {
                    p.box_(rr, WIN_HOVER, 4);
                }
                p.symbol(
                    row.symbol,
                    rr.x + 14,
                    rr.y + 7,
                    16,
                    row.tint.unwrap_or(WIN_ACCENT),
                );
                p.left(rr.x + 40, rr.y + 5, rr.width - 70, &row.label, 12, WIN_INK);
                if row.pinned {
                    p.symbol(
                        "pin",
                        rr.x + rr.width as i32 - 22,
                        rr.y + 9,
                        12,
                        Color::rgb(160, 160, 160),
                    );
                }
                p.region(rr, &row.target, &row.label);
                sy += 30;
            }
        }
    }
    let view = Rect::new(
        pane.x + pane.width as i32 + 1,
        top,
        w - pane.width - 3,
        body_h,
    );
    let type_w = 180u32.min(view.width / 3);
    let name_w = view.width - type_w;
    // Details columns: Name and Type (the listing carries no dates or sizes).
    p.left(
        view.x + 36,
        top + 4,
        name_w - 40,
        if d.home_view { "Quick access" } else { "Name" },
        12,
        WIN_DIM,
    );
    if !d.home_view {
        p.vline(view.x + name_w as i32 - 8, top + 4, 18, WIN_LINE);
        p.left(
            view.x + name_w as i32,
            top + 4,
            type_w - 8,
            "Type",
            12,
            WIN_DIM,
        );
    }
    let rows = Rect::new(view.x, top + 28, view.width, body_h - 28);
    list_notice(p, rows, d, "This folder is empty.", WIN_DIM);
    listing(p, rows, d, 28, |p, rr, e, sel| {
        let plate = Rect::new(rr.x + 4, rr.y + 1, rr.width - 8, rr.height - 2);
        if sel {
            p.box_(plate, Color::rgb(204, 232, 255), 4);
        } else if over(pointer, plate) {
            p.box_(plate, Color::rgb(229, 243, 255), 4);
        }
        let is_dir = e.ends_with('/');
        let name = e.trim_end_matches('/');
        if is_dir {
            let tint = if d.home_view {
                None
            } else {
                Some(Color::rgb(245, 187, 64))
            };
            p.symbol(
                "folder",
                rr.x + 12,
                rr.y + 6,
                16,
                tint.unwrap_or(Color::rgb(245, 187, 64)),
            );
        } else {
            p.symbol(
                "document",
                rr.x + 12,
                rr.y + 6,
                16,
                Color::rgb(120, 120, 120),
            );
        }
        p.left(rr.x + 36, rr.y + 5, name_w - 44, name, 12, WIN_INK);
        if !d.home_view {
            p.left(
                rr.x + name_w as i32,
                rr.y + 5,
                type_w - 8,
                &kind_label(DesktopTheme::Windows, e),
                12,
                WIN_DIM,
            );
        }
    });
    // Bottom: File name, the type filter, and the buttons.
    let fy = top + body_h as i32 + 12;
    p.hline(r.x, fy - 8, w, WIN_LINE);
    let (name, focused) = name_text(cad, d);
    let lx = r.x + 20;
    let fx = r.x + 130;
    p.right(lx, fy + 5, 100, "File name:", 12, WIN_INK);
    let field_w = if saving { w - 150 } else { w - 150 - 250 };
    text_box(
        p,
        Rect::new(fx, fy, field_w, 28),
        &name,
        focused,
        Color::WHITE,
        Color::rgb(210, 210, 210),
        WIN_ACCENT,
        4,
        12,
        "freecad:field:file-name",
        "File name",
    );
    let by = if saving {
        p.right(lx, fy + 41, 100, "Save as type:", 12, WIN_INK);
        win_combo(
            p,
            Rect::new(fx, fy + 36, field_w, 28),
            &type_label(d),
            "freecad:choice:open:filetype",
            "Save as type",
            type_choice(d),
        );
        fy + 76
    } else {
        win_combo(
            p,
            Rect::new(fx + field_w as i32 + 10, fy, 240 - 10, 28),
            &type_label(d),
            "freecad:choice:open:filetype",
            "File type",
            type_choice(d),
        );
        fy + 40
    };
    let cancel = Rect::new(r.x + w as i32 - 20 - 100, by, 100, 30);
    win_button(p, cancel, "Cancel", "freecad:file:cancel", false, Ok(()));
    win_button(
        p,
        Rect::new(cancel.x - 110, by, 100, 30),
        d.purpose.button(),
        "freecad:file:ok",
        true,
        can_ok(cad, d),
    );

    if let Some(name) = &d.confirm {
        // "Confirm Save As": No is the default button.
        over_dialog(p, r, 0);
        let (aw, ah) = (380u32, 170u32);
        let a = Rect::new(
            r.x + (w as i32 - aw as i32) / 2,
            r.y + (h as i32 - ah as i32) / 2,
            aw,
            ah,
        );
        p.drop_shadow(a, 8, 20, 90, 8);
        p.border(a, Color::WHITE, 8, Color(0, 0, 0, 60));
        p.left(a.x + 14, a.y + 8, aw - 60, "Confirm Save As", 12, WIN_INK);
        p.region(
            Rect::new(a.x + aw as i32 - 46, a.y + 1, 45, 30),
            "freecad:file:keep",
            "Close",
        );
        p.symbol("close", a.x + aw as i32 - 29, a.y + 11, 10, WIN_INK);
        // The warning triangle.
        let (tx, ty) = (a.x + 22, a.y + 44);
        p.path(
            vec![(tx + 16, ty), (tx + 32, ty + 28), (tx, ty + 28)],
            Color::rgb(252, 225, 0),
        );
        p.strong(tx + 12, ty + 8, 10, "!", 14, WIN_INK);
        p.paragraph(
            a.x + 70,
            a.y + 44,
            aw - 90,
            &format!("{name} already exists.\nDo you want to replace it?"),
            12,
            WIN_INK,
        );
        let bar = Rect::new(a.x + 1, a.y + ah as i32 - 52, aw - 2, 51);
        p.box_(bar, Color::rgb(243, 243, 243), 0);
        let no = Rect::new(a.x + aw as i32 - 110, bar.y + 11, 96, 30);
        win_button(
            p,
            Rect::new(no.x - 104, no.y, 96, 30),
            "Yes",
            "freecad:file:replace",
            false,
            Ok(()),
        );
        win_button(p, no, "No", "freecad:file:keep", true, Ok(()));
    }
}

// ---------------------------------------------------------------------------------
// GNOME: GTK 4's file chooser dialog (libadwaita styling).

const GTK_ACCENT: Color = Color::rgb(53, 132, 228);
const GTK_INK: Color = Color::rgb(36, 36, 36);
const GTK_DIM: Color = Color::rgb(119, 118, 123);
const GTK_LINE: Color = Color(0, 0, 0, 22);
const GTK_BUTTON: Color = Color(0, 0, 0, 20);
const GTK_RED: Color = Color::rgb(224, 27, 36);

fn gtk_button(
    p: &mut Painter,
    r: Rect,
    label: &str,
    target: &str,
    fill: Option<Color>,
    ok: Result<(), String>,
) {
    match fill {
        Some(c) if ok.is_ok() => {
            control(p, r, c, None, 6, label, 13, Color::WHITE, true, target, ok)
        }
        _ => control(
            p, r, GTK_BUTTON, None, 6, label, 13, GTK_INK, true, target, ok,
        ),
    }
}

fn gnome(cad: &Cad, p: &mut Painter, l: &Layout, d: &FileDialog, env: &crate::AppEnv<'_>) {
    let pointer = env.pointer;
    modal(p, l, 60);
    let saving = d.purpose.saving();
    let w = l.w.saturating_sub(40).min(880);
    let h = l.h.saturating_sub(30).min(580);
    let r = Rect::new(
        (l.w as i32 - w as i32) / 2,
        (l.h as i32 - h as i32) / 2,
        w,
        h,
    );
    p.drop_shadow(r, 12, 24, 90, 8);
    p.border(r, Color::rgb(250, 250, 250), 12, Color(0, 0, 0, 40));
    // Header bar: Cancel, the title, the suggested action.
    let hb = Rect::new(r.x, r.y, w, 46);
    p.box_(
        Rect::new(r.x + 1, r.y + 1, w - 2, 45),
        Color::rgb(235, 235, 235),
        12,
    );
    p.box_(
        Rect::new(r.x + 1, r.y + 30, w - 2, 16),
        Color::rgb(235, 235, 235),
        0,
    );
    p.hline(r.x, hb.y + 46, w, GTK_LINE);
    gtk_button(
        p,
        Rect::new(r.x + 8, r.y + 8, 76, 30),
        "Cancel",
        "freecad:file:cancel",
        None,
        Ok(()),
    );
    p.strong_center(r.x + 100, r.y + 13, w - 200, d.purpose.title(), 13, GTK_INK);
    gtk_button(
        p,
        Rect::new(r.x + w as i32 - 84, r.y + 8, 76, 30),
        d.purpose.button(),
        "freecad:file:ok",
        Some(GTK_ACCENT),
        can_ok(cad, d),
    );
    let mut y = r.y + 56;
    if saving {
        // The Name row.
        let fw = 300u32.min(w - 160);
        let fx = r.x + (w as i32 - fw as i32) / 2 + 24;
        p.right(fx - 70, y + 6, 60, "Name", 13, GTK_INK);
        let (name, focused) = name_text(cad, d);
        text_box(
            p,
            Rect::new(fx, y, fw, 34),
            &name,
            focused,
            Color::WHITE,
            Color(0, 0, 0, 40),
            GTK_ACCENT,
            6,
            13,
            "freecad:field:file-name",
            "Name",
        );
        y += 44;
    }
    // Body: places sidebar and, on the right, the path bar over the list.
    let body_h = (r.y + h as i32 - y - 56).max(80) as u32;
    let side = Rect::new(r.x + 1, y, 190, body_h);
    p.box_(side, Color::rgb(242, 242, 242), 0);
    p.vline(side.x + side.width as i32, y, body_h, GTK_LINE);
    let mut sy = y + 6;
    for item in places(DesktopTheme::Ubuntu, env, d) {
        if sy + 34 > y + body_h as i32 {
            break;
        }
        match item {
            Side::Heading(_) => {}
            Side::Gap => {
                p.hline(side.x + 12, sy + 4, side.width - 24, GTK_LINE);
                sy += 9;
            }
            Side::Row(row) => {
                let rr = Rect::new(side.x + 6, sy, side.width - 12, 34);
                if row.current {
                    p.box_(rr, Color(0, 0, 0, 24), 6);
                } else if over(pointer, rr) {
                    p.box_(rr, Color(0, 0, 0, 10), 6);
                }
                p.symbol(row.symbol, rr.x + 10, rr.y + 9, 16, GTK_INK);
                p.left(rr.x + 36, rr.y + 7, rr.width - 40, &row.label, 13, GTK_INK);
                p.region(rr, &row.target, &row.label);
                sy += 34;
            }
        }
    }
    let content = Rect::new(
        side.x + side.width as i32 + 1,
        y,
        w - side.width - 3,
        body_h,
    );
    // Path bar: Home (or the file system root) and each folder below it, plus Create
    // Folder at the right.
    let py = y + 8;
    let home = cad.home.trim_end_matches('/');
    let under_home = !home.is_empty()
        && home != "/"
        && (d.folder == home || d.folder.starts_with(&format!("{home}/")));
    let mut segs: Vec<(String, String, Option<&str>)> = vec![];
    if under_home {
        segs.push(("Home".into(), home.to_owned(), Some("home")));
        let mut prefix = home.to_owned();
        for part in d.folder[home.len()..].split('/').filter(|s| !s.is_empty()) {
            prefix = format!("{prefix}/{part}");
            segs.push((part.to_owned(), prefix.clone(), None));
        }
    } else {
        for (i, (label, path)) in crumbs(&d.folder, "/").into_iter().enumerate() {
            segs.push((
                if i == 0 { String::new() } else { label },
                path,
                (i == 0).then_some("drive"),
            ));
        }
    }
    let create = Rect::new(content.x + content.width as i32 - 44, py, 34, 30);
    let create_ok = if d.home_view || d.loading {
        Err("Open a folder first".to_owned())
    } else {
        Ok(())
    };
    let mut x = content.x + 10;
    let limit = if saving {
        create.x - 8
    } else {
        content.x + content.width as i32 - 8
    };
    let widths: Vec<i32> = segs
        .iter()
        .map(|(s, _, icon)| {
            p.measure(s, 13, false) as i32 + if icon.is_some() { 26 } else { 0 } + 22
        })
        .collect();
    let mut first = 0;
    while first + 1 < segs.len() && widths[first..].iter().sum::<i32>() > limit - x {
        first += 1;
    }
    let last = segs.len().saturating_sub(1);
    for (i, (label, path, icon)) in segs.iter().enumerate().skip(first) {
        let bw = widths[i] as u32;
        let seg = Rect::new(x, py, bw, 30);
        let fill = if i == last {
            Color(0, 0, 0, 30)
        } else if over(pointer, seg) {
            Color(0, 0, 0, 14)
        } else {
            GTK_BUTTON
        };
        p.box_(seg, fill, 6);
        let mut tx = seg.x + 11;
        if let Some(icon) = icon {
            p.symbol(icon, tx, py + 7, 16, GTK_INK);
            tx += 22;
        }
        p.label(
            tx,
            py + 5,
            bw - (tx - seg.x) as u32,
            label,
            13,
            GTK_INK,
            i == last,
            Align::Left,
        );
        let name = if label.is_empty() {
            "File System"
        } else {
            label.as_str()
        };
        p.region(seg, &format!("freecad:file:crumb:{path}"), name);
        x += bw as i32 + 4;
    }
    if saving {
        p.box_(
            create,
            if create_ok.is_ok() {
                GTK_BUTTON
            } else {
                Color(0, 0, 0, 8)
            },
            6,
        );
        icon_button(
            p,
            create,
            "plus",
            14,
            GTK_INK,
            Color(0, 0, 0, 14),
            pointer,
            "freecad:file:folder-prompt:",
            "Create Folder",
            create_ok,
        );
    }
    // The list: Name (the listing carries no sizes or dates).
    let list = Rect::new(content.x, py + 40, content.width, body_h - 48);
    p.hline(list.x, list.y - 1, list.width, GTK_LINE);
    p.left(
        list.x + 44,
        list.y + 5,
        list.width - 60,
        "Name",
        12,
        GTK_DIM,
    );
    p.hline(list.x, list.y + 27, list.width, GTK_LINE);
    let rows = Rect::new(list.x, list.y + 28, list.width, list.height - 28);
    list_notice(p, rows, d, "Folder is Empty", GTK_DIM);
    listing(p, rows, d, 34, |p, rr, e, sel| {
        if sel {
            p.box_(
                Rect::new(rr.x + 4, rr.y + 1, rr.width - 8, rr.height - 2),
                Color(53, 132, 228, 60),
                6,
            );
        } else if over(pointer, rr) {
            p.box_(
                Rect::new(rr.x + 4, rr.y + 1, rr.width - 8, rr.height - 2),
                Color(0, 0, 0, 10),
                6,
            );
        }
        if e.ends_with('/') {
            p.symbol("folder", rr.x + 16, rr.y + 8, 18, Color::rgb(233, 84, 32));
        } else {
            p.symbol("document", rr.x + 16, rr.y + 8, 18, GTK_DIM);
        }
        p.left(
            rr.x + 44,
            rr.y + 7,
            rr.width - 56,
            e.trim_end_matches('/'),
            13,
            GTK_INK,
        );
    });
    // The filter drop-down sits under the list, at the right.
    let fy = y + body_h as i32 + 12;
    p.hline(r.x, fy - 12, w, GTK_LINE);
    let filter = Rect::new(r.x + w as i32 - 300, fy, 284, 32);
    let live = type_choice(d);
    p.box_(filter, GTK_BUTTON, 6);
    p.left(
        filter.x + 12,
        fy + 6,
        filter.width - 40,
        &type_label(d),
        13,
        if live.is_ok() { GTK_INK } else { GTK_DIM },
    );
    p.symbol(
        "chevron-down",
        filter.x + filter.width as i32 - 24,
        fy + 11,
        10,
        GTK_INK,
    );
    match live {
        Ok(()) => p.region(filter, "freecad:choice:open:filetype", "File type"),
        Err(why) => {
            p.box_(filter, Color::TRANSPARENT, 6);
            p.disabled(&format!("File type: {why}"));
        }
    }

    if d.prompt.is_some() {
        // The Create Folder popover hangs under its button; a click outside closes it.
        p.z += 20;
        p.region(r, "freecad:file:folder-cancel", "Close the popover");
        p.z += 1;
        let pw = 300u32;
        let pop = Rect::new(
            create.x + create.width as i32 - pw as i32 + 6,
            create.y + 38,
            pw,
            132,
        );
        p.drop_shadow(pop, 12, 14, 70, 4);
        p.border(pop, Color::rgb(250, 250, 250), 12, Color(0, 0, 0, 40));
        p.path(
            vec![
                (create.x + 9, pop.y + 1),
                (create.x + 17, pop.y - 7),
                (create.x + 25, pop.y + 1),
            ],
            Color::rgb(250, 250, 250),
        );
        p.region(pop, "freecad:dialog:block", "Create Folder");
        p.strong(pop.x + 16, pop.y + 12, pw - 32, "Folder Name", 13, GTK_INK);
        let (text, focused) = folder_text(cad);
        text_box(
            p,
            Rect::new(pop.x + 16, pop.y + 40, pw - 116, 34),
            &text,
            focused,
            Color::WHITE,
            Color(0, 0, 0, 40),
            GTK_ACCENT,
            6,
            13,
            "freecad:field:folder-name",
            "Folder Name",
        );
        gtk_button(
            p,
            Rect::new(pop.x + pw as i32 - 92, pop.y + 40, 76, 34),
            "Create",
            "freecad:file:folder-create",
            Some(GTK_ACCENT),
            cad.folder_name_problem().map_or(Ok(()), Err),
        );
        if let Some(e) = folder_error(cad, d, &text) {
            p.left(pop.x + 16, pop.y + 88, pw - 32, &e, 12, GTK_RED);
        }
    }
    if let Some(name) = &d.confirm {
        // GTK's replace question: Replace is the default response.
        over_dialog(p, r, 50);
        let (aw, ah) = (420u32, 200u32);
        let a = Rect::new(
            r.x + (w as i32 - aw as i32) / 2,
            r.y + (h as i32 - ah as i32) / 2,
            aw,
            ah,
        );
        p.drop_shadow(a, 14, 22, 100, 8);
        p.border(a, Color::rgb(250, 250, 250), 14, Color(0, 0, 0, 40));
        let th = p.paragraph(
            a.x + 24,
            a.y + 24,
            aw - 48,
            &format!("A file named “{name}” already exists. Do you want to replace it?"),
            14,
            GTK_INK,
        );
        p.paragraph(
            a.x + 24,
            a.y + 32 + th as i32,
            aw - 48,
            &format!(
                "The file already exists in “{}”. Replacing it will overwrite its contents.",
                folder_name(&d.folder, "/")
            ),
            12,
            GTK_DIM,
        );
        let bw = (aw - 60) / 2;
        let by = a.y + ah as i32 - 52;
        gtk_button(
            p,
            Rect::new(a.x + 24, by, bw, 34),
            "Cancel",
            "freecad:file:keep",
            None,
            Ok(()),
        );
        gtk_button(
            p,
            Rect::new(a.x + 36 + bw as i32, by, bw, 34),
            "Replace",
            "freecad:file:replace",
            Some(GTK_RED),
            Ok(()),
        );
    }
}
