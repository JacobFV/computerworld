//! Native client-area projection. No host data and no invented file metadata.
use super::{shared::Painter, DesktopTheme};
use cw_scene::{Color, Primitive, Rect, Scene};

const INK: Color = Color::rgb(38, 39, 43);
const MUTED: Color = Color::rgb(112, 114, 120);
const LINE: Color = Color::rgb(222, 223, 227);
fn platform(t: DesktopTheme) -> &'static str {
    match t {
        DesktopTheme::Macos => "macos",
        DesktopTheme::Windows => "windows",
        DesktopTheme::Ubuntu => "ubuntu",
        DesktopTheme::Ios => "ios",
        DesktopTheme::Android => "android",
    }
}
fn separator(p: &mut Painter, y: i32, w: u32) {
    p.box_(Rect::new(0, y, w, 1), LINE, 0);
}
fn label(p: &mut Painter, x: i32, y: i32, w: u32, s: &str, size: u16) {
    p.text(x, y, w, s, size, INK);
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
fn control(p: &mut Painter, r: Rect, s: &str, action: &str, color: Color) {
    p.button(r, Color::TRANSPARENT, 5, action, s);
    p.text(r.x + 8, r.y + 7, r.width.saturating_sub(12), s, 13, color);
}
fn disabled(p: &mut Painter, r: Rect, s: &str) {
    p.node(
        r,
        Primitive::UiText {
            text: s.into(),
            size: 12,
            color: Color::rgb(151, 153, 158),
        },
        None,
    );
    if let Some(n) = p.scene.nodes.last_mut() {
        n.semantic = Some(cw_scene::Semantic {
            role: "button".into(),
            label: s.into(),
            value: None,
            disabled: true,
            focusable: false,
        });
    }
}
fn folder(p: &mut Painter, x: i32, y: i32, size: u32, t: DesktopTheme) {
    let (light, dark) = match t {
        DesktopTheme::Windows => (Color::rgb(255, 208, 86), Color::rgb(225, 171, 46)),
        DesktopTheme::Ubuntu => (Color::rgb(237, 141, 92), Color::rgb(190, 97, 65)),
        _ => (Color::rgb(92, 188, 242), Color::rgb(56, 151, 218)),
    };
    p.box_(Rect::new(x, y, size * 2 / 5, size / 3), dark, 2);
    p.box_(
        Rect::new(x, y + size as i32 / 6, size, size * 2 / 3),
        light,
        2,
    );
    p.box_(
        Rect::new(x + 1, y + size as i32 / 6, size.saturating_sub(2), 1),
        Color(255, 255, 255, 120),
        0,
    );
}
fn file_icon(p: &mut Painter, x: i32, y: i32, size: u32, t: DesktopTheme, directory: bool) {
    if directory {
        folder(p, x, y, size, t);
    } else {
        let w = size * 3 / 4;
        p.border(
            Rect::new(x + 2, y, w, size),
            Color::WHITE,
            2,
            Color::rgb(178, 184, 193),
        );
        for i in 0..3 {
            p.box_(
                Rect::new(x + 6, y + 8 + i * 4, w.saturating_sub(8), 1),
                Color::rgb(170, 181, 197),
                0,
            );
        }
    }
}
fn basename(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("File system")
}

fn files(p: &mut Painter, t: DesktopTheme, w: u32, h: u32, path: &str, entries: &[String]) {
    p.scene.background = Color::WHITE;
    let mobile = t.mobile();
    let side = if !mobile && w > 470 {
        if t == DesktopTheme::Windows {
            176
        } else {
            164
        }
    } else {
        0
    };
    let top = match t {
        DesktopTheme::Macos => 58,
        DesktopTheme::Windows => 94,
        DesktopTheme::Ubuntu => 54,
        DesktopTheme::Ios => 112,
        DesktopTheme::Android => 110,
    };
    let accent = if t == DesktopTheme::Ubuntu {
        Color::rgb(222, 80, 38)
    } else {
        Color::rgb(0, 112, 222)
    };
    if side > 0 {
        let bg = match t {
            DesktopTheme::Macos => Color::rgb(237, 236, 238),
            DesktopTheme::Windows => Color::rgb(249, 249, 249),
            _ => Color::rgb(245, 245, 245),
        };
        p.box_(Rect::new(0, 0, side, h), bg, 0);
        p.text(
            18,
            22,
            side - 30,
            if t == DesktopTheme::Macos {
                "Locations"
            } else {
                "This computer"
            },
            11,
            MUTED,
        );
        p.button(
            Rect::new(9, 48, side - 18, 34),
            if t == DesktopTheme::Macos {
                Color::rgb(213, 212, 216)
            } else {
                Color::rgb(225, 235, 245)
            },
            6,
            "files-root",
            "File system",
        );
        p.asset(
            Rect::new(18, 56, 18, 18),
            &format!("icon/{}/files", platform(t)),
        );
        label(p, 44, 58, side - 50, "File system", 13);
        p.text(18, 107, side - 28, "CURRENT FOLDER", 10, MUTED);
        folder(p, 19, 137, 17, t);
        label(p, 44, 138, side - 50, basename(path), 13);
        p.text(
            18,
            h.saturating_sub(62) as i32,
            side - 30,
            "Local storage",
            11,
            MUTED,
        );
        p.text(
            18,
            h.saturating_sub(43) as i32,
            side - 30,
            &format!("{} items", entries.len()),
            11,
            MUTED,
        );
        p.box_(Rect::new(side as i32 - 1, 0, 1, h), LINE, 0);
    }
    let x = side as i32;
    let content = w.saturating_sub(side);
    match t {
        DesktopTheme::Macos => {
            p.box_(Rect::new(x, 0, content, 58), Color::rgb(250, 249, 250), 0);
            control(p, Rect::new(x + 10, 10, 35, 32), "‹", "files-up", INK);
            label(
                p,
                x + 51,
                18,
                content.saturating_sub(210),
                basename(path),
                15,
            );
            disabled(
                p,
                Rect::new(w.saturating_sub(153) as i32, 20, 110, 20),
                "☷   List view",
            );
        }
        DesktopTheme::Windows => {
            p.box_(Rect::new(x, 0, content, 94), Color::rgb(250, 250, 250), 0);
            control(p, Rect::new(x + 8, 5, 36, 35), "↑", "files-up", INK);
            p.border(
                Rect::new(x + 51, 7, content.saturating_sub(66), 31),
                Color::WHITE,
                4,
                LINE,
            );
            label(p, x + 62, 15, content.saturating_sub(87), path, 12);
            disabled(p, Rect::new(x + 20, 58, 80, 22), "New  ∨");
            disabled(p, Rect::new(x + 103, 58, 100, 22), "Sort  ∨");
            p.text(x + 192, 58, 90, "Details", 12, INK);
        }
        DesktopTheme::Ubuntu => {
            p.box_(Rect::new(x, 0, content, 54), Color::rgb(249, 249, 249), 0);
            p.border(Rect::new(x + 12, 9, 34, 34), Color::WHITE, 7, LINE);
            control(p, Rect::new(x + 12, 9, 34, 34), "‹", "files-up", INK);
            p.border(
                Rect::new(x + 55, 9, content.saturating_sub(69), 34),
                Color::rgb(235, 235, 235),
                7,
                Color::rgb(231, 231, 231),
            );
            label(p, x + 68, 19, content.saturating_sub(95), path, 13);
        }
        DesktopTheme::Ios => {
            p.scene.background = Color::rgb(248, 248, 250);
            control(p, Rect::new(10, 5, 100, 34), "‹ Browse", "files-up", accent);
            control(
                p,
                Rect::new(w.saturating_sub(73) as i32, 5, 65, 34),
                "Root",
                "files-root",
                accent,
            );
            label(p, 22, 49, w.saturating_sub(40), basename(path), 28);
            p.text(23, 87, w.saturating_sub(46), path, 12, MUTED);
        }
        DesktopTheme::Android => {
            p.scene.background = Color::rgb(250, 248, 255);
            control(p, Rect::new(12, 8, 70, 35), "‹ Back", "files-up", INK);
            control(
                p,
                Rect::new(w.saturating_sub(94) as i32, 8, 85, 35),
                "Storage",
                "files-root",
                INK,
            );
            label(p, 25, 56, w.saturating_sub(50), basename(path), 25);
            p.text(26, 88, w.saturating_sub(50), path, 12, MUTED);
        }
    }
    if side == 0 && !mobile {
        control(
            p,
            Rect::new(w.saturating_sub(59) as i32, top - 30, 52, 25),
            "Root",
            "files-root",
            accent,
        );
    }
    p.box_(Rect::new(x, top, content, 1), LINE, 0);
    let header = if mobile { 0 } else { 28 };
    if !mobile {
        p.box_(
            Rect::new(x, top + 1, content, 27),
            Color::rgb(249, 249, 250),
            0,
        );
        p.text(
            x + 42,
            top + 8,
            content.saturating_sub(140),
            "Name",
            11,
            MUTED,
        );
        if content > 330 {
            p.text(
                w.saturating_sub(124) as i32,
                top + 8,
                110,
                "Kind",
                11,
                MUTED,
            );
        }
    }
    let row_height = if mobile {
        65
    } else if t == DesktopTheme::Macos {
        29
    } else {
        38
    };
    let rows = h.saturating_sub((top + header + 32) as u32) / row_height;
    for (i, entry) in entries.iter().take(rows as usize).enumerate() {
        let y = top + header + i as i32 * row_height as i32 + 1;
        let directory = entry.ends_with('/');
        let bg = if t == DesktopTheme::Macos && i % 2 == 0 {
            Color::rgb(245, 245, 247)
        } else {
            Color::TRANSPARENT
        };
        p.button(
            Rect::new(x + 5, y, content.saturating_sub(10), row_height),
            bg,
            4,
            &format!("open:{i}"),
            entry,
        );
        let size = if mobile { 32 } else { 19 };
        file_icon(
            p,
            x + if mobile { 22 } else { 17 },
            y + if mobile { 15 } else { 5 },
            size,
            t,
            directory,
        );
        label(
            p,
            x + if mobile { 70 } else { 45 },
            y + if mobile { 13 } else { 7 },
            content.saturating_sub(if mobile { 91 } else { 180 }),
            entry.trim_end_matches('/'),
            if mobile { 16 } else { 12 },
        );
        if mobile {
            p.text(
                x + 70,
                y + 36,
                content.saturating_sub(96),
                if directory { "Folder" } else { "File" },
                12,
                MUTED,
            );
            p.box_(
                Rect::new(x + 70, y + 64, content.saturating_sub(85), 1),
                LINE,
                0,
            );
        } else if content > 330 {
            p.text(
                w.saturating_sub(124) as i32,
                y + 7,
                110,
                if directory { "Folder" } else { "File" },
                12,
                MUTED,
            );
        }
    }
    if entries.is_empty() {
        p.text(
            x + 28,
            top + header + 54,
            content.saturating_sub(56),
            "This folder is empty",
            14,
            MUTED,
        );
    }
    p.box_(
        Rect::new(x, h.saturating_sub(27) as i32, content, 27),
        Color::rgb(247, 247, 248),
        0,
    );
    p.text(
        x + 16,
        h.saturating_sub(20) as i32,
        content.saturating_sub(28),
        &format!("{} items  ·  {}", entries.len(), path),
        11,
        MUTED,
    );
}

fn terminal(p: &mut Painter, t: DesktopTheme, w: u32, h: u32, input: &str, output: &str) {
    let (bg, fg, bar, prompt) = match t {
        DesktopTheme::Macos => (
            Color::rgb(255, 255, 255),
            Color::rgb(30, 30, 30),
            Color::rgb(237, 236, 237),
            Color::rgb(30, 30, 30),
        ),
        DesktopTheme::Ubuntu => (
            Color::rgb(48, 10, 36),
            Color::rgb(242, 236, 239),
            Color::rgb(59, 58, 58),
            Color::rgb(137, 213, 83),
        ),
        DesktopTheme::Windows => (
            Color::rgb(12, 12, 12),
            Color::rgb(230, 230, 230),
            Color::rgb(34, 34, 34),
            Color::rgb(230, 230, 230),
        ),
        _ => (
            Color::rgb(24, 27, 32),
            Color::rgb(220, 230, 228),
            Color::rgb(33, 37, 43),
            Color::rgb(111, 220, 160),
        ),
    };
    p.scene.background = bg;
    p.box_(Rect::new(0, 0, w, 32), bar, 0);
    let tab = if t == DesktopTheme::Windows {
        "PowerShell"
    } else if t == DesktopTheme::Macos {
        "Shell — Terminal"
    } else {
        "Terminal"
    };
    p.asset(
        Rect::new(13, 7, 18, 18),
        &format!("icon/{}/terminal", platform(t)),
    );
    p.text(
        40,
        9,
        220,
        tab,
        12,
        if t == DesktopTheme::Macos {
            INK
        } else {
            Color::rgb(230, 230, 230)
        },
    );
    p.box_(
        Rect::new(0, 31, w, 1),
        if t == DesktopTheme::Macos {
            LINE
        } else {
            Color::rgb(71, 65, 71)
        },
        0,
    );
    let pad = if t.mobile() { 16 } else { 12 };
    let size = 13;
    let cells = (w.saturating_sub(pad * 2) / 8).max(1) as usize;
    let mut lines = Vec::new();
    for line in output.lines() {
        let chars: Vec<_> = line.chars().collect();
        if chars.is_empty() {
            lines.push(String::new());
        }
        for chunk in chars.chunks(cells) {
            lines.push(chunk.iter().collect::<String>());
        }
    }
    let capacity = (h.saturating_sub(65) / 19).max(1) as usize;
    let first = lines.len().saturating_sub(capacity);
    let mut y = 43;
    for line in &lines[first..] {
        mono(
            p,
            Rect::new(pad as i32, y, w.saturating_sub(pad * 2), 19),
            line,
            size,
            fg,
        );
        y += 19;
    }
    p.region(
        Rect::new(0, 33, w, h.saturating_sub(33)),
        "terminal-input",
        "Terminal command input",
    );
    let sig = if t == DesktopTheme::Windows {
        "PS> "
    } else {
        "$ "
    };
    mono(
        p,
        Rect::new(pad as i32, y, w.saturating_sub(pad * 2), 19),
        &format!("{sig}{input}"),
        size,
        prompt,
    );
    let caret = (sig.chars().count() + input.chars().count()) as i32 * 8;
    p.box_(
        Rect::new(pad as i32 + caret, y, 7, 16),
        Color(fg.0, fg.1, fg.2, 180),
        0,
    );
    if first > 0 {
        p.box_(
            Rect::new(w.saturating_sub(5) as i32, 40, 3, h.saturating_sub(48)),
            Color(128, 128, 128, 55),
            2,
        );
        p.box_(
            Rect::new(
                w.saturating_sub(5) as i32,
                h.saturating_sub(60) as i32,
                3,
                48,
            ),
            Color(128, 128, 128, 120),
            2,
        );
    }
}

fn editor(
    p: &mut Painter,
    t: DesktopTheme,
    (w, h): (u32, u32),
    path: &str,
    text: &str,
    dirty: bool,
    cursor: usize,
) {
    let mobile = t.mobile();
    let paper = if t == DesktopTheme::Ios {
        Color::rgb(255, 253, 247)
    } else {
        Color::WHITE
    };
    p.scene.background = paper;
    let toolbar = if mobile { 49 } else { 38 };
    let status = if mobile { 26 } else { 28 };
    p.box_(
        Rect::new(0, 0, w, toolbar),
        if t == DesktopTheme::Ios {
            paper
        } else {
            Color::rgb(247, 247, 247)
        },
        0,
    );
    let name = if path.is_empty() {
        "Untitled"
    } else {
        basename(path)
    };
    if mobile {
        label(p, 19, 14, w.saturating_sub(103), name, 16);
    } else if t == DesktopTheme::Macos {
        p.text(16, 12, 150, "Plain text", 12, MUTED);
        p.text(160, 12, w.saturating_sub(263), name, 12, INK);
    } else if t == DesktopTheme::Windows {
        p.asset(Rect::new(14, 10, 19, 19), "icon/windows/editor");
        label(p, 43, 12, w.saturating_sub(145), name, 12);
    } else {
        label(p, 17, 12, w.saturating_sub(116), name, 13);
    }
    if !path.is_empty() {
        control(
            p,
            Rect::new(w.saturating_sub(77) as i32, 4, 68, 30),
            "Save",
            "editor-save",
            if t == DesktopTheme::Ios {
                Color::rgb(174, 125, 0)
            } else {
                Color::rgb(0, 100, 204)
            },
        );
    } else {
        disabled(
            p,
            Rect::new(w.saturating_sub(65) as i32, 13, 57, 20),
            "Save",
        );
    }
    separator(p, toolbar as i32, w);
    let gutter = if t == DesktopTheme::Ubuntu { 43 } else { 0 };
    if gutter > 0 {
        p.box_(
            Rect::new(
                0,
                toolbar as i32,
                gutter,
                h.saturating_sub(toolbar + status),
            ),
            Color::rgb(248, 248, 248),
            0,
        );
    }
    let left = if mobile { 22 } else { gutter + 16 };
    let mut end = cursor.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let row = before.bytes().filter(|b| *b == b'\n').count();
    let col = before.rsplit('\n').next().unwrap_or("").chars().count();
    let line_height = if mobile { 25 } else { 22 };
    let capacity = (h.saturating_sub(toolbar + status + 24) / line_height).max(1) as usize;
    let first = row.saturating_sub(capacity.saturating_sub(1));
    p.region(
        Rect::new(
            gutter as i32,
            toolbar as i32 + 1,
            w.saturating_sub(gutter),
            h.saturating_sub(toolbar + status + 1),
        ),
        "editor-text",
        "Document text",
    );
    for (i, line) in text.split('\n').enumerate().skip(first).take(capacity) {
        let y = toolbar as i32 + 14 + (i - first) as i32 * line_height as i32;
        if gutter > 0 {
            p.text(12, y, 27, &(i + 1).to_string(), 11, MUTED);
        }
        mono(
            p,
            Rect::new(left as i32, y, w.saturating_sub(left + 17), line_height),
            line,
            if mobile { 15 } else { 14 },
            INK,
        );
    }
    // The canonical fixed-cell text primitive is 9px at 14px, 9px at 15px.
    p.box_(
        Rect::new(
            left as i32 + col as i32 * 9,
            toolbar as i32 + 14 + (row - first) as i32 * line_height as i32,
            1,
            18,
        ),
        Color::rgb(30, 104, 206),
        0,
    );
    p.box_(
        Rect::new(0, h.saturating_sub(status) as i32, w, status),
        if t == DesktopTheme::Ios {
            paper
        } else {
            Color::rgb(245, 246, 247)
        },
        0,
    );
    separator(p, h.saturating_sub(status) as i32, w);
    p.text(
        13,
        h.saturating_sub(status - 7) as i32,
        w.saturating_sub(170),
        &format!(
            "Ln {}, Col {}{}",
            row + 1,
            col + 1,
            if dirty { "  •  Modified" } else { "" }
        ),
        11,
        MUTED,
    );
    if w > 300 {
        p.text(
            w.saturating_sub(145) as i32,
            h.saturating_sub(status - 7) as i32,
            133,
            "Plain text   UTF-8",
            11,
            MUTED,
        );
    }
}

/// Pure application projection; the compositor owns frame geometry and clipping.
pub fn app_content(state: &crate::AppState, theme: DesktopTheme, width: u32, height: u32) -> Scene {
    let mut p = Painter {
        scene: Scene::new(width, height),
        next: 1_u64 << 52,
        z: 0,
    };
    match state {
        crate::AppState::Files { path, entries } => {
            files(&mut p, theme, width, height, path, entries)
        }
        crate::AppState::Terminal { input, output, .. } => {
            terminal(&mut p, theme, width, height, input, output)
        }
        crate::AppState::Editor {
            path,
            text,
            dirty,
            cursor,
        } => editor(&mut p, theme, (width, height), path, text, *dirty, *cursor),
        crate::AppState::Browser { address } => {
            p.scene.background = Color::rgb(250, 250, 252);
            let x = width.saturating_sub(300) as i32 / 2;
            p.asset(
                Rect::new(x + 118, 50, 64, 64),
                &format!("icon/{}/browser", platform(theme)),
            );
            label(&mut p, x, 137, 300, "Explore your world", 25);
            p.text(
                x,
                181,
                300,
                "Enter an address to open a website.",
                13,
                MUTED,
            );
            p.text(x, 211, 300, address, 12, MUTED);
        }
    }
    for n in &mut p.scene.nodes {
        n.clip = Some(Rect::new(0, 0, width, height));
        if let Some(s) = &mut n.semantic {
            if matches!(
                n.interaction.as_deref(),
                Some("editor-text" | "terminal-input")
            ) {
                s.role = "textbox".into();
            }
        }
    }
    p.scene
}
