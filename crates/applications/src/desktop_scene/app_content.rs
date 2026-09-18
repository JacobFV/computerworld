//! Native client-area projection. No host data and no invented file metadata: every
//! row, path and counter comes from application state, and only real actions are
//! interactive. Window frames own titles and tabs; these scenes own the content.
use super::{
    shared::{Align, Painter},
    DesktopTheme,
};
use crate::entry_name;
use cw_scene::{Color, Primitive, Rect, Scene};

const INK: Color = Color::rgb(29, 29, 31);
const MUTED: Color = Color::rgb(112, 114, 120);
const FAINT: Color = Color::rgb(160, 162, 168);
const LINE: Color = Color(0, 0, 0, 26);

struct Look {
    accent: Color,
    folder: Color,
    sidebar: Color,
    sidebar_width: u32,
    selection: Color,
    row: u32,
    root: &'static str,
}
fn look(t: DesktopTheme) -> Look {
    match t {
        DesktopTheme::Macos => Look {
            accent: Color::rgb(0, 122, 255),
            folder: Color::rgb(58, 160, 240),
            sidebar: Color::rgb(232, 231, 234),
            sidebar_width: super::macos::FINDER_SIDEBAR,
            selection: Color(0, 0, 0, 24),
            row: 24,
            root: "Macintosh HD",
        },
        DesktopTheme::Windows => Look {
            accent: Color::rgb(0, 95, 184),
            folder: Color::rgb(245, 187, 64),
            sidebar: Color::rgb(249, 249, 249),
            sidebar_width: 184,
            selection: Color::rgb(229, 241, 251),
            row: 32,
            root: "This PC",
        },
        DesktopTheme::Ubuntu => Look {
            accent: Color::rgb(233, 84, 32),
            folder: Color::rgb(233, 84, 32),
            sidebar: Color::rgb(246, 246, 246),
            sidebar_width: super::ubuntu::FILES_SIDEBAR,
            selection: Color(0, 0, 0, 22),
            row: 40,
            root: "Computer",
        },
        DesktopTheme::Ios => Look {
            accent: Color::rgb(0, 122, 255),
            folder: Color::rgb(64, 168, 250),
            sidebar: Color::WHITE,
            sidebar_width: 0,
            selection: Color(0, 0, 0, 16),
            row: 60,
            root: "On My iPhone",
        },
        DesktopTheme::Android => Look {
            accent: Color::rgb(76, 102, 43),
            folder: Color::rgb(95, 99, 104),
            sidebar: Color::WHITE,
            sidebar_width: 0,
            selection: Color(0, 0, 0, 16),
            row: 64,
            root: "Internal storage",
        },
    }
}
fn mono(p: &mut Painter, r: Rect, s: &str, size: u16, color: Color) {
    p.node(
        r,
        Primitive::Text {
            text: s.into(),
            size,
            color,
        },
        None,
    );
}
/// A control that is real only when `enabled`; otherwise it is drawn greyed and
/// announced as disabled, never painted as an affordance that would be refused.
fn control(p: &mut Painter, r: Rect, action: &str, label: &str, enabled: bool) {
    if enabled {
        p.button(r, Color::TRANSPARENT, 4, action, label);
    } else {
        p.box_(r, Color::TRANSPARENT, 4);
        p.disabled(label);
    }
}
fn components(path: &str) -> Vec<&str> {
    path.split(['/', '\\']).filter(|s| !s.is_empty()).collect()
}
fn current<'a>(path: &'a str, root: &'a str) -> &'a str {
    components(path).last().copied().unwrap_or(root)
}
fn parent<'a>(path: &'a str, root: &'a str) -> &'a str {
    let parts = components(path);
    if parts.len() >= 2 {
        parts[parts.len() - 2]
    } else {
        root
    }
}
fn has_parent(path: &str) -> bool {
    !components(path).is_empty()
}
/// Breadcrumb trail as (label, absolute path) pairs, root first.
fn crumbs(path: &str, root: &str) -> Vec<(String, String)> {
    let mut out = vec![(root.to_owned(), "/".to_owned())];
    let mut prefix = String::new();
    for part in components(path) {
        prefix.push('/');
        prefix.push_str(part);
        out.push((part.to_owned(), prefix.clone()));
    }
    out
}
/// Finder's tab bar. Returns the height it consumed.
fn tab_strip(
    p: &mut Painter,
    l: &Look,
    x: i32,
    width: u32,
    tabs: &[crate::FileTab],
    active: usize,
) -> u32 {
    const HEIGHT: u32 = 28;
    p.box_(Rect::new(x, 0, width, HEIGHT), Color::rgb(236, 236, 238), 0);
    p.hline(x, HEIGHT as i32 - 1, width, LINE);
    let plus = 30;
    let each = (width.saturating_sub(plus) / tabs.len().max(1) as u32).clamp(60, 240);
    for (i, tab) in tabs.iter().enumerate() {
        let r = Rect::new(x + i as i32 * each as i32, 0, each, HEIGHT - 1);
        p.button(
            r,
            if i == active {
                Color::WHITE
            } else {
                Color::TRANSPARENT
            },
            0,
            &format!("files-tab:{i}"),
            tab.name(),
        );
        p.vline(r.x + r.width as i32 - 1, 4, HEIGHT - 9, LINE);
        p.label(
            r.x + 10,
            5,
            r.width.saturating_sub(34),
            tab.name(),
            12,
            if i == active { INK } else { MUTED },
            i == active,
            Align::Left,
        );
        let close = Rect::new(r.x + r.width as i32 - 24, 5, 18, 18);
        p.button(
            close,
            Color::TRANSPARENT,
            4,
            &format!("files-closetab:{i}"),
            "Close tab",
        );
        p.symbol("close", close.x + 4, close.y + 4, 10, MUTED);
    }
    let plus_rect = Rect::new(x + width as i32 - plus as i32, 3, 22, 22);
    p.button(plus_rect, Color::TRANSPARENT, 4, "files-newtab", "New tab");
    p.symbol("plus", plus_rect.x + 5, plus_rect.y + 5, 12, l.accent);
    HEIGHT
}
fn entry_icon(p: &mut Painter, l: &Look, x: i32, y: i32, size: u32, directory: bool) {
    if directory {
        p.symbol("folder", x, y, size, l.folder);
    } else {
        p.symbol("document", x, y, size, Color::rgb(126, 132, 142));
    }
}

/// One row of a file manager sidebar: its name, glyph, glyph tint and the command it
/// runs. `current` lights the row for the place the tab is showing.
struct Place {
    label: String,
    symbol: &'static str,
    tint: Option<Color>,
    action: String,
    current: bool,
    pinned: bool,
}
enum Side {
    Heading(&'static str),
    Row(Place),
    Gap,
}
/// Sidebar glyph for a standard home folder.
fn folder_symbol(name: &str) -> &'static str {
    match name {
        "Desktop" => "desktop",
        "Documents" => "document",
        "Downloads" => "download",
        "Music" => "music",
        "Pictures" => "image",
        "Videos" | "Movies" => "film",
        _ => "folder",
    }
}
/// Explorer tints its known folders the way their shell icons are coloured.
fn explorer_tint(name: &str) -> Color {
    match name {
        "Desktop" => Color::rgb(0, 120, 212),
        "Downloads" => Color::rgb(16, 137, 62),
        "Documents" => Color::rgb(96, 110, 128),
        "Pictures" => Color::rgb(0, 153, 188),
        "Music" => Color::rgb(232, 99, 43),
        "Videos" => Color::rgb(135, 100, 184),
        _ => Color::rgb(245, 187, 64),
    }
}
/// The platform's standard sidebar, holding only places that can really be opened:
/// the lists the desktop keeps, the home folder, the standard folders that exist in it
/// right now, the Trash and the computer. AirDrop, iCloud, Tags, OneDrive and Network
/// have nothing behind them in the simulator and are left out rather than faked.
fn places(t: DesktopTheme, env: &crate::AppEnv<'_>, tab: &crate::FileTab) -> Vec<Side> {
    use crate::FileScope;
    let files = &env.files;
    let folder_at = |path: &str| {
        tab.scope == FileScope::Folder
            && !path.is_empty()
            && tab.path.trim_end_matches('/') == path.trim_end_matches('/')
    };
    let row = |label: &str, symbol: &'static str, action: String, current: bool| {
        Side::Row(Place {
            label: label.to_owned(),
            symbol,
            tint: None,
            action,
            current,
            pinned: false,
        })
    };
    let standard = |names: &[&str], explorer: bool| -> Vec<Side> {
        names
            .iter()
            .filter(|name| files.has(name))
            .map(|name| {
                let path = files.folder(name);
                Side::Row(Place {
                    label: (*name).to_owned(),
                    symbol: folder_symbol(name),
                    tint: explorer.then(|| explorer_tint(name)),
                    current: folder_at(&path),
                    action: format!("files-location:{path}"),
                    pinned: explorer,
                })
            })
            .collect()
    };
    let has_home = !files.home.is_empty() && files.home != "/";
    let mut out = Vec::new();
    match t {
        DesktopTheme::Ubuntu => {
            out.push(row(
                "Recent",
                "clock",
                "files-recents".into(),
                tab.scope == FileScope::Recents,
            ));
            out.push(row(
                "Starred",
                "star",
                "files-starred".into(),
                tab.scope == FileScope::Starred,
            ));
            if has_home {
                out.push(row(
                    "Home",
                    "home",
                    "files-home".into(),
                    folder_at(files.home),
                ));
            }
            out.extend(standard(
                &[
                    "Desktop",
                    "Documents",
                    "Downloads",
                    "Music",
                    "Pictures",
                    "Videos",
                ],
                false,
            ));
            if !files.trash.is_empty() {
                out.push(row(
                    "Trash",
                    "trash",
                    "files-trash".into(),
                    folder_at(&files.trash),
                ));
            }
            out.push(Side::Gap);
            out.push(row(
                "Other Locations",
                "plus",
                "files-root".into(),
                folder_at("/"),
            ));
        }
        DesktopTheme::Macos => {
            out.push(Side::Heading("Favorites"));
            out.push(row(
                "Recents",
                "clock",
                "files-recents".into(),
                tab.scope == FileScope::Recents,
            ));
            out.extend(standard(&["Desktop", "Documents", "Downloads"], false));
            out.push(Side::Heading("Locations"));
            out.push(row(
                "Macintosh HD",
                "drive",
                "files-root".into(),
                folder_at("/"),
            ));
        }
        DesktopTheme::Windows => {
            if has_home {
                out.push(row(
                    "Home",
                    "home",
                    "files-quick-access".into(),
                    tab.scope == FileScope::QuickAccess,
                ));
                if files.has("Pictures") {
                    out.push(row(
                        "Gallery",
                        "image",
                        "files-gallery".into(),
                        tab.scope == FileScope::Gallery,
                    ));
                }
            }
            out.push(Side::Gap);
            out.extend(standard(&crate::QUICK_ACCESS, true));
            out.push(Side::Gap);
            out.push(row(
                "This PC",
                "desktop",
                "files-root".into(),
                folder_at("/"),
            ));
        }
        _ => {}
    }
    out
}
/// Draw the sidebar `places` describes. Every row is a real command; the lit row is
/// the place the tab is showing, so the sidebar and the listing cannot disagree.
fn sidebar(p: &mut Painter, env: &crate::AppEnv<'_>, l: &Look, h: u32, tab: &crate::FileTab) {
    let t = env.theme;
    let side = l.sidebar_width;
    p.box_(Rect::new(0, 0, side, h), l.sidebar, 0);
    p.vline(side as i32 - 1, 0, h, LINE);
    let (row, text, icon, radius) = match t {
        DesktopTheme::Ubuntu => (36, 13, 16, 6),
        DesktopTheme::Windows => (32, 12, 16, 4),
        _ => (26, 13, 16, 5),
    };
    let mut y = if t == DesktopTheme::Macos { 4 } else { 8 };
    for item in places(t, env, tab) {
        if y + row as i32 > h as i32 {
            break;
        }
        match item {
            Side::Heading(title) => {
                y += 8;
                p.strong(18, y, side - 28, title, 11, FAINT);
                y += 20;
            }
            Side::Gap => {
                y += 6;
                p.hline(12, y, side - 24, LINE);
                y += 7;
            }
            Side::Row(place) => {
                let plate = Rect::new(8, y, side - 16, row - 2);
                p.button(
                    plate,
                    if place.current {
                        l.selection
                    } else {
                        Color::TRANSPARENT
                    },
                    radius,
                    &place.action,
                    &place.label,
                );
                let tint = place.tint.unwrap_or(match t {
                    DesktopTheme::Ubuntu => INK,
                    _ => l.accent,
                });
                let ix = if t == DesktopTheme::Windows { 20 } else { 18 };
                p.symbol(
                    place.symbol,
                    ix,
                    y + (row as i32 - 2 - icon as i32) / 2,
                    icon,
                    tint,
                );
                let tx = ix + icon as i32 + 10;
                p.left(
                    tx,
                    y + (row as i32 - 2 - 18) / 2,
                    (side as i32 - tx - if place.pinned { 30 } else { 12 }).max(0) as u32,
                    &place.label,
                    text,
                    INK,
                );
                if place.pinned {
                    // Explorer marks pinned Quick access folders with a pin.
                    p.symbol(
                        "pin",
                        side as i32 - 32,
                        y + (row as i32 - 14) / 2,
                        12,
                        FAINT,
                    );
                }
                y += row as i32;
            }
        }
    }
}

fn files(p: &mut Painter, env: &crate::AppEnv<'_>, tabs: &[crate::FileTab], active: usize) {
    let (t, w, h) = (env.theme, env.width, env.height);
    let (clipboard, share_to) = (env.clipboard, env.share_to);
    p.scene.background = Color::WHITE;
    let Some(tab) = tabs.get(active).or_else(|| tabs.first()) else {
        return;
    };
    let (path, entries) = (tab.path.as_str(), tab.entries.as_slice());
    // Screen order, resolved once: every row, every `open:<i>` and the selection
    // highlight all read from this list, so they cannot disagree.
    let rows_shown = tab.display();
    if t.mobile() {
        return files_mobile(p, t, w, h, tab, &rows_shown);
    }
    let l = look(t);
    let side = if w > 470 { l.sidebar_width } else { 0 };
    if side > 0 {
        sidebar(p, env, &l, h, tab);
    }
    // What the platform calls this place, and whether it is one (a list, a view, the
    // Trash) rather than a folder to show a trail of.
    let title = tab.title(t, env.files.home, &env.files.trash);
    let place = tab.is_place(&env.files.trash);
    let absolute = tab.scope.absolute();
    let x = side as i32;
    let content = w.saturating_sub(side);
    let mut top = 0;
    // Finder keeps its tab bar under the toolbar; Explorer and Files put theirs in
    // the title bar, which the window frame draws.
    if t == DesktopTheme::Macos {
        top = tab_strip(p, &l, x, content, tabs, active) as i32;
    }
    if t == DesktopTheme::Windows {
        // Navigation row with breadcrumb address, then the command bar.
        p.box_(Rect::new(x, 0, content, 88), Color::rgb(249, 250, 252), 0);
        for (i, (symbol, action, label, enabled)) in [
            ("arrow-left", "files-back", "Back", tab.can_go_back()),
            (
                "arrow-right",
                "files-forward",
                "Forward",
                tab.can_go_forward(),
            ),
            // Home and Gallery are views, not folders in a tree: Up is greyed there.
            (
                "arrow-up",
                "files-up",
                "Up one level",
                has_parent(path) && tab.scope == crate::FileScope::Folder,
            ),
            ("reload", "files-reload", "Refresh", true),
        ]
        .into_iter()
        .enumerate()
        {
            let hit = Rect::new(x + 8 + i as i32 * 34, 6, 32, 32);
            if enabled {
                p.button(hit, Color::TRANSPARENT, 4, action, label);
            }
            p.symbol(
                symbol,
                hit.x + 8,
                hit.y + 8,
                16,
                if enabled { INK } else { FAINT },
            );
            if !enabled {
                p.disabled(label);
            }
        }
        let address = Rect::new(x + 150, 7, content.saturating_sub(150 + 190), 30);
        p.border(address, Color::WHITE, 4, LINE);
        p.symbol(
            if tab.scope == crate::FileScope::QuickAccess {
                "home"
            } else if tab.scope == crate::FileScope::Gallery {
                "image"
            } else {
                "desktop"
            },
            address.x + 10,
            address.y + 8,
            14,
            MUTED,
        );
        let mut cx = address.x + 32;
        let trail = if place {
            // A view is its own root: Explorer's address reads "> Home", not a path.
            vec![(title.clone(), String::new())]
        } else {
            crumbs(path, l.root)
        };
        for (part, target) in trail {
            p.symbol("chevron-right", cx, address.y + 10, 10, MUTED);
            cx += 16;
            let width = (address.x + address.width as i32 - cx - 8).max(0) as u32;
            let text = p.left(cx, address.y + 7, width, &part, 12, INK);
            if !target.is_empty() {
                p.region(
                    Rect::new(cx - 3, address.y + 4, text + 6, 24),
                    &format!("files-location:{target}"),
                    &part,
                );
            }
            cx += text as i32 + 8;
        }
        // Real query field: what is typed lands in `tab.query` and the listing below is
        // filtered by it, so the field and the rows can never disagree.
        let search = Rect::new(x + content as i32 - 182, 7, 172, 30);
        p.border(search, Color::WHITE, 4, LINE);
        p.region(search, "files-search", "Search this folder");
        p.symbol("search", search.x + 8, search.y + 8, 13, FAINT);
        let typing = tab.searching && tab.rename.is_none();
        let shown = if tab.query.is_empty() && !typing {
            format!("Search {title}")
        } else {
            tab.query.clone()
        };
        let text = p.left(
            search.x + 26,
            search.y + 7,
            118,
            &shown,
            12,
            if tab.query.is_empty() { FAINT } else { INK },
        );
        if typing {
            p.box_(
                Rect::new(search.x + 27 + text as i32, search.y + 7, 1, 16),
                INK,
                0,
            );
        }
        if !tab.query.is_empty() || typing {
            let clear = Rect::new(search.x + search.width as i32 - 24, search.y + 6, 18, 18);
            p.button(clear, Color::TRANSPARENT, 9, "files-search-clear", "Clear");
            p.symbol("close", clear.x + 4, clear.y + 4, 10, MUTED);
        }
        p.hline(x, 44, content, LINE);
        // Command bar. Everything here dispatches a real effect, and what needs a
        // selection or a clipboard is greyed when it has none rather than refusing.
        let has_selection = tab.selection().is_some();
        let folder_scope = tab.scope == crate::FileScope::Folder;
        control(
            p,
            Rect::new(x + 10, 50, 74, 28),
            "files-new-folder",
            "New folder",
            folder_scope,
        );
        p.symbol(
            "plus",
            x + 16,
            58,
            14,
            if folder_scope { INK } else { FAINT },
        );
        p.left(
            x + 36,
            56,
            44,
            "New",
            12,
            if folder_scope { INK } else { FAINT },
        );
        for (i, (symbol, label, action, enabled)) in [
            (
                "scissors",
                "Cut",
                "files-cut",
                has_selection && folder_scope,
            ),
            ("copy", "Copy", "files-copy", has_selection && folder_scope),
            (
                "paste",
                "Paste",
                "files-paste",
                clipboard.is_some_and(|c| !c.paths.is_empty()) && folder_scope,
            ),
            (
                "rename",
                "Rename",
                "files-rename",
                has_selection && folder_scope,
            ),
            (
                "trash",
                "Delete",
                "files-delete",
                has_selection && folder_scope,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let hit = Rect::new(x + 94 + i as i32 * 36, 50, 32, 28);
            control(p, hit, action, label, enabled);
            p.symbol(symbol, hit.x + 6, 57, 16, if enabled { INK } else { FAINT });
        }
        // Share hands the selected item to Messages or Mail, whichever the machine has;
        // with no selection, or nothing installed to receive it, it is greyed.
        let share = Rect::new(x + 268, 50, 28, 28);
        match share_to.filter(|_| has_selection) {
            Some(kind) => control(
                p,
                share,
                &format!("shell:share:{kind}"),
                if kind == "chat" {
                    "Share with Messages"
                } else {
                    "Share with Mail"
                },
                true,
            ),
            None => control(p, share, "shell:share", "Share", false),
        }
        p.symbol(
            "share",
            x + 274,
            57,
            16,
            if share_to.is_some() && has_selection {
                INK
            } else {
                FAINT
            },
        );
        control(
            p,
            Rect::new(x + 306, 50, 28, 28),
            "files-new-file",
            "New file",
            folder_scope,
        );
        p.symbol(
            "document",
            x + 310,
            57,
            16,
            if folder_scope { INK } else { FAINT },
        );
        if content > 520 {
            let arrow = if tab.descending {
                "\u{2193}"
            } else {
                "\u{2191}"
            };
            // Sort flips the direction of the key that is in force; the column headers
            // choose the key. Both are the same `files-sort:<key>` command.
            for (dx, symbol, label, action) in [
                (
                    350,
                    "sort",
                    format!("Sort {arrow}"),
                    format!("files-sort:{}", tab.sort.id()),
                ),
                (
                    434,
                    if tab.view == crate::FileView::Grid {
                        "list-view"
                    } else {
                        "grid-view"
                    },
                    if tab.view == crate::FileView::Grid {
                        "List".to_owned()
                    } else {
                        "Grid".to_owned()
                    },
                    "files-view".to_owned(),
                ),
            ] {
                let hit = Rect::new(x + dx - 6, 50, 78, 28);
                p.button(hit, Color::TRANSPARENT, 4, &action, &label);
                p.symbol(symbol, x + dx, 57, 16, INK);
                p.left(x + dx + 22, 56, 50, &label, 12, INK);
            }
        }
        p.hline(x, 88, content, LINE);
        top = 89;
    }
    if side == 0 {
        // Narrow windows lose the sidebar, so the header carries both real actions.
        p.box_(Rect::new(0, top, w, 30), Color::rgb(248, 248, 249), 0);
        let up = has_parent(path);
        if up {
            p.button(
                Rect::new(6, top + 2, 72, 26),
                Color::TRANSPARENT,
                5,
                "files-up",
                "Enclosing folder",
            );
        }
        p.symbol(
            "chevron-left",
            10,
            top + 8,
            13,
            if up { l.accent } else { FAINT },
        );
        p.left(
            26,
            top + 6,
            50,
            "Back",
            13,
            if up { l.accent } else { FAINT },
        );
        if !up {
            p.disabled("Enclosing folder");
        }
        p.button(
            Rect::new(w.saturating_sub(118) as i32, top + 2, 112, 26),
            Color::TRANSPARENT,
            5,
            "files-root",
            l.root,
        );
        p.right(
            w.saturating_sub(116) as i32,
            top + 6,
            106,
            l.root,
            13,
            l.accent,
        );
        top += 30;
    }
    // Column headers are the sort controls: each one selects its key, and clicking the
    // key already in force reverses it. The arrow says which, so the order on screen is
    // always accounted for by something visible.
    // Files keeps a star column at the right edge of its list; the kind column moves
    // over for it.
    let star_column = t == DesktopTheme::Ubuntu && tab.view == crate::FileView::List;
    let kind_x = x + content as i32 - if star_column { 190 } else { 150 };
    let mark = |key: crate::SortKey| {
        if tab.sort != key {
            ""
        } else if tab.descending {
            " \u{2193}"
        } else {
            " \u{2191}"
        }
    };
    let name_header = Rect::new(x + 40, top, content.saturating_sub(206), 26);
    p.button(
        name_header,
        Color::TRANSPARENT,
        0,
        "files-sort:name",
        "Sort by name",
    );
    p.left(
        x + 44,
        top + 6,
        content.saturating_sub(210),
        &format!("Name{}", mark(crate::SortKey::Name)),
        11,
        MUTED,
    );
    if content > 330 {
        let kind = if t == DesktopTheme::Windows {
            "Type"
        } else {
            "Kind"
        };
        p.vline(kind_x - 10, top + 5, 16, LINE);
        p.button(
            Rect::new(kind_x - 4, top, 134, 26),
            Color::TRANSPARENT,
            0,
            "files-sort:kind",
            "Sort by kind",
        );
        p.left(
            kind_x,
            top + 6,
            130,
            &format!("{kind}{}", mark(crate::SortKey::Kind)),
            11,
            MUTED,
        );
    }
    p.hline(x, top + 26, content, LINE);
    let footer = if t == DesktopTheme::Ubuntu { 0 } else { 26 };
    let viewport = Rect::new(
        x,
        top + 27,
        content,
        h.saturating_sub((top + 27) as u32 + footer).max(1),
    );
    // Every row is painted in a pane that scrolls; each folder keeps its own place, so
    // Back returns to where the list was.
    let pane = p.pane(&folder_pane(path, tab.scope), viewport);
    let body = pane.top();
    if tab.view == crate::FileView::Grid {
        grid(
            p,
            env,
            &l,
            Rect::new(x, body, content, viewport.height),
            tab,
            &rows_shown,
        );
    }
    let rows = if tab.view == crate::FileView::Grid {
        0
    } else {
        rows_shown.len()
    };
    for (i, index) in rows_shown.iter().copied().take(rows).enumerate() {
        let entry = &entries[index];
        let y = body + (i as u32 * l.row) as i32;
        let directory = entry.ends_with('/');
        let inset = if t == DesktopTheme::Macos { 8 } else { 4 };
        let selected = tab.selected == Some(index);
        let bg = if selected {
            l.selection
        } else if t == DesktopTheme::Macos && i % 2 == 1 {
            Color::rgb(244, 245, 245)
        } else {
            Color::TRANSPARENT
        };
        p.button(
            Rect::new(
                x + inset,
                y,
                content.saturating_sub(inset as u32 * 2),
                l.row,
            ),
            bg,
            if t == DesktopTheme::Macos { 5 } else { 4 },
            &format!("open:{i}"),
            entry,
        );
        let icon = if t == DesktopTheme::Ubuntu { 24 } else { 16 };
        if t == DesktopTheme::Macos && directory {
            p.symbol(
                "chevron-right",
                x + 12,
                y + (l.row as i32 - 9) / 2,
                9,
                MUTED,
            );
        }
        entry_icon(
            p,
            &l,
            x + if t == DesktopTheme::Macos { 24 } else { 16 },
            y + (l.row as i32 - icon as i32) / 2,
            icon,
            directory,
        );
        let name_x = x + if t == DesktopTheme::Ubuntu { 52 } else { 48 };
        if star_column {
            // A real star: `files-star:<row>` adds the row to, or takes it off, the
            // desktop's starred list, which the Starred place shows.
            let absolute_path = if absolute {
                entry_name(entry).to_owned()
            } else {
                tab.child(entry_name(entry))
            };
            let starred = env.files.starred(&absolute_path);
            let star = Rect::new(x + content as i32 - 40, y + (l.row as i32 - 28) / 2, 28, 28);
            p.button(
                star,
                Color::TRANSPARENT,
                14,
                &format!("files-star:{i}"),
                &format!(
                    "{} {}",
                    if starred { "Unstar" } else { "Star" },
                    entry_name(entry).rsplit('/').next().unwrap_or_default()
                ),
            );
            p.symbol(
                if starred { "star" } else { "star-outline" },
                star.x + 6,
                star.y + 6,
                16,
                if starred { INK } else { FAINT },
            );
        }
        // A rename replaces the row's name with the field collecting it, caret and all:
        // what is on screen is the buffer that Enter will commit.
        match &tab.rename {
            Some(rename) if selected => {
                let field = Rect::new(name_x - 4, y + 2, 200.min(content), l.row.saturating_sub(4));
                p.border(field, Color::WHITE, 3, l.accent);
                let width = p.left(
                    field.x + 4,
                    y + (l.row as i32 - 19) / 2,
                    190,
                    &rename.name,
                    13,
                    INK,
                );
                p.box_(
                    Rect::new(
                        field.x + 5 + width as i32,
                        y + (l.row as i32 - 15) / 2,
                        1,
                        15,
                    ),
                    INK,
                    0,
                );
            }
            _ if absolute => {
                // A list of paths from all over the machine: the name, then where it
                // lives, the way Files' Recent and Starred lists read.
                let full = entry_name(entry);
                let (folder, name) = full.rsplit_once('/').unwrap_or(("", full));
                let room = (kind_x - 16 - name_x).max(0) as u32;
                let used = p.left(name_x, y + (l.row as i32 - 19) / 2, room, name, 13, INK);
                let home = env.files.home.trim_end_matches('/');
                let folder = if folder.is_empty() {
                    "/".to_owned()
                } else if !home.is_empty() && t != DesktopTheme::Windows && folder.starts_with(home)
                {
                    format!("~{}", &folder[home.len()..])
                } else {
                    folder.to_owned()
                };
                p.left(
                    name_x + used as i32 + 10,
                    y + (l.row as i32 - 18) / 2,
                    room.saturating_sub(used + 10),
                    &folder,
                    12,
                    MUTED,
                );
            }
            _ => {
                p.left(
                    name_x,
                    y + (l.row as i32 - 19) / 2,
                    (kind_x - 16 - name_x).max(0) as u32,
                    entry.trim_end_matches('/'),
                    13,
                    INK,
                );
            }
        }
        if content > 330 {
            let kind = kind_label(t, entry);
            p.left(kind_x, y + (l.row as i32 - 18) / 2, 130, &kind, 12, MUTED);
        }
    }
    if rows_shown.is_empty() {
        // A filter that hides everything says so: an empty folder and a query with no
        // match look identical otherwise, and only one of them is the folder's fault.
        let empty = if !tab.query.is_empty() {
            "No items match your search."
        } else {
            match (t, tab.scope) {
                (DesktopTheme::Windows, crate::FileScope::Gallery) => {
                    "Photos you save to Pictures will appear here."
                }
                (DesktopTheme::Windows, crate::FileScope::Starred) => {
                    "Files you add to Favorites will appear here."
                }
                (DesktopTheme::Windows, _) => "This folder is empty.",
                (_, crate::FileScope::Recents) => "No Recent Files",
                (_, crate::FileScope::Starred) => "No Starred Files",
                _ if place => "Trash is Empty",
                _ => "Folder is Empty",
            }
        };
        p.center(x, body + 60, content, empty, 15, FAINT);
    }
    p.end_pane(pane, None);
    if footer > 0 {
        let fy = h.saturating_sub(footer) as i32;
        p.box_(
            Rect::new(x, fy, content, footer),
            Color::rgb(248, 248, 249),
            0,
        );
        p.hline(x, fy, content, LINE);
        if t == DesktopTheme::Macos {
            // Path bar.
            let mut cx = x + 12;
            for (i, part) in std::iter::once(l.root).chain(components(path)).enumerate() {
                if i > 0 {
                    p.symbol("chevron-right", cx, fy + 9, 8, FAINT);
                    cx += 14;
                }
                p.symbol(
                    if i == 0 { "drive" } else { "folder" },
                    cx,
                    fy + 6,
                    13,
                    if i == 0 { MUTED } else { l.folder },
                );
                cx += 18;
                cx += p.left(
                    cx,
                    fy + 5,
                    (x + content as i32 - cx - 8).max(0) as u32,
                    part,
                    11,
                    MUTED,
                ) as i32
                    + 8;
            }
        } else {
            let note = format!(
                "{} item{}{}{}",
                rows_shown.len(),
                if rows_shown.len() == 1 { "" } else { "s" },
                // The filtered count is the count on screen; the total says what it
                // was filtered out of, rather than letting the two blur.
                if rows_shown.len() == tab.listed() {
                    String::new()
                } else {
                    format!(" of {}", tab.listed())
                },
                match tab.selection() {
                    Some(entry) => format!("  ·  {} selected", entry.trim_end_matches('/')),
                    None => String::new(),
                }
            );
            p.left(x + 14, fy + 5, content.saturating_sub(28), &note, 12, MUTED);
        }
    }
}

/// What each platform's Kind (Type) column calls an entry, read off the name the way
/// the platform reads it: by extension. Nothing here is a guess about the contents.
fn kind_label(t: DesktopTheme, entry: &str) -> String {
    if entry.ends_with('/') {
        return if t == DesktopTheme::Windows {
            "File folder".into()
        } else {
            "Folder".into()
        };
    }
    let name = entry.rsplit('/').next().unwrap_or(entry);
    let ext = name
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, ext)| ext.to_ascii_lowercase());
    match (t, ext.as_deref()) {
        (DesktopTheme::Windows, Some("txt")) => "Text Document".into(),
        (DesktopTheme::Windows, Some(ext)) => format!("{} File", ext.to_ascii_uppercase()),
        (DesktopTheme::Windows, None) => "File".into(),
        (DesktopTheme::Macos, Some("txt")) => "Plain Text Document".into(),
        (_, Some("txt")) => "Plain text document".into(),
        (DesktopTheme::Macos, Some("png")) => "PNG image".into(),
        (DesktopTheme::Macos, Some("jpg" | "jpeg")) => "JPEG image".into(),
        (_, Some("png")) => "PNG image".into(),
        (_, Some("jpg" | "jpeg")) => "JPEG image".into(),
        (_, Some("md")) => "Markdown document".into(),
        (_, Some("html" | "htm")) => "HTML document".into(),
        _ => "Document".into(),
    }
}

/// The scroll pane a folder's listing is painted in: one per place, named by a stable
/// digest of the path because a pane name is a single segment.
fn folder_pane(path: &str, scope: crate::FileScope) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in format!("{scope:?}{path}").bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("files-{hash:016x}")
}

/// Icon grid. Same rows, same order, same `open:<i>` targets as the list — only the
/// arrangement differs, so switching view cannot move a file out from under a click.
/// GNOME Files marks a starred item with a star on its icon, and the selected item
/// carries the star button too, so starring does not need the list's star column.
fn grid(
    p: &mut Painter,
    env: &crate::AppEnv<'_>,
    l: &Look,
    area: Rect,
    tab: &crate::FileTab,
    rows: &[usize],
) {
    const CELL: u32 = 96;
    let columns = (area.width / CELL).max(1);
    let stars = env.theme == DesktopTheme::Ubuntu;
    for (i, index) in rows.iter().copied().enumerate() {
        let entry = &tab.entries[index];
        let cell = Rect::new(
            area.x + (i as u32 % columns * CELL) as i32,
            area.y + (i as u32 / columns * CELL) as i32,
            CELL,
            CELL,
        );
        p.button(
            Rect::new(cell.x + 4, cell.y + 4, CELL - 8, CELL - 8),
            if tab.selected == Some(index) {
                l.selection
            } else {
                Color::TRANSPARENT
            },
            6,
            &format!("open:{i}"),
            entry,
        );
        entry_icon(p, l, cell.x + 30, cell.y + 14, 36, entry.ends_with('/'));
        p.label(
            cell.x + 6,
            cell.y + 58,
            CELL - 12,
            entry.trim_end_matches('/'),
            12,
            INK,
            false,
            Align::Center,
        );
        if stars {
            let path = if tab.scope.absolute() {
                entry_name(entry).to_owned()
            } else {
                tab.child(entry_name(entry))
            };
            let starred = env.files.starred(&path);
            if starred || tab.selected == Some(index) {
                let star = Rect::new(cell.x + CELL as i32 - 30, cell.y + 8, 22, 22);
                p.region_above(
                    star,
                    &format!("files-star:{i}"),
                    &format!(
                        "{} {}",
                        if starred { "Unstar" } else { "Star" },
                        entry_name(entry)
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                            .unwrap_or_default()
                    ),
                );
                p.symbol(
                    if starred { "star" } else { "star-outline" },
                    star.x + 3,
                    star.y + 3,
                    16,
                    if starred { INK } else { MUTED },
                );
            }
        }
    }
}

fn files_mobile(
    p: &mut Painter,
    t: DesktopTheme,
    w: u32,
    h: u32,
    tab: &crate::FileTab,
    rows_shown: &[usize],
) {
    let (path, entries) = (tab.path.as_str(), tab.entries.as_slice());
    let l = look(t);
    let ios = t == DesktopTheme::Ios;
    p.scene.background = if ios {
        Color::WHITE
    } else {
        Color::rgb(248, 250, 240)
    };
    let recents = tab.scope == crate::FileScope::Recents;
    let tab_bar = if ios { 50 } else { 0 };
    let mut top;
    // The listing scrolls; on iOS the large title and the search field scroll with it,
    // and the title collapses into the navigation bar (which carries the crumb back to
    // the enclosing folder) once it has gone under it.
    let titled = ios.then(|| {
        let title = if recents {
            "Recents"
        } else {
            current(path, l.root)
        };
        let pane = p
            .pane(
                &folder_pane(path, tab.scope),
                Rect::new(0, 0, w, h.saturating_sub(tab_bar).max(1)),
            )
            .titled(title, 44);
        p.strong(16, pane.top() + 4, w.saturating_sub(32), title, 32, INK);
        pane
    });
    if let Some(pane) = &titled {
        let y0 = pane.top() - 90 + 54;
        // Real field: typing lands in `tab.query` and the rows below are what survives
        // it, so nothing on this screen is outside the filter it advertises.
        let field = Rect::new(16, y0 + 90, w.saturating_sub(32), 36);
        p.box_(field, Color(118, 118, 128, 30), 10);
        p.region(field, "files-search", "Search");
        p.symbol("search", field.x + 8, field.y + 10, 16, FAINT);
        let typing = tab.searching;
        let width = p.left(
            field.x + 30,
            field.y + 8,
            field.width.saturating_sub(70),
            if tab.query.is_empty() && !typing {
                "Search"
            } else {
                &tab.query
            },
            17,
            if tab.query.is_empty() { FAINT } else { INK },
        );
        if typing {
            p.box_(
                Rect::new(field.x + 31 + width as i32, field.y + 8, 2, 21),
                l.accent,
                0,
            );
        }
        if !tab.query.is_empty() || typing {
            let clear = Rect::new(field.x + field.width as i32 - 30, field.y + 8, 22, 22);
            p.button(clear, Color::TRANSPARENT, 11, "files-search-clear", "Clear");
            p.symbol("close", clear.x + 5, clear.y + 5, 12, MUTED);
        }
        top = y0 + 138;
    } else {
        // Breadcrumb chips: storage root, enclosing folder, current folder.
        let mut cx = 16;
        let depth = components(path).len();
        for (i, (name, action)) in [
            (l.root, Some("files-root")),
            (parent(path, l.root), Some("files-up")),
            (current(path, l.root), None),
        ]
        .iter()
        .enumerate()
        {
            if (i == 1 && depth < 2) || (i == 2 && depth < 1) {
                if i == 1 {
                    p.region(Rect::new(16, 4, 28, 32), "files-up", "Enclosing folder");
                }
                continue;
            }
            if i > 0 {
                p.symbol("chevron-right", cx, 15, 10, MUTED);
                cx += 16;
            }
            let width = p.measure(name, 14, action.is_none());
            if let Some(action) = action {
                p.button(
                    Rect::new(cx - 6, 4, width + 12, 32),
                    Color::TRANSPARENT,
                    16,
                    action,
                    name,
                );
            }
            p.label(
                cx,
                11,
                w.saturating_sub(cx as u32 + 8),
                name,
                14,
                if action.is_some() { MUTED } else { INK },
                action.is_none(),
                Align::Left,
            );
            cx += width as i32 + 10;
        }
        p.hline(0, 42, w, LINE);
        top = 0;
    }
    // Android's crumbs stay put over the list; iOS's title scrolls with it.
    let pane = match titled {
        Some(pane) => pane,
        None => {
            let pane = p.pane(
                &folder_pane(path, tab.scope),
                Rect::new(0, 43, w, h.saturating_sub(43).max(1)),
            );
            top = pane.top() + 7;
            pane
        }
    };
    for (i, index) in rows_shown.iter().copied().enumerate() {
        let entry = &entries[index];
        let y = top + (i as u32 * l.row) as i32;
        let directory = entry.ends_with('/');
        p.button(
            Rect::new(0, y, w, l.row),
            Color::TRANSPARENT,
            0,
            &format!("open:{i}"),
            entry,
        );
        if ios {
            entry_icon(p, &l, 16, y + 12, 36, directory);
        } else {
            p.circle(36, y + 32, 20, Color::rgb(232, 236, 222));
            entry_icon(p, &l, 25, y + 21, 22, directory);
        }
        let tx = if ios { 66 } else { 72 };
        // A recent is an absolute path, so the row shows the name and says where it
        // lives; inside a folder every row is already a child of the title.
        let trimmed = entry.trim_end_matches('/');
        let (name, note) = match recents {
            true => (
                trimmed.rsplit('/').next().unwrap_or(trimmed),
                parent_label(trimmed, l.root),
            ),
            false => (
                trimmed,
                if directory { "Folder" } else { "Document" }.to_owned(),
            ),
        };
        p.left(
            tx,
            y + if ios { 10 } else { 13 },
            w.saturating_sub(tx as u32 + 40),
            name,
            16,
            INK,
        );
        p.left(
            tx,
            y + if ios { 32 } else { 36 },
            w.saturating_sub(tx as u32 + 40),
            &note,
            13,
            MUTED,
        );
        if ios {
            p.symbol(
                "chevron-right",
                w.saturating_sub(26) as i32,
                y + 23,
                13,
                FAINT,
            );
            p.hline(tx, y + l.row as i32 - 1, w.saturating_sub(tx as u32), LINE);
        }
    }
    if rows_shown.is_empty() {
        let empty = if !tab.query.is_empty() {
            "No items match your search."
        } else if recents {
            "No recent documents"
        } else if ios {
            "Folder is Empty"
        } else {
            "No items"
        };
        p.center(0, top + 70, w, empty, 17, MUTED);
    }
    p.end_pane(pane, None);
    top = h.saturating_sub(tab_bar) as i32;
    if ios {
        p.box_(Rect::new(0, top, w, tab_bar), Color(249, 249, 249, 245), 0);
        p.hline(0, top, w, LINE);
        for (i, (symbol, name, action)) in [
            // Recents is a real list: every entry is a document this desktop opened.
            ("clock", "Recents", Some("files-recents")),
            // Shared stays inert. Nothing in this world can receive a shared file —
            // no recipient, no link, no service to hand it to — so there is nothing
            // for a Shared list to hold and nothing honest for the tab to dispatch.
            ("person", "Shared", None),
            ("folder", "Browse", Some("files-browse")),
        ]
        .iter()
        .enumerate()
        {
            let cx = (w as i32 / 3) * i as i32 + w as i32 / 6;
            let selected = (i == 0) == recents && i != 1;
            let tint = match (action.is_some(), selected) {
                (false, _) => FAINT,
                (true, true) => l.accent,
                (true, false) => MUTED,
            };
            if let Some(action) = action {
                p.button(
                    Rect::new(cx - 40, top + 2, 80, 46),
                    Color::TRANSPARENT,
                    8,
                    action,
                    name,
                );
            }
            p.symbol(symbol, cx - 12, top + 5, 24, tint);
            p.label(cx - 40, top + 30, 80, name, 10, tint, false, Align::Center);
            if action.is_none() {
                p.disabled(name);
            }
        }
    }
}
/// Enclosing folder of an absolute path, as a Recents subtitle shows it.
fn parent_label(path: &str, root: &str) -> String {
    match path.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => parent.to_owned(),
        _ => root.to_owned(),
    }
}

/// Wrap `text` to `cells` columns, tagging every line with the colour that says which
/// stream it came from.
/// Rows of `cells` terminal cells: wide characters (CJK, emoji) take two, combining
/// marks none, exactly as `Primitive::Text` lays them out.
fn wrap_into(lines: &mut Vec<(String, Color)>, text: &str, color: Color, cells: usize) {
    for line in text.lines() {
        for row in cw_scene::wrap_text(line, cells) {
            lines.push((row, color));
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn terminal(
    p: &mut Painter,
    t: DesktopTheme,
    w: u32,
    h: u32,
    input: &str,
    prompt_text: &str,
    transcript: &[crate::TerminalEntry],
    cursor: usize,
    scroll: usize,
) {
    let (bg, fg, prompt, err) = match t {
        DesktopTheme::Macos => (
            Color::WHITE,
            Color::rgb(0, 0, 0),
            Color::rgb(0, 0, 0),
            Color::rgb(190, 30, 30),
        ),
        DesktopTheme::Ubuntu => (
            Color::rgb(48, 10, 36),
            Color::rgb(255, 255, 255),
            Color::rgb(138, 226, 52),
            Color::rgb(239, 41, 41),
        ),
        DesktopTheme::Windows => (
            Color::rgb(12, 12, 12),
            Color::rgb(204, 204, 204),
            Color::rgb(204, 204, 204),
            Color::rgb(231, 72, 86),
        ),
        DesktopTheme::Ios => (
            Color::rgb(0, 0, 0),
            Color::rgb(235, 235, 235),
            Color::rgb(48, 209, 88),
            Color::rgb(255, 69, 58),
        ),
        DesktopTheme::Android => (
            Color::rgb(18, 20, 22),
            Color::rgb(220, 230, 228),
            Color::rgb(111, 220, 160),
            Color::rgb(242, 109, 109),
        ),
    };
    p.scene.background = bg;
    let pad: u32 = if t.mobile() { 14 } else { 8 };
    let size = 13;
    let cells = (w.saturating_sub(pad * 2) / 8).max(1) as usize;
    let mut lines: Vec<(String, Color)> = Vec::new();
    for entry in transcript {
        // Echo first: the prompt plus the command is the boundary an observer keys on.
        wrap_into(&mut lines, &entry.echo(), prompt, cells);
        if !entry.stdout.is_empty() {
            wrap_into(&mut lines, &entry.stdout, fg, cells);
        }
        if !entry.stderr.is_empty() {
            wrap_into(&mut lines, &entry.stderr, err, cells);
        }
        if entry.failed() {
            // Only failures are marked: a clean frame stays clean, and `[exit 1]` is a
            // literal to match where error prose is not.
            wrap_into(&mut lines, &entry.status(), err, cells);
        }
    }
    let capacity = (h.saturating_sub(pad * 2 + 19) / 19).max(1) as usize;
    // `scroll` lifts the window off the tail. It is clamped here rather than in state
    // because only the view knows how many wrapped lines fit at this size.
    let hidden = lines.len().saturating_sub(capacity);
    let scroll = scroll.min(hidden);
    let first = hidden - scroll;
    let mut y = pad as i32;
    for (line, color) in &lines[first..(first + capacity).min(lines.len())] {
        mono(
            p,
            Rect::new(pad as i32, y, w.saturating_sub(pad * 2), 19),
            line,
            size,
            *color,
        );
        y += 19;
    }
    p.region(
        Rect::new(0, 0, w, h),
        "terminal-input",
        "Terminal command input",
    );
    // The machine's own prompt, not a per-theme sigil: what is drawn is what the shell
    // prints, so nothing on screen is a harness invention.
    let sig = format!("{} ", crate::prompt_or_sigil(prompt_text));
    mono(
        p,
        Rect::new(pad as i32, y, w.saturating_sub(pad * 2), 19),
        &sig,
        size,
        prompt,
    );
    let offset = cw_scene::text::terminal::columns(&sig) as i32 * 8;
    mono(
        p,
        Rect::new(
            pad as i32 + offset,
            y,
            w.saturating_sub(pad * 2 + offset as u32),
            19,
        ),
        input,
        size,
        fg,
    );
    // The prompt line has its own target, laid over the body and starting at the first
    // character of the input, so `click_at` reads a column straight off `dx`. Clicking
    // anywhere else still focuses the shell and leaves the caret where it was.
    p.region(
        Rect::new(
            pad as i32 + offset,
            y,
            w.saturating_sub(pad * 2 + offset as u32).max(1),
            19,
        ),
        "terminal-line",
        "Terminal prompt line",
    );
    // Where the caret really is: `cursor` is a byte offset into the input, and a click
    // on the line above moved it there.
    let before =
        cw_scene::text::terminal::columns(input.get(..cursor.min(input.len())).unwrap_or(input))
            as i32;
    let caret = offset + before * 8;
    // Block cursor on the desktops, a bar on touch keyboards.
    p.box_(
        Rect::new(
            pad as i32 + caret,
            y + 1,
            if t.mobile() { 2 } else { 8 },
            16,
        ),
        if t == DesktopTheme::Macos {
            Color(0, 0, 0, 110)
        } else {
            Color(fg.0, fg.1, fg.2, 200)
        },
        0,
    );
    if hidden > 0 {
        // A real scrollbar: the thumb sits where the view is, and the track above and
        // below it pages the scrollback by the height of one screen.
        let track = h.saturating_sub(8);
        let thumb = (track * capacity as u32 / lines.len().max(1) as u32).clamp(24, track.max(24));
        let travel = track.saturating_sub(thumb);
        let from_top = (travel as usize * (hidden - scroll) / hidden.max(1)) as i32;
        let bar = Rect::new(w.saturating_sub(9) as i32, 4, 8, track);
        let page = capacity.max(1);
        for (r, lines_to, label, live) in [
            (
                Rect::new(bar.x, bar.y, bar.width, from_top.max(0) as u32),
                scroll + page,
                "Scroll back",
                scroll < hidden,
            ),
            (
                Rect::new(
                    bar.x,
                    bar.y + from_top + thumb as i32,
                    bar.width,
                    travel.saturating_sub(from_top.max(0) as u32),
                ),
                scroll.saturating_sub(page),
                "Scroll forward",
                scroll > 0,
            ),
        ] {
            if r.height > 0 && live {
                p.button(
                    r,
                    Color::TRANSPARENT,
                    2,
                    &format!("terminal-scroll:{lines_to}"),
                    label,
                );
            }
        }
        p.box_(
            Rect::new(bar.x + 2, bar.y + from_top, 4, thumb),
            Color(128, 128, 128, 150),
            2,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn editor(
    p: &mut Painter,
    t: DesktopTheme,
    (w, h): (u32, u32),
    path: &str,
    text: &str,
    dirty: bool,
    cursor: usize,
    wrap: bool,
) {
    let mobile = t.mobile();
    let paper = match t {
        DesktopTheme::Ios => Color::rgb(255, 255, 255),
        DesktopTheme::Android => Color::rgb(248, 250, 240),
        _ => Color::WHITE,
    };
    p.scene.background = paper;
    // Desktop editors keep their commands in menus: Notepad's menu bar, TextEdit's
    // menu bar and GNOME Text Editor's primary menu each hold Save. A plain-text
    // TextEdit window and a Text Editor window are the document and nothing else.
    let toolbar: u32 = match t {
        DesktopTheme::Windows => 36,
        DesktopTheme::Macos | DesktopTheme::Ubuntu => 0,
        _ => 44,
    };
    let status: u32 = match t {
        DesktopTheme::Windows => 26,
        DesktopTheme::Ubuntu | DesktopTheme::Macos => 0,
        _ => 46,
    };
    let accent = look(t).accent;
    let save_label = match t {
        DesktopTheme::Ios => "Done",
        _ => "Save",
    };
    // (x, y, width) of the Save control on each platform.
    let save = match t {
        DesktopTheme::Ubuntu => Rect::new(
            w.saturating_sub(74) as i32,
            h.saturating_sub(27) as i32,
            66,
            24,
        ),
        DesktopTheme::Windows => Rect::new(150, 4, 52, 28),
        DesktopTheme::Macos => Rect::new(w.saturating_sub(62) as i32, 3, 54, 24),
        _ => Rect::new(w.saturating_sub(76) as i32, 6, 64, 32),
    };
    match t {
        DesktopTheme::Windows => {
            p.box_(Rect::new(0, 0, w, toolbar), Color::rgb(249, 250, 252), 0);
            // The shell owns menus: these open the same panels the desktop menu bar does.
            for (i, (menu, panel)) in [("File", "file"), ("Edit", "edit"), ("View", "view")]
                .into_iter()
                .enumerate()
            {
                let x = 8 + i as i32 * 46;
                p.button(
                    Rect::new(x, 3, 44, toolbar - 7),
                    Color::TRANSPARENT,
                    4,
                    &format!("shell:panel:{panel}"),
                    menu,
                );
                p.left(x + 6, 9, 40, menu, 13, INK);
            }
            // Notepad's settings: word wrap and the app theme, both real machine settings.
            p.button(
                Rect::new(w.saturating_sub(40) as i32, 3, 32, toolbar - 7),
                Color::TRANSPARENT,
                4,
                "shell:panel:app-settings",
                "Settings",
            );
            p.symbol("gear", w.saturating_sub(32) as i32, 10, 16, INK);
            p.hline(0, toolbar as i32 - 1, w, LINE);
        }
        DesktopTheme::Macos | DesktopTheme::Ubuntu => {}
        _ => {
            let name = components(path).last().copied().unwrap_or("New Note");
            p.strong(16, 11, w.saturating_sub(110), name, 17, INK);
        }
    }
    // Only a phone paints Save; see above.
    if mobile && path.is_empty() {
        p.left(
            save.x + 8,
            save.y + (save.height as i32 - 18) / 2,
            save.width,
            save_label,
            13,
            FAINT,
        );
        p.disabled(save_label);
    } else if mobile {
        let filled = matches!(t, DesktopTheme::Ubuntu) && dirty;
        p.button(
            save,
            if filled { accent } else { Color::TRANSPARENT },
            6,
            "editor-save",
            save_label,
        );
        p.label(
            save.x,
            save.y + (save.height as i32 - 18) / 2,
            save.width,
            save_label,
            13,
            if filled {
                Color::WHITE
            } else if mobile {
                Color::rgb(204, 149, 0)
            } else {
                accent
            },
            mobile,
            Align::Center,
        );
    }
    // Line numbers are off by default in every desktop editor modelled here.
    let gutter = 0;
    if gutter > 0 {
        p.box_(
            Rect::new(
                0,
                toolbar as i32,
                gutter,
                h.saturating_sub(toolbar + status),
            ),
            Color::rgb(250, 250, 250),
            0,
        );
    }
    let left = if mobile { 18 } else { gutter + 12 };
    let mut end = cursor.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let row = before.bytes().filter(|b| *b == b'\n').count();
    let col = before.rsplit('\n').next().unwrap_or("").chars().count();
    // One 8x18 cell, the geometry `crate::caret_for_point` assumes: `Primitive::Text`
    // advances `text_cell(13).0` = 8 px, and rows are stepped 18 px apart. Keeping the
    // paint on that grid is what makes a click land on the character under the pointer.
    const CELL_W: i32 = 8;
    const ROW_H: u32 = 18;
    const SIZE: u16 = 13;
    // With word wrap on, rows are as many cells as the text area holds; `Ln`/`Col`
    // below stay logical, as Notepad's do, while the caret sits on its visual row.
    let text_width = w.saturating_sub(left + 12);
    let columns = if wrap {
        (text_width / CELL_W as u32).max(1) as usize
    } else {
        0
    };
    let rows = crate::editor_rows(text, columns);
    let (visual, visual_col) = crate::editor_caret_cell(text, end, columns);
    let capacity = (h.saturating_sub(toolbar + status + ROW_H) / ROW_H).max(1) as usize;
    let first = visual.saturating_sub(capacity.saturating_sub(1));
    let origin = (left as i32, toolbar as i32 + 10);
    // The hit region starts at the first line's top-left corner, so the offsets
    // `click_at` reports are already relative to the text grid.
    p.region(
        Rect::new(
            origin.0,
            origin.1,
            w.saturating_sub(left + 12),
            h.saturating_sub(toolbar + status + 10),
        ),
        // The scroll position travels with the target, so a click on a scrolled
        // document still resolves to the character actually under the pointer.
        &if columns > 0 {
            format!("editor-text:{first}:{columns}")
        } else {
            format!("editor-text:{first}")
        },
        "Document text",
    );
    // Logical line of the first painted row; a gutter numbers each line once, on the
    // row it starts on, and leaves its soft-wrapped continuations blank.
    let mut line = text[..rows.get(first).map_or(0, |r| r.0)]
        .bytes()
        .filter(|b| *b == b'\n')
        .count();
    for (i, (start, stop)) in rows.iter().enumerate().skip(first).take(capacity) {
        let y = origin.1 + ((i - first) as u32 * ROW_H) as i32;
        let starts_line = i == 0 || text.as_bytes()[start - 1] == b'\n';
        if starts_line && i > first {
            line += 1;
        }
        if gutter > 0 && starts_line {
            p.right(
                0,
                y + 1,
                gutter - 10,
                &(line + 1).to_string(),
                11,
                if line == row { INK } else { FAINT },
            );
        }
        mono(
            p,
            Rect::new(origin.0, y, text_width, ROW_H),
            &text[*start..*stop],
            SIZE,
            INK,
        );
    }
    // Caret where the model says it is, on the same grid the click arrives on.
    p.box_(
        Rect::new(
            origin.0 + visual_col as i32 * CELL_W,
            origin.1 + ((visual - first) as u32 * ROW_H) as i32,
            if mobile { 2 } else { 1 },
            ROW_H,
        ),
        if mobile { Color::rgb(204, 149, 0) } else { INK },
        0,
    );
    let sy = h.saturating_sub(status) as i32;
    let position = format!("Ln {}, Col {}", row + 1, col + 1);
    match t {
        DesktopTheme::Windows => {
            p.box_(Rect::new(0, sy, w, status), Color::rgb(249, 250, 252), 0);
            p.hline(0, sy, w, LINE);
            // Ln/Col and the character count are live; zoom, line endings and encoding
            // are fixed facts about the model (no zoom level, text kept with `\n`
            // line breaks, always UTF-8) and open no picker.
            p.left(14, sy + 5, 120, &position, 12, MUTED);
            let count = text.chars().count();
            if w > 560 {
                p.vline(146, sy + 5, 16, LINE);
                p.left(
                    158,
                    sy + 5,
                    140,
                    &format!("{count} character{}", if count == 1 { "" } else { "s" }),
                    12,
                    MUTED,
                );
            }
            if w > 420 {
                for (i, item) in ["100%", "Unix (LF)", "UTF-8"].iter().enumerate() {
                    let x = w as i32 - 300 + i as i32 * 100;
                    p.vline(x - 12, sy + 5, 16, LINE);
                    p.left(x, sy + 5, 100, item, 12, FAINT);
                    p.disabled(item);
                }
            }
        }
        DesktopTheme::Ubuntu | DesktopTheme::Macos => {}
        _ => {
            p.hline(0, sy, w, LINE);
            // Only "New note" is real — `shell:new` opens another window of this app.
            // Gallery view, the camera and markup have no model behind them.
            let live = if t == DesktopTheme::Ios {
                Color::rgb(204, 149, 0)
            } else {
                MUTED
            };
            for (i, (symbol, label, action)) in [
                ("list-view", "Gallery view", None),
                ("camera", "Insert photo", None),
                ("edit", "Markup", None),
                ("compose", "New note", Some("shell:new")),
            ]
            .into_iter()
            .enumerate()
            {
                let cx = (w as i32 / 4) * i as i32 + w as i32 / 8;
                if let Some(action) = action {
                    p.button(
                        Rect::new(cx - 24, sy + 6, 48, 34),
                        Color::TRANSPARENT,
                        8,
                        action,
                        label,
                    );
                }
                p.symbol(
                    symbol,
                    cx - 11,
                    sy + 12,
                    22,
                    if action.is_some() { live } else { FAINT },
                );
                if action.is_none() {
                    p.disabled(label);
                }
            }
        }
    }
}

/// Pure application projection; the compositor owns frame geometry and clipping.
pub fn app_content(state: &crate::AppState, theme: DesktopTheme, width: u32, height: u32) -> Scene {
    app_content_with(
        state,
        &crate::AppEnv {
            theme,
            width,
            height,
            clock_us: 0,
            settings: &crate::SystemSettings::DEFAULT,
            clipboard: None,
            share_to: None,
            editor: None,
            pointer: None,
            files: Default::default(),
        },
    )
}
/// The same projection, with the machine facts a native application is allowed to read.
pub fn app_content_with(state: &crate::AppState, env: &crate::AppEnv<'_>) -> Scene {
    app_content_scrolled(state, env, &crate::Scroll::default())
}
/// The projection of a window whose panes are scrolled to `scroll`.
pub fn app_content_scrolled(
    state: &crate::AppState,
    env: &crate::AppEnv<'_>,
    scroll: &crate::Scroll,
) -> Scene {
    let (theme, width, height) = (env.theme, env.width, env.height);
    let mut p = Painter::themed(theme, width, height, 1_u64 << 52);
    p.scroll = scroll.clone();
    match state {
        crate::AppState::Native(app) => app.render(&mut p, env),
        crate::AppState::Files { tabs, active } => files(&mut p, env, tabs, *active),
        crate::AppState::Terminal {
            input,
            prompt,
            transcript,
            cursor,
            scroll,
            ..
        } => terminal(
            &mut p, theme, width, height, input, prompt, transcript, *cursor, *scroll,
        ),
        crate::AppState::Editor {
            path,
            text,
            dirty,
            cursor,
        } => editor(
            &mut p,
            theme,
            (width, height),
            path,
            text,
            *dirty,
            *cursor,
            // A phone's editor always wraps; there is no horizontal scroll to fall back on.
            env.settings.word_wrap || theme.mobile(),
        ),
        // Browser windows are projected by `cw_browser::Browser::scene`, which owns the
        // page, its start page and its chrome; this arm only keeps the match total.
        crate::AppState::Browser { .. } => {}
    }
    let bounds = Rect::new(0, 0, width, height);
    for n in &mut p.scene.nodes {
        // An application's own clip (a scrolled list, a viewport) is kept, within the
        // window; a node clipped away entirely keeps an empty clip and paints nothing.
        n.clip = Some(match n.clip {
            Some(c) => c.intersection(bounds).unwrap_or(Rect::new(0, 0, 0, 0)),
            None => bounds,
        });
        if let Some(s) = &mut n.semantic {
            if n.interaction.as_deref().is_some_and(|i| {
                i.starts_with("editor-text") || i == "terminal-input" || i == "files-search"
            }) {
                s.role = "textbox".into();
            }
        }
    }
    p.scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{caret_for_point, AppState, FileTab};

    fn actions(scene: &Scene) -> Vec<&str> {
        scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.as_deref())
            .collect()
    }
    fn unavailable(scene: &Scene) -> Vec<&str> {
        scene
            .nodes
            .iter()
            .filter_map(|n| n.semantic.as_ref())
            .filter(|s| s.disabled)
            .map(|s| s.label.as_str())
            .collect()
    }

    /// The painted text grid and `caret_for_point` must be the same 8x18 grid, or a
    /// click places the caret somewhere the user did not point at.
    #[test]
    fn editor_click_grid_agrees_with_the_caret_model() {
        let text = "hello world\nsecond line\nthird".to_owned();
        let scene = app_content(
            &AppState::Editor {
                path: "/note.txt".into(),
                text: text.clone(),
                cursor: 0,
                dirty: false,
            },
            DesktopTheme::Macos,
            400,
            300,
        );
        let region = scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some("editor-text:0"))
            .unwrap()
            .bounds;
        let (dx, dy) = (7 * 8 + 3, 18 + 9);
        assert_eq!(
            scene
                .hit_test(region.x + dx, region.y + dy)
                .unwrap()
                .interaction
                .as_deref(),
            Some("editor-text:0")
        );
        let cursor = caret_for_point(&text, 0, dx, dy);
        assert_eq!(&text[cursor..cursor + 1], "l");
        let placed = app_content(
            &AppState::Editor {
                path: "/note.txt".into(),
                text,
                cursor,
                dirty: false,
            },
            DesktopTheme::Macos,
            400,
            300,
        );
        let caret = placed
            .nodes
            .iter()
            .find(|n| n.bounds.width == 1 && n.bounds.height == 18)
            .unwrap()
            .bounds;
        assert_eq!((caret.x, caret.y), (region.x + 7 * 8, region.y + 18));
    }

    #[test]
    fn editor_chrome_dispatches_real_shell_actions() {
        let state = AppState::Editor {
            path: "/note.txt".into(),
            text: "note".into(),
            cursor: 0,
            dirty: false,
        };
        let notepad = app_content(&state, DesktopTheme::Windows, 620, 400);
        for id in ["shell:panel:file", "shell:panel:edit", "shell:panel:view"] {
            assert!(actions(&notepad).contains(&id), "menu bar lost {id}");
        }
        assert!(actions(&notepad).contains(&"shell:panel:app-settings"));
        assert!(unavailable(&notepad).contains(&"UTF-8"));
        // Notes' compose button opens another window of the focused application.
        let notes = app_content(&state, DesktopTheme::Ios, 390, 700);
        assert!(actions(&notes).contains(&"shell:new"));
        assert!(unavailable(&notes).contains(&"Insert photo"));
        // Desktop editors keep Save in their menus (Notepad's File menu, TextEdit's
        // menu bar, Text Editor's primary menu), so the document area paints none.
        for theme in [
            DesktopTheme::Macos,
            DesktopTheme::Windows,
            DesktopTheme::Ubuntu,
        ] {
            let scene = app_content(&state, theme, 620, 400);
            assert!(!actions(&scene).contains(&"editor-save"), "{theme:?}");
        }
        // Notepad's status bar counts what is really there: `\n` endings, 4 chars.
        assert!(unavailable(&notepad).contains(&"Unix (LF)"));
    }

    /// Nothing that cannot act may look clickable. A command that needs a selection or
    /// a clipboard is greyed until it has one, and Share — which has no recipient
    /// anywhere in this world — is greyed always.
    #[test]
    fn file_manager_commands_are_live_only_when_they_can_act() {
        let mut tab = FileTab::new("/work");
        tab.entries = vec!["notes.txt".into(), "invoices/".into()];
        let idle = AppState::Files {
            tabs: vec![tab.clone()],
            active: 0,
        };
        let explorer = app_content(&idle, DesktopTheme::Windows, 900, 520);
        // Nothing is selected and the clipboard is empty, so none of these can act.
        for label in ["Cut", "Copy", "Paste", "Rename", "Delete"] {
            assert!(
                unavailable(&explorer).contains(&label),
                "{label} looks live with nothing selected"
            );
        }
        // These never need one.
        for action in [
            "files-new-folder",
            "files-new-file",
            "files-search",
            "files-sort:name",
            "files-sort:kind",
            "files-view",
        ] {
            assert!(actions(&explorer).contains(&action), "missing {action}");
        }
        tab.selected = Some(0);
        let picked = AppState::Files {
            tabs: vec![tab],
            active: 0,
        };
        let clipboard = crate::Clipboard::new(vec!["/work/notes.txt".into()], false);
        let live = app_content_with(
            &picked,
            &crate::AppEnv {
                theme: DesktopTheme::Windows,
                width: 900,
                height: 520,
                clock_us: 0,
                settings: &crate::SystemSettings::DEFAULT,
                clipboard: Some(&clipboard),
                share_to: None,
                editor: None,
                pointer: None,
                files: Default::default(),
            },
        );
        for action in [
            "files-cut",
            "files-copy",
            "files-paste",
            "files-rename",
            "files-delete",
        ] {
            assert!(actions(&live).contains(&action), "missing {action}");
        }
        // Share has no target model at all, in either shell.
        assert!(unavailable(&live).contains(&"Share"));
        let ios = app_content(&idle, DesktopTheme::Ios, 390, 700);
        assert!(actions(&ios).contains(&"files-search"));
        assert!(actions(&ios).contains(&"files-recents"));
        assert!(unavailable(&ios).contains(&"Shared"));
        for scene in [&explorer, &live, &ios] {
            for n in &scene.nodes {
                if n.semantic.as_ref().is_some_and(|s| s.disabled) {
                    assert!(n.interaction.is_none());
                }
            }
        }
    }

    /// Wide characters take two cells in the transcript's rows and under the caret,
    /// exactly as `Primitive::Text` draws them.
    #[test]
    fn terminal_rows_and_caret_count_cells_not_characters() {
        let output = "日本語のテキストが長く続くと折り返されます";
        let state = AppState::Terminal {
            input: "echo 中文".into(),
            prompt: "me@box:/$".into(),
            transcript: vec![crate::TerminalEntry::new("me@box:/$", "cat", output, "", 0)],
            history: vec![],
            cursor: "echo 中文".len(),
            scroll: 0,
        };
        let scene = app_content(&state, DesktopTheme::Ubuntu, 16 + 8 * 20, 400);
        let rows: Vec<&str> = scene
            .nodes
            .iter()
            .filter_map(|n| match &n.primitive {
                Primitive::Text { text, .. } if !text.starts_with("me@") => Some(text.as_str()),
                _ => None,
            })
            .filter(|t| t.chars().any(|c| c > '\u{3000}'))
            .collect();
        // Twenty cells hold ten ideographs.
        assert_eq!(rows[0], "日本語のテキストが長");
        assert!(rows
            .iter()
            .all(|r| cw_scene::text::terminal::columns(r) <= 20));
        // "echo 中文" is nine cells, so the caret sits nine cells past the prompt.
        assert_eq!(cw_scene::text::terminal::columns("echo 中文"), 9);
        assert_eq!(crate::caret_for_column("echo 中文", 7 * 8), "echo 中".len());
        assert_eq!(
            crate::caret_for_column("echo 中文", 9 * 8),
            "echo 中文".len()
        );
        // The editor soft-wraps and places its caret on the same cells.
        let text = "中文中文中文";
        assert_eq!(crate::editor_rows(text, 5), [(0, 6), (6, 12), (12, 18)]);
        assert_eq!(crate::editor_caret_cell(text, 9, 5), (1, 2));
        assert_eq!(crate::caret_for_point_wrapped(text, 0, 5, 2 * 8, 18), 9);
        assert_eq!(crate::editor_rows("e\u{301}xyz", 3), [(0, 5), (5, 6)]);
    }
    /// The scrollbar really scrolls, and the caret really sits where `cursor` says.
    #[test]
    fn terminal_scrollbar_pages_and_the_caret_follows_the_cursor() {
        let transcript = (0..60)
            .map(|i| crate::TerminalEntry::new("me@box:/$", "ls", &format!("line {i}"), "", 0))
            .collect::<Vec<_>>();
        let tail = AppState::Terminal {
            input: "ls".into(),
            prompt: "me@box:/$".into(),
            transcript: transcript.clone(),
            history: vec![],
            cursor: 2,
            scroll: 0,
        };
        let scene = app_content(&tail, DesktopTheme::Ubuntu, 600, 240);
        // Pinned to the tail: only "scroll back" is offered, and it names a real target.
        let back: Vec<_> = actions(&scene)
            .into_iter()
            .filter(|a| a.starts_with("terminal-scroll:"))
            .collect();
        assert_eq!(back.len(), 1);
        let lines: usize = back[0]
            .trim_start_matches("terminal-scroll:")
            .parse()
            .unwrap();
        assert!(lines > 0);
        let text = |scene: &Scene, want: &str| {
            scene
                .nodes
                .iter()
                .any(|n| matches!(&n.primitive, Primitive::Text { text, .. } if text == want))
        };
        assert!(text(&scene, "line 59"));
        let lifted = app_content(
            &AppState::Terminal {
                input: "ls".into(),
                prompt: "me@box:/$".into(),
                transcript,
                history: vec![],
                cursor: 2,
                scroll: lines,
            },
            DesktopTheme::Ubuntu,
            600,
            240,
        );
        // Scrolled back: earlier output is on screen and both directions are offered.
        assert!(!text(&lifted, "line 59"));
        assert_eq!(
            actions(&lifted)
                .into_iter()
                .filter(|a| a.starts_with("terminal-scroll:"))
                .count(),
            2
        );
        // The prompt line is its own target, and `click_at` reads a column off it.
        assert!(actions(&scene).contains(&"terminal-line"));
        let mut desktop = crate::DesktopState::default();
        let (id, _) = desktop.launch("terminal", "").unwrap();
        desktop.text("hello").unwrap();
        desktop.click_at("terminal-line", 8, 0).unwrap();
        desktop.text("X").unwrap();
        match &desktop.windows[&id].state {
            AppState::Terminal { input, cursor, .. } => {
                assert_eq!(input, "hXello");
                assert_eq!(*cursor, 2);
            }
            _ => panic!("not a terminal"),
        }
    }
}
