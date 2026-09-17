//! Deterministic OS presentation. The shell is a native scene, never a browser DOM.
use cw_scene::{Color, Node, Primitive, Rect, Scene};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopTheme {
    Macos,
    Windows,
    Ubuntu,
    Ios,
    Android,
}
impl DesktopTheme {
    pub fn from_profile(profile: &str) -> Option<Self> {
        let p = profile.to_ascii_lowercase();
        if p.contains("macos") || p == "mac" {
            Some(Self::Macos)
        } else if p.contains("windows") {
            Some(Self::Windows)
        } else if p.contains("ubuntu") || p == "linux" {
            Some(Self::Ubuntu)
        } else if p.contains("ios") {
            Some(Self::Ios)
        } else if p.contains("android") {
            Some(Self::Android)
        } else {
            None
        }
    }
    fn mobile(self) -> bool {
        matches!(self, Self::Ios | Self::Android)
    }
}

pub fn content_rect(theme: DesktopTheme, width: u32, height: u32, maximized: bool) -> Rect {
    let w = width as i32;
    let h = height as i32;
    if theme.mobile() {
        return Rect::new(0, 124.min(h), width, height.saturating_sub(156));
    }
    let (left, top, right, bottom) = if maximized {
        (
            if theme == DesktopTheme::Ubuntu { 76 } else { 0 },
            102,
            0,
            if theme == DesktopTheme::Windows {
                56
            } else {
                78
            },
        )
    } else {
        (
            if theme == DesktopTheme::Ubuntu {
                112
            } else {
                (w / 13).max(20)
            },
            (h / 9).max(48) + 76,
            (w / 13).max(20),
            (h / 6).max(90),
        )
    };
    Rect::new(
        left.min(w),
        top.min(h),
        (w - left - right).max(1) as u32,
        (h - top - bottom).max(1) as u32,
    )
}

struct Painter {
    scene: Scene,
    next: u64,
    z: i32,
}
impl Painter {
    fn node(&mut self, r: Rect, p: Primitive, interaction: Option<(&str, &str)>) {
        let mut n = Node::new(self.next, r, p);
        self.next += 1;
        n.z = self.z;
        if let Some((action, label)) = interaction {
            n = n.interactive(action, "button", label);
        }
        self.scene.nodes.push(n);
    }
    fn box_(&mut self, r: Rect, c: Color, radius: u32) {
        self.node(
            r,
            Primitive::RoundedBox {
                fill: c,
                border: None,
                border_width: 0,
                radius,
            },
            None,
        );
    }
    fn text(&mut self, x: i32, y: i32, w: u32, text: &str, size: u16, c: Color) {
        self.node(
            Rect::new(x, y, w, u32::from(size) + 9),
            Primitive::UiText {
                text: text.into(),
                color: c,
                size,
            },
            None,
        );
    }
    fn button(&mut self, r: Rect, c: Color, radius: u32, action: &str, label: &str) {
        self.node(
            r,
            Primitive::RoundedBox {
                fill: c,
                border: None,
                border_width: 0,
                radius,
            },
            Some((action, label)),
        );
    }
    fn path(&mut self, points: Vec<(i32, i32)>, fill: Color) {
        self.node(
            Rect::new(0, 0, self.scene.width, self.scene.height),
            Primitive::Path {
                points,
                fill: Some(fill),
                stroke: None,
                stroke_width: 0,
                closed: true,
            },
            None,
        );
    }
    fn line(&mut self, points: Vec<(i32, i32)>, color: Color, thickness: u16) {
        self.node(
            Rect::new(0, 0, self.scene.width, self.scene.height),
            Primitive::Path {
                points,
                fill: None,
                stroke: Some(color),
                stroke_width: thickness,
                closed: false,
            },
            None,
        );
    }
    fn icon(&mut self, x: i32, y: i32, size: u32, kind: &str, label: bool) {
        let s = size as i32;
        let action = format!("shell:launch:{kind}");
        let c = match kind {
            "browser" => Color::rgb(41, 138, 245),
            "files" => Color::rgb(62, 163, 244),
            "terminal" => Color::rgb(40, 46, 57),
            _ => Color::rgb(247, 247, 250),
        };
        self.button(
            Rect::new(x, y, size, size),
            c,
            size / 4,
            &action,
            match kind {
                "browser" => "Open browser",
                "files" => "Open files",
                "terminal" => "Open terminal",
                _ => "Open text editor",
            },
        );
        let white = Color::WHITE;
        match kind {
            "browser" => {
                self.box_(
                    Rect::new(x + s / 6, y + s / 6, size * 2 / 3, size * 2 / 3),
                    white,
                    size / 3,
                );
                self.path(
                    vec![
                        (x + s / 2, y + s / 5),
                        (x + s * 3 / 5, y + s / 2),
                        (x + s / 2, y + s * 4 / 5),
                        (x + s * 2 / 5, y + s / 2),
                    ],
                    Color::rgb(234, 82, 88),
                );
                self.path(
                    vec![
                        (x + s / 2, y + s / 5),
                        (x + s * 3 / 5, y + s / 2),
                        (x + s * 2 / 5, y + s / 2),
                    ],
                    Color::rgb(55, 156, 230),
                );
            }
            "files" => {
                self.box_(
                    Rect::new(x + s / 6, y + s / 3, size * 2 / 3, size / 2),
                    Color::rgb(239, 248, 255),
                    3,
                );
                self.box_(
                    Rect::new(x + s / 6, y + s / 4, size / 3, size / 5),
                    Color::rgb(239, 248, 255),
                    3,
                );
            }
            "terminal" => {
                self.line(
                    vec![
                        (x + s / 4, y + s / 3),
                        (x + s * 2 / 5, y + s / 2),
                        (x + s / 4, y + s * 2 / 3),
                    ],
                    white,
                    2,
                );
                self.line(
                    vec![(x + s / 2, y + s * 2 / 3), (x + s * 3 / 4, y + s * 2 / 3)],
                    white,
                    2,
                );
            }
            _ => {
                self.box_(
                    Rect::new(x + s / 4, y + s / 6, size / 2, size * 2 / 3),
                    Color::rgb(254, 208, 78),
                    2,
                );
                for i in 0..3 {
                    self.line(
                        vec![
                            (x + s / 3, y + s / 3 + i * s / 8),
                            (x + s * 2 / 3, y + s / 3 + i * s / 8),
                        ],
                        Color::rgb(159, 113, 34),
                        2,
                    );
                }
            }
        }
        if label {
            self.text(
                x - 14,
                y + s + 8,
                size + 28,
                match kind {
                    "browser" => "Browser",
                    "files" => "Files",
                    "terminal" => "Terminal",
                    _ => "Notes",
                },
                13,
                white,
            );
        }
    }
}
fn lerp(a: Color, b: Color, t: u32, n: u32) -> Color {
    let f = |a: u8, b: u8| ((u32::from(a) * (n - t) + u32::from(b) * t) / n) as u8;
    Color(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2), 255)
}
fn wallpaper(p: &mut Painter, theme: DesktopTheme) {
    let w = p.scene.width as i32;
    let h = p.scene.height as i32;
    let (a, b) = match theme {
        DesktopTheme::Macos => (Color::rgb(49, 102, 124), Color::rgb(238, 173, 133)),
        DesktopTheme::Windows => (Color::rgb(9, 23, 68), Color::rgb(44, 129, 213)),
        DesktopTheme::Ubuntu => (Color::rgb(48, 13, 49), Color::rgb(156, 39, 72)),
        DesktopTheme::Ios => (Color::rgb(87, 64, 184), Color::rgb(242, 128, 142)),
        DesktopTheme::Android => (Color::rgb(208, 221, 202), Color::rgb(162, 189, 176)),
    };
    for i in 0..80 {
        let y = h * i / 80;
        let next = h * (i + 1) / 80;
        p.box_(
            Rect::new(0, y, w as u32, (next - y) as u32),
            lerp(a, b, i as u32, 80),
            0,
        );
    }
    match theme {
        DesktopTheme::Macos => {
            p.path(
                vec![
                    (0, h * 3 / 5),
                    (w / 6, h * 2 / 5),
                    (w * 2 / 5, h * 3 / 5),
                    (w * 3 / 5, h * 2 / 3),
                    (w, h / 2),
                    (w, h),
                    (0, h),
                ],
                Color::rgb(80, 111, 123),
            );
            p.path(
                vec![
                    (0, h * 4 / 5),
                    (w / 5, h * 3 / 5),
                    (w * 2 / 5, h * 3 / 4),
                    (w * 3 / 5, h * 7 / 10),
                    (w, h * 4 / 5),
                    (w, h),
                    (0, h),
                ],
                Color::rgb(40, 81, 96),
            );
            p.path(
                vec![(w / 2, h), (w * 2 / 3, h * 7 / 10), (w, h * 3 / 5), (w, h)],
                Color::rgb(193, 124, 84),
            );
            // Golden Gate suspension bridge: a native geometric coastal scene.
            let y = h * 3 / 5;
            for x in [w * 3 / 5, w * 4 / 5] {
                p.box_(
                    Rect::new(x, y - h / 8, 6, (h / 3) as u32),
                    Color::rgb(198, 98, 67),
                    0,
                );
            }
            p.line(
                vec![
                    (w / 2, y + h / 7),
                    (w * 3 / 5, y - h / 8),
                    (w * 7 / 10, y + h / 20),
                    (w * 4 / 5, y - h / 8),
                    (w, y + h / 7),
                ],
                Color::rgb(222, 132, 89),
                3,
            );
            p.line(
                vec![(w / 2, y + h / 7), (w, y + h / 7)],
                Color::rgb(214, 120, 82),
                5,
            );
        }
        DesktopTheme::Windows => {
            for i in (0..12).rev() {
                let d = i * 13;
                p.path(
                    vec![
                        (w / 2 - d, h / 5 + d / 3),
                        (w * 4 / 5 + d / 2, h / 3 - d / 2),
                        (w * 3 / 4 - d / 3, h * 3 / 4 + d / 4),
                        (w / 3 - d / 2, h * 4 / 5 - d / 4),
                        (w / 2 + d / 2, h / 2),
                    ],
                    lerp(
                        Color::rgb(23, 74, 167),
                        Color::rgb(84, 179, 252),
                        i as u32,
                        12,
                    ),
                );
            }
        }
        DesktopTheme::Ubuntu => {
            p.path(
                vec![(w / 3, h), (w * 3 / 5, h / 4), (w, h / 2), (w, h)],
                Color::rgb(124, 29, 62),
            );
            p.path(
                vec![
                    (w * 3 / 5, h / 4),
                    (w * 4 / 5, h / 8),
                    (w, h / 2),
                    (w * 4 / 5, h * 3 / 5),
                ],
                Color::rgb(179, 53, 64),
            );
            p.line(
                vec![(w * 3 / 5, h / 4), (w * 4 / 5, h * 3 / 5), (w, h / 2)],
                Color::rgb(224, 95, 57),
                2,
            );
        }
        DesktopTheme::Ios => {
            p.box_(
                Rect::new(-w / 3, h / 3, (w * 4 / 3) as u32, (h * 3 / 4) as u32),
                Color::rgb(246, 137, 151),
                (w / 2) as u32,
            );
            p.box_(
                Rect::new(w / 3, h / 2, w as u32, h as u32),
                Color::rgb(228, 79, 111),
                (w / 2) as u32,
            );
        }
        DesktopTheme::Android => {
            p.box_(
                Rect::new(-w / 3, h / 3, w as u32, (h * 2 / 3) as u32),
                Color::rgb(135, 163, 149),
                (w / 2) as u32,
            );
            p.box_(
                Rect::new(w / 2, h / 4, w as u32, (h * 2 / 3) as u32),
                Color::rgb(229, 225, 192),
                (w / 2) as u32,
            );
        }
    }
}

/// Compose application content with OS chrome. All labels and interaction regions remain inspectable.
#[allow(clippy::too_many_arguments)]
pub fn render_shell(
    theme: DesktopTheme,
    width: u32,
    height: u32,
    clock_us: u64,
    title: &str,
    content: Option<Scene>,
    launcher_open: bool,
    maximized: bool,
) -> Scene {
    // Compact structured clients may request thumbnails smaller than usable chrome.
    // Keep the same local application semantics without overflowing shell geometry.
    if width < 240 || height < 240 {
        let mut scene = content.unwrap_or_else(|| Scene::new(width, height));
        scene.width = width;
        scene.height = height;
        return scene;
    }
    let mut p = Painter {
        scene: Scene::new(width, height),
        next: 1_u64 << 60,
        z: -100,
    };
    let w = width as i32;
    let h = height as i32;
    wallpaper(&mut p, theme);
    p.z = 10;
    let time = format!(
        "{:02}:{:02}",
        9 + (clock_us / 3_600_000_000) % 12,
        (clock_us / 60_000_000) % 60
    );
    let active = content.is_some();
    let white = Color::WHITE;
    let ink = Color::rgb(34, 39, 48);
    if theme.mobile() {
        p.text(24, 15, 100, &time, 15, white);
        p.text(w - 87, 16, 80, "5G  100%", 12, white);
        if theme == DesktopTheme::Ios {
            p.box_(
                Rect::new(w / 2 - 45, 11, 90, 24),
                Color::rgb(22, 20, 32),
                12,
            );
        } else {
            p.box_(Rect::new(w / 2 - 5, 13, 10, 10), Color::rgb(41, 54, 47), 5);
        }
        if !active {
            if theme == DesktopTheme::Android {
                p.text(
                    29,
                    77,
                    width.saturating_sub(58),
                    "Thursday, September 17",
                    17,
                    ink,
                );
                p.text(29, 117, width.saturating_sub(58), &time, 68, ink);
                p.box_(
                    Rect::new(24, 215, width.saturating_sub(48), 58),
                    Color::rgb(233, 238, 223),
                    29,
                );
                p.text(
                    44,
                    230,
                    width.saturating_sub(88),
                    "Search your world",
                    17,
                    Color::rgb(64, 78, 66),
                );
            } else {
                p.box_(
                    Rect::new(24, 73, width.saturating_sub(48), 130),
                    Color(245, 234, 249, 210),
                    24,
                );
                p.text(43, 87, 200, "THURSDAY", 12, Color::rgb(142, 48, 79));
                p.text(43, 110, 130, "17", 48, ink);
                p.text(w / 2, 105, width / 2 - 25, "Your day", 18, ink);
                p.text(
                    w / 2,
                    137,
                    width / 2 - 25,
                    "A fresh start",
                    13,
                    Color::rgb(99, 80, 102),
                );
            }
            let start = if theme == DesktopTheme::Ios { 239 } else { 309 };
            let size = (width / 6).clamp(42, 64);
            let spacing = (w - 40) / 4;
            for (i, kind) in ["browser", "files", "editor", "terminal"]
                .iter()
                .enumerate()
            {
                p.icon(
                    20 + i as i32 * spacing + (spacing - size as i32) / 2,
                    start,
                    size,
                    kind,
                    true,
                );
            }
        }
        p.button(
            Rect::new(22, h - 94, width.saturating_sub(44), 66),
            Color(248, 247, 250, 180),
            24,
            "shell:home",
            "Home",
        );
        for (i, k) in ["browser", "files", "editor", "terminal"]
            .iter()
            .enumerate()
        {
            let size = 42.min(width / 7);
            p.icon(35 + i as i32 * (w - 60) / 4, h - 82, size, k, false);
        }
        p.button(
            Rect::new(w / 2 - 52, h - 17, 104, 5),
            Color::rgb(34, 35, 42),
            3,
            "shell:home",
            "Go home",
        );
    } else {
        match theme {
            DesktopTheme::Macos => {
                p.box_(Rect::new(0, 0, width, 28), Color(248, 244, 240, 235), 0);
                p.button(
                    Rect::new(9, 2, 28, 24),
                    Color::TRANSPARENT,
                    4,
                    "shell:launcher",
                    "Applications",
                );
                p.text(17, 4, 24, "◉", 16, ink);
                p.text(
                    47,
                    5,
                    180,
                    if active { "Computerworld" } else { "Finder" },
                    13,
                    ink,
                );
                p.text(
                    166,
                    5,
                    420,
                    "File    Edit    View    Window    Help",
                    13,
                    ink,
                );
                p.text(w - 181, 5, 175, &format!("Wi-Fi   Thu  {time}"), 12, ink);
            }
            DesktopTheme::Ubuntu => {
                p.box_(Rect::new(0, 0, width, 28), Color::rgb(33, 30, 35), 0);
                p.button(
                    Rect::new(12, 2, 86, 24),
                    Color::TRANSPARENT,
                    4,
                    "shell:launcher",
                    "Activities",
                );
                p.text(17, 5, 90, "Activities", 13, white);
                p.text(w / 2 - 68, 5, 180, &format!("Sep 17  {time}"), 13, white);
                p.text(w - 138, 5, 130, "Wi-Fi   100%", 12, white);
            }
            DesktopTheme::Windows => (),
            _ => unreachable!(),
        }
        let dock_w = 292;
        let dock_x = w / 2 - dock_w / 2;
        if theme == DesktopTheme::Ubuntu {
            p.box_(
                Rect::new(0, 28, 72, height.saturating_sub(28)),
                Color(35, 28, 38, 230),
                0,
            );
            for (i, k) in ["browser", "files", "terminal", "editor"]
                .iter()
                .enumerate()
            {
                p.icon(12, 45 + i as i32 * 64, 48, k, false);
            }
            p.button(
                Rect::new(13, h - 62, 46, 46),
                Color::rgb(77, 62, 79),
                10,
                "shell:launcher",
                "Show applications",
            );
            p.text(23, h - 56, 40, "•••", 20, white);
        } else if theme == DesktopTheme::Windows {
            p.box_(
                Rect::new(0, h - 56, width, 56),
                Color(230, 239, 251, 248),
                0,
            );
            p.button(
                Rect::new(dock_x - 26, h - 45, 36, 36),
                Color::TRANSPARENT,
                4,
                "shell:launcher",
                "Start",
            );
            for row in 0..2 {
                for col in 0..2 {
                    p.box_(
                        Rect::new(dock_x - 19 + col * 13, h - 38 + row * 13, 11, 11),
                        Color::rgb(16, 118, 218),
                        0,
                    );
                }
            }
            for (i, k) in ["browser", "files", "terminal", "editor"]
                .iter()
                .enumerate()
            {
                p.icon(dock_x + 25 + i as i32 * 53, h - 45, 36, k, false);
            }
            p.text(w - 110, h - 44, 104, &time, 13, ink);
            p.text(w - 110, h - 24, 104, "9/17/2026", 11, ink);
        } else {
            p.box_(
                Rect::new(dock_x - 10, h - 79, (dock_w + 20) as u32, 68),
                Color(244, 240, 234, 205),
                19,
            );
            for (i, k) in ["files", "browser", "terminal", "editor"]
                .iter()
                .enumerate()
            {
                p.icon(dock_x + 14 + i as i32 * 68, h - 70, 50, k, false);
            }
        }
        if !active && theme != DesktopTheme::Ubuntu {
            p.icon(w - 105, 69, 48, "files", true);
        }
    }
    if let Some(mut content) = content {
        let r = content_rect(theme, width, height, maximized);
        let y = r.y - 76;
        p.z = 20;
        if !theme.mobile() {
            for i in (1..=5).rev() {
                p.box_(
                    Rect::new(
                        r.x - i,
                        y - i,
                        r.width + (i * 2) as u32,
                        r.height + 76 + (i * 2) as u32,
                    ),
                    Color(0, 0, 0, 9),
                    13,
                );
            }
        }
        let chrome = if theme == DesktopTheme::Ubuntu {
            Color::rgb(54, 49, 57)
        } else {
            Color::rgb(242, 243, 247)
        };
        p.box_(
            Rect::new(r.x, y, r.width, r.height + 76),
            chrome,
            if theme.mobile() { 0 } else { 10 },
        );
        let textcolor = if theme == DesktopTheme::Ubuntu {
            white
        } else {
            ink
        };
        let label = if title.len() > 68 {
            &title[..title
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|i| *i <= 68)
                .last()
                .unwrap_or(0)]
        } else {
            title
        };
        p.text(
            r.x + if theme == DesktopTheme::Macos { 90 } else { 17 },
            y + 13,
            r.width.saturating_sub(160),
            label,
            13,
            textcolor,
        );
        if theme == DesktopTheme::Macos {
            for (i, (action, c)) in [
                ("shell:close", Color::rgb(255, 96, 90)),
                ("shell:minimize", Color::rgb(255, 190, 54)),
                ("shell:maximize", Color::rgb(41, 201, 70)),
            ]
            .iter()
            .enumerate()
            {
                p.button(
                    Rect::new(r.x + 14 + i as i32 * 21, y + 16, 12, 12),
                    *c,
                    6,
                    action,
                    action.trim_start_matches("shell:"),
                );
            }
        } else if !theme.mobile() {
            for (i, (action, label)) in [
                ("shell:minimize", "−"),
                ("shell:maximize", "□"),
                ("shell:close", "×"),
            ]
            .iter()
            .enumerate()
            {
                let x = r.x + r.width as i32 - 105 + i as i32 * 33;
                p.button(
                    Rect::new(x, y + 6, 30, 30),
                    if *action == "shell:close" && theme == DesktopTheme::Ubuntu {
                        Color::rgb(223, 85, 49)
                    } else {
                        Color::TRANSPARENT
                    },
                    15,
                    action,
                    label,
                );
                p.text(x + 9, y + 10, 22, label, 16, textcolor);
            }
        } else {
            p.button(
                Rect::new(w - 53, y + 6, 36, 30),
                Color::TRANSPARENT,
                10,
                "shell:home",
                "Home",
            );
            p.text(w - 42, y + 10, 30, "×", 19, ink);
        }
        let browser = title.contains("://")
            || title.eq_ignore_ascii_case("browser")
            || title.starts_with("Browser — ");
        p.box_(
            Rect::new(r.x, r.y - 34, r.width, 34),
            if theme == DesktopTheme::Ubuntu {
                Color::rgb(245, 243, 246)
            } else {
                Color::rgb(250, 251, 253)
            },
            0,
        );
        if browser {
            for (i, (action, label)) in [
                ("shell:back", "‹"),
                ("shell:forward", "›"),
                ("shell:reload", "↻"),
            ]
            .iter()
            .enumerate()
            {
                let x = r.x + 7 + i as i32 * 29;
                p.button(
                    Rect::new(x, r.y - 32, 27, 29),
                    Color::TRANSPARENT,
                    5,
                    action,
                    label,
                );
                p.text(x + 7, r.y - 29, 25, label, 20, ink);
            }
            p.button(
                Rect::new(r.x + 100, r.y - 29, r.width.saturating_sub(114), 24),
                Color::rgb(233, 236, 242),
                12,
                "shell:address",
                "Browser address",
            );
            p.text(
                r.x + 115,
                r.y - 25,
                r.width.saturating_sub(145),
                if title.contains("://") || title.starts_with("Browser — ") {
                    title.strip_prefix("Browser — ").unwrap_or(title)
                } else {
                    "Search or enter an address"
                },
                12,
                Color::rgb(68, 76, 90),
            );
        } else {
            p.text(
                r.x + 17,
                r.y - 26,
                r.width.saturating_sub(34),
                match title.to_ascii_lowercase().as_str() {
                    "terminal" => "Shell  •  Local session",
                    "files" => "Home   /   Files",
                    "editor" => "Text editor   •   Autosave off",
                    _ => "Workspace",
                },
                12,
                Color::rgb(100, 108, 121),
            );
        }
        for node in &mut content.nodes {
            node.transform.tx += r.x;
            node.transform.ty += r.y;
            node.clip = Some(
                node.clip
                    .map(|c| Rect::new(c.x + r.x, c.y + r.y, c.width, c.height))
                    .and_then(|c| c.intersection(r))
                    .unwrap_or(r),
            );
            node.z += 30;
        }
        p.node(
            r,
            Primitive::Box {
                fill: content.background,
                border: None,
                border_width: 0,
            },
            None,
        );
        p.scene.nodes.extend(content.nodes);
    }
    if launcher_open && !theme.mobile() {
        p.z = 200;
        let panel_w = 460.min(width.saturating_sub(24));
        let x = (w - panel_w as i32) / 2;
        let y = (h - 380).max(36);
        let bg = if theme == DesktopTheme::Ubuntu {
            Color::rgb(45, 37, 51)
        } else {
            Color::rgb(246, 247, 252)
        };
        let fg = if theme == DesktopTheme::Ubuntu {
            white
        } else {
            ink
        };
        p.box_(
            Rect::new(x - 4, y - 4, panel_w + 8, 318),
            Color(0, 0, 0, 35),
            20,
        );
        p.box_(Rect::new(x, y, panel_w, 310), bg, 18);
        p.text(x + 25, y + 25, panel_w - 50, "Applications", 23, fg);
        p.text(
            x + 25,
            y + 63,
            panel_w - 50,
            "Your tools, ready to work",
            13,
            fg,
        );
        for (i, k) in ["browser", "files", "terminal", "editor"]
            .iter()
            .enumerate()
        {
            let ix = x + 29 + i as i32 * (panel_w as i32 - 40) / 4;
            p.icon(ix, y + 114, 50, k, false);
            p.text(
                ix - 4,
                y + 177,
                90,
                match *k {
                    "browser" => "Browser",
                    "files" => "Files",
                    "terminal" => "Terminal",
                    _ => "Notes",
                },
                12,
                fg,
            );
        }
        p.box_(
            Rect::new(x, y + 242, panel_w, 68),
            if theme == DesktopTheme::Ubuntu {
                Color::rgb(38, 31, 44)
            } else {
                Color::rgb(230, 234, 243)
            },
            0,
        );
        p.text(
            x + 26,
            y + 265,
            panel_w - 52,
            "Computerworld  •  Local session",
            13,
            fg,
        );
    }
    // Preserve shell namespace independent of application node allocation.
    p.scene.revision = clock_us;
    p.scene
}

/// Native application views use actual application state, separate from desktop chrome.
pub fn app_content(state: &crate::AppState, theme: DesktopTheme, width: u32, height: u32) -> Scene {
    let mut p = Painter {
        scene: Scene::new(width, height),
        next: 1_u64 << 52,
        z: 0,
    };
    let ink = Color::rgb(40, 45, 57);
    let muted = Color::rgb(110, 116, 128);
    match state {
        crate::AppState::Terminal { input, output, .. } => {
            p.scene.background = if theme == DesktopTheme::Ubuntu {
                Color::rgb(44, 16, 39)
            } else {
                Color::rgb(24, 29, 38)
            };
            let lines: Vec<&str> = output.lines().collect();
            let capacity = (height.saturating_sub(62) / 21).max(1) as usize;
            let start = lines.len().saturating_sub(capacity);
            let mut y = 17;
            for line in &lines[start..] {
                p.node(
                    Rect::new(18, y, width.saturating_sub(36), 22),
                    Primitive::Text {
                        text: (*line).into(),
                        size: 14,
                        color: Color::rgb(215, 225, 235),
                    },
                    None,
                );
                y += 21;
            }
            p.node(
                Rect::new(18, y, width.saturating_sub(36), 24),
                Primitive::Text {
                    text: format!("$ {input}▌"),
                    size: 14,
                    color: Color::rgb(127, 217, 178),
                },
                Some(("terminal-input", "Terminal input")),
            );
        }
        crate::AppState::Files { path, entries } => {
            p.scene.background = Color::rgb(252, 252, 254);
            let sidebar = if width > 500 { 164 } else { 0 };
            if sidebar > 0 {
                p.box_(
                    Rect::new(0, 0, sidebar, height),
                    Color::rgb(239, 241, 246),
                    0,
                );
                p.text(18, 20, 140, "Locations", 11, muted);
                p.button(
                    Rect::new(8, 48, 148, 31),
                    Color::rgb(215, 224, 239),
                    6,
                    "files-root",
                    "This computer",
                );
                p.text(20, 56, 130, "This computer", 13, ink);
                p.text(18, 109, 130, "File system", 11, muted);
                p.text(18, 137, 130, path, 12, ink);
            }
            let x = sidebar as i32 + 24;
            p.button(
                Rect::new(x, 12, 36, 30),
                Color::rgb(233, 236, 242),
                6,
                "files-up",
                "Parent folder",
            );
            p.text(x + 11, 17, 22, "↑", 16, ink);
            p.text(
                x + 48,
                18,
                width.saturating_sub(sidebar + 88),
                path,
                16,
                ink,
            );
            if sidebar == 0 {
                p.button(
                    Rect::new(width.saturating_sub(65) as i32, 47, 55, 24),
                    Color::rgb(233, 236, 242),
                    5,
                    "files-root",
                    "File system root",
                );
                p.text(width.saturating_sub(58) as i32, 50, 45, "Root", 12, ink);
            }
            p.text(
                x,
                51,
                width.saturating_sub(sidebar + 40),
                &format!("{} items", entries.len()),
                12,
                muted,
            );
            p.box_(
                Rect::new(sidebar as i32, 79, width.saturating_sub(sidebar), 1),
                Color::rgb(226, 229, 236),
                0,
            );
            for (i, entry) in entries
                .iter()
                .enumerate()
                .take((height.saturating_sub(95) / 42) as usize)
            {
                let y = 89 + i as i32 * 42;
                let action = format!("open:{i}");
                p.button(
                    Rect::new(x - 8, y - 2, width.saturating_sub(sidebar + 28), 38),
                    if i % 2 == 0 {
                        Color::rgb(246, 247, 250)
                    } else {
                        Color::TRANSPARENT
                    },
                    5,
                    &action,
                    entry,
                );
                p.box_(
                    Rect::new(x, y + 7, 20, 17),
                    if entry.ends_with('/') {
                        Color::rgb(72, 161, 230)
                    } else {
                        Color::rgb(202, 208, 219)
                    },
                    3,
                );
                p.text(
                    x + 34,
                    y + 7,
                    width.saturating_sub(sidebar + 70),
                    entry,
                    14,
                    ink,
                );
            }
        }
        crate::AppState::Editor {
            path,
            text,
            dirty,
            cursor,
        } => {
            p.scene.background = Color::rgb(254, 254, 255);
            p.box_(Rect::new(0, 0, 46, height), Color::rgb(247, 248, 251), 0);
            p.node(
                Rect::new(46, 0, width.saturating_sub(46), height.saturating_sub(29)),
                Primitive::Region,
                Some(("editor-text", "Document text")),
            );
            // Restored state can contain a stale byte offset; projection must never panic.
            let mut cursor_byte = (*cursor).min(text.len());
            while !text.is_char_boundary(cursor_byte) {
                cursor_byte -= 1;
            }
            let pre = &text[..cursor_byte];
            let row = pre.bytes().filter(|b| *b == b'\n').count();
            let capacity = (height.saturating_sub(46) / 23).max(1) as usize;
            let first = row.saturating_sub(capacity.saturating_sub(1));
            for (i, line) in text.split('\n').enumerate().skip(first).take(capacity) {
                let y = 17 + (i - first) as i32 * 23;
                p.text(12, y, 30, &(i + 1).to_string(), 12, muted);
                p.node(
                    Rect::new(61, y, width.saturating_sub(78), 24),
                    Primitive::Text {
                        text: line.into(),
                        size: 14,
                        color: ink,
                    },
                    None,
                );
            }
            let col = pre.rsplit('\n').next().unwrap_or("").chars().count() as i32;
            p.box_(
                Rect::new(61 + col * 9, 17 + (row - first) as i32 * 23, 2, 18),
                Color::rgb(44, 112, 219),
                0,
            );
            p.box_(
                Rect::new(0, height.saturating_sub(29) as i32, width, 29),
                Color::rgb(239, 243, 250),
                0,
            );
            p.text(
                13,
                height.saturating_sub(22) as i32,
                width.saturating_sub(95),
                &format!(
                    "{}{}   •   UTF-8",
                    if path.is_empty() { "Untitled" } else { path },
                    if *dirty { " • Modified" } else { "" }
                ),
                11,
                muted,
            );
            if !path.is_empty() {
                p.button(
                    Rect::new(
                        width.saturating_sub(73) as i32,
                        height.saturating_sub(26) as i32,
                        65,
                        23,
                    ),
                    Color::rgb(218, 230, 248),
                    5,
                    "editor-save",
                    "Save document",
                );
                p.text(
                    width.saturating_sub(57) as i32,
                    height.saturating_sub(23) as i32,
                    46,
                    "Save",
                    12,
                    ink,
                );
            }
        }
        crate::AppState::Browser { address } => {
            p.scene.background = Color::rgb(246, 248, 252);
            p.text(
                35,
                55,
                width.saturating_sub(70),
                "Explore your world",
                27,
                ink,
            );
            p.text(
                35,
                106,
                width.saturating_sub(70),
                "Enter an address above to open a website.",
                14,
                muted,
            );
            p.text(35, 142, width.saturating_sub(70), address, 14, ink);
        }
    }
    let viewport = Rect::new(0, 0, width, height);
    for node in &mut p.scene.nodes {
        node.clip = Some(viewport);
        if let Some(semantic) = &mut node.semantic {
            if matches!(
                node.interaction.as_deref(),
                Some("editor-text" | "terminal-input")
            ) {
                semantic.role = "textbox".into();
            }
        }
    }
    p.scene
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn themes_are_distinct_and_deterministic() {
        let mut scenes = vec![];
        for t in [
            DesktopTheme::Macos,
            DesktopTheme::Windows,
            DesktopTheme::Ubuntu,
            DesktopTheme::Ios,
            DesktopTheme::Android,
        ] {
            let a = render_shell(t, 960, 640, 0, "", None, false, false);
            assert_eq!(a, render_shell(t, 960, 640, 0, "", None, false, false));
            assert!(a
                .nodes
                .iter()
                .any(|n| n.interaction.as_deref() == Some("shell:launch:browser")));
            scenes.push(a);
        }
        for pair in scenes.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
    }
    #[test]
    fn content_uses_window_coordinates() {
        let r = content_rect(DesktopTheme::Macos, 960, 640, false);
        let mut app = Scene::new(r.width, r.height);
        app.nodes.push(
            Node::rectangle(1, Rect::new(4, 5, 30, 30), Color::WHITE)
                .interactive("app:test", "button", "Test"),
        );
        let scene = render_shell(
            DesktopTheme::Macos,
            960,
            640,
            0,
            "Test",
            Some(app),
            false,
            false,
        );
        let node = scene.nodes.iter().find(|n| n.id == 1).unwrap();
        assert_eq!(node.transform, cw_scene::Transform::translate(r.x, r.y));
        assert_eq!(node.clip, Some(r));
    }
    #[test]
    fn browser_chrome_dispatches_real_navigation_and_tiny_views_are_safe() {
        let rect = content_rect(DesktopTheme::Windows, 960, 640, false);
        let scene = render_shell(
            DesktopTheme::Windows,
            960,
            640,
            0,
            "Browser — http",
            Some(Scene::new(rect.width, rect.height)),
            false,
            false,
        );
        for target in [
            "shell:back",
            "shell:forward",
            "shell:reload",
            "shell:address",
        ] {
            let node = scene
                .nodes
                .iter()
                .find(|n| n.interaction.as_deref() == Some(target))
                .unwrap();
            let r = node.bounds;
            assert_eq!(
                scene
                    .hit_test(r.x + r.width as i32 / 2, r.y + r.height as i32 / 2)
                    .unwrap()
                    .interaction
                    .as_deref(),
                Some(target)
            );
        }
        for theme in [
            DesktopTheme::Macos,
            DesktopTheme::Windows,
            DesktopTheme::Ubuntu,
            DesktopTheme::Ios,
            DesktopTheme::Android,
        ] {
            let scene = render_shell(theme, 1, 1, 0, "", None, true, false);
            assert_eq!((scene.width, scene.height), (1, 1));
        }
    }
}
