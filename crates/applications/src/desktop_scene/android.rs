//! Pixel / Android 12 Material You shell, rendered entirely by the Rust scene engine.
use super::shared::{Painter, ShellContext, WindowView};
use cw_scene::{Color, Rect};

const INK: Color = Color::rgb(38, 43, 35);
const PAPER: Color = Color::rgb(244, 247, 237);

fn date(clock_us: u64) -> String {
    // The world starts on Thursday 17 September; this date advances only with simulation time.
    let days = clock_us / 86_400_000_000;
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][(days % 7) as usize];
    let mut remaining = days + 259; // September 17, zero based, in non-leap 2026.
    let mut year = 2026u64;
    // A Gregorian 400-year cycle is exactly 146097 days.
    year += (remaining / 146097) * 400;
    remaining %= 146097;
    loop {
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let length = if leap { 366 } else { 365 };
        if remaining < length {
            break;
        }
        remaining -= length;
        year += 1;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let lengths = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut month = 0;
    while remaining >= lengths[month] {
        remaining -= lengths[month];
        month += 1;
    }
    format!("{weekday}, {} {}", names[month], remaining + 1)
}

fn google(p: &mut Painter, x: i32, y: i32, size: i32) {
    // Original vector construction, not a font glyph or remotely loaded logo.
    let colors = [
        Color::rgb(66, 133, 244),
        Color::rgb(52, 168, 83),
        Color::rgb(251, 188, 5),
        Color::rgb(234, 67, 53),
    ];
    for (start, end, color) in [
        (0.0, 85.0, colors[0]),
        (85.0, 165.0, colors[1]),
        (165.0, 210.0, colors[2]),
        (210.0, 315.0, colors[3]),
    ] {
        let points = (0..=20)
            .map(|i| {
                let a = (start + (end - start) * i as f64 / 20.0).to_radians();
                (
                    x + size / 2 + (a.cos() * f64::from(size) / 2.0) as i32,
                    y + size / 2 + (a.sin() * f64::from(size) / 2.0) as i32,
                )
            })
            .collect();
        p.line(points, color, (size / 5).max(2) as u16);
    }
    p.line(
        vec![
            (x + size / 2, y + size / 2),
            (x + size, y + size / 2),
            (x + size, y + size * 3 / 4),
        ],
        colors[0],
        (size / 5).max(2) as u16,
    );
}

fn app(p: &mut Painter, x: i32, y: i32, size: u32, kind: &str, label: &str, show_label: bool) {
    p.platform_icon(
        Rect::new(x, y, size, size),
        "android",
        kind,
        &format!("shell:launch:{kind}"),
        label,
    );
    if show_label {
        p.text(x - 11, y + size as i32 + 7, size + 35, label, 12, INK);
    }
}

pub fn background(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.asset(Rect::new(0, 0, ctx.width, ctx.height), "wallpaper/android");
    if ctx.active || ctx.launcher_open {
        return;
    }
    let width = ctx.width as i32;
    let height = ctx.height as i32;
    p.text(
        29,
        61,
        ctx.width.saturating_sub(58),
        &date(ctx.clock_us),
        18,
        INK,
    );
    // Android 12's characteristic tonal, scalloped clock widget.
    let diameter = (ctx.width * 61 / 100).clamp(148, 260) as i32;
    let cx = width / 2;
    let cy = (height * 34 / 100).max(181);
    let points = (0..180)
        .map(|i| {
            let a = i as f64 * std::f64::consts::TAU / 180.0;
            let radius = f64::from(diameter) / 2.0 * (0.94 + 0.06 * (a * 12.0).cos());
            (
                cx + (radius * a.cos()) as i32,
                cy + (radius * a.sin()) as i32,
            )
        })
        .collect();
    p.path(points, Color::rgb(227, 237, 195));
    let time = ctx.time();
    let pieces: Vec<_> = time.split(':').collect();
    let size = (diameter * 33 / 100) as u16;
    let tx = cx - diameter * 28 / 100;
    p.text(
        tx,
        cy - diameter * 34 / 100,
        diameter as u32,
        pieces[0],
        size,
        Color::rgb(49, 64, 33),
    );
    p.text(
        tx,
        cy + diameter * 2 / 100,
        diameter as u32,
        pieces[1],
        size,
        Color::rgb(49, 64, 33),
    );
    let icon_size = (ctx.width / 7).clamp(43, 60);
    let row_y = (height - 269).max(cy + diameter / 2 + 28);
    for (i, (kind, label)) in [
        ("mail", "Mail"),
        ("calendar", "Calendar"),
        ("files", "Files"),
        ("terminal", "Terminal"),
    ]
    .iter()
    .filter(|(kind, _)| ctx.installed(kind))
    .enumerate()
    {
        let x = width * (i as i32 * 2 + 1) / 8 - icon_size as i32 / 2;
        app(p, x, row_y, icon_size, kind, label, true);
    }
    let dock_y = height - 164;
    for (i, (kind, label)) in [
        ("chat", "Chat"),
        ("browser", "Browser"),
        ("docs", "Documents"),
        ("editor", "Editor"),
    ]
    .iter()
    .filter(|(kind, _)| ctx.installed(kind))
    .enumerate()
    {
        let x = width * (i as i32 * 2 + 1) / 8 - icon_size as i32 / 2;
        app(p, x, dock_y, icon_size, kind, label, false);
    }
    p.box_(
        Rect::new(width / 2 - 3, height - 188, 6, 6),
        Color::rgb(57, 65, 47),
        3,
    );
    p.box_(
        Rect::new(width / 2 + 11, height - 187, 4, 4),
        Color(57, 65, 47, 90),
        2,
    );
    let search = Rect::new(21, height - 83, ctx.width.saturating_sub(42), 51);
    p.box_(search, Color::rgb(246, 248, 239), 26);
    google(p, search.x + 19, search.y + 16, 19);
    // Tapping the Pixel search surface opens the installed-application drawer.
    p.region(search, "shell:launcher", "Search installed applications");
    p.line(
        vec![(width - 65, height - 67), (width - 65, height - 54)],
        Color::rgb(69, 98, 66),
        3,
    );
    p.line(
        vec![
            (width - 71, height - 57),
            (width - 71, height - 53),
            (width - 65, height - 48),
            (width - 59, height - 53),
            (width - 59, height - 57),
        ],
        Color::rgb(69, 98, 66),
        2,
    );
    p.line(
        vec![(width - 65, height - 48), (width - 65, height - 44)],
        Color::rgb(69, 98, 66),
        2,
    );
    p.region(
        Rect::new(0, height - 116, ctx.width, 25),
        "shell:launcher",
        "Swipe up to open all applications",
    );
}

fn status(p: &mut Painter, ctx: &ShellContext<'_>) {
    let w = ctx.width as i32;
    p.text(25, 11, 70, &ctx.time(), 14, INK);
    // Pixel's centred camera aperture stays in the system-owned safe area.
    p.box_(Rect::new(w / 2 - 5, 14, 10, 10), Color::rgb(15, 19, 18), 5);
    p.path(vec![(w - 84, 17), (w - 68, 17), (w - 76, 26)], INK);
    p.path(vec![(w - 62, 26), (w - 50, 14), (w - 50, 26)], INK);
    p.border(Rect::new(w - 40, 15, 10, 14), Color::TRANSPARENT, 1, INK);
    p.box_(Rect::new(w - 38, 19, 6, 8), INK, 0);
    p.box_(Rect::new(w - 37, 13, 4, 2), INK, 0);
}

fn overview(p: &mut Painter, ctx: &ShellContext<'_>) {
    let w = ctx.width as i32;
    let h = ctx.height as i32;
    p.box_(
        Rect::new(0, 0, ctx.width, ctx.height),
        Color(221, 231, 204, 235),
        0,
    );
    if ctx.windows.is_empty() {
        p.text(
            48,
            h / 2,
            ctx.width.saturating_sub(96),
            "No recent applications",
            19,
            INK,
        );
        return;
    }
    let selected = ctx
        .windows
        .iter()
        .position(|window| window.focused)
        .unwrap_or(ctx.windows.len() - 1);
    let card_width = ctx.width * 75 / 100;
    let card_height = ctx.height * 63 / 100;
    for (i, window) in ctx.windows.iter().enumerate() {
        let x =
            (w - card_width as i32) / 2 + (i as i32 - selected as i32) * (card_width as i32 + 18);
        let y = 118;
        if x + card_width as i32 <= 0 || x >= w {
            continue;
        }
        let card = Rect::new(x, y, card_width, card_height);
        p.shadow(card, 22);
        p.box_(card, PAPER, 22);
        p.asset(
            Rect::new(x + card_width as i32 / 2 - 22, y - 60, 44, 44),
            &format!("icon/android/{}", window.kind),
        );
        p.text(
            x + 18,
            y + 15,
            card_width.saturating_sub(36),
            &window.title,
            16,
            INK,
        );
        if let Some(content) = &window.content {
            let scale = (i64::from(card_width) * 1024 / i64::from(content.width.max(1))) as i32;
            let bounds = Rect::new(
                x + 8,
                y + 54,
                card_width.saturating_sub(16),
                card_height.saturating_sub(65),
            );
            let mut nodes: Vec<_> = content.nodes.iter().collect();
            nodes.sort_by_key(|n| (n.z, n.id));
            for source in nodes {
                let mut n = source.clone();
                n.id = p.next;
                p.next += 1;
                n.z = p.z;
                n.transform.a = n.transform.a * scale / 1024;
                n.transform.b = n.transform.b * scale / 1024;
                n.transform.c = n.transform.c * scale / 1024;
                n.transform.d = n.transform.d * scale / 1024;
                n.transform.tx = bounds.x + n.transform.tx * scale / 1024;
                n.transform.ty = bounds.y + n.transform.ty * scale / 1024;
                n.clip = source
                    .clip
                    .map(|clip| {
                        Rect::new(
                            bounds.x + clip.x * scale / 1024,
                            bounds.y + clip.y * scale / 1024,
                            (u64::from(clip.width) * scale as u64 / 1024) as u32,
                            (u64::from(clip.height) * scale as u64 / 1024) as u32,
                        )
                    })
                    .map_or(Some(bounds), |clip| clip.intersection(bounds));
                if n.clip.is_none() {
                    continue;
                }
                n.interaction = None;
                n.semantic = None;
                p.scene.nodes.push(n);
            }
        }
        p.region(
            card,
            &window.action("focus"),
            &format!("Resume {}", window.title),
        );
    }
    // Every live app is addressable, including those outside the horizontal card viewport.
    let count = ctx.windows.len().max(1) as i32;
    let icon_size = (w / (count + 1)).clamp(22, 42) as u32;
    for (i, window) in ctx.windows.iter().enumerate() {
        let x = w * (i as i32 + 1) / (count + 1) - icon_size as i32 / 2;
        p.platform_icon(
            Rect::new(x, h - 95, icon_size, icon_size),
            "android",
            &window.kind,
            &window.action("focus"),
            &format!("Resume {}", window.title),
        );
    }
}

pub fn chrome(p: &mut Painter, ctx: &ShellContext<'_>) {
    let w = ctx.width as i32;
    let h = ctx.height as i32;
    if ctx.launcher_open || ctx.panel == Some("search") {
        p.box_(Rect::new(0, 0, ctx.width, ctx.height), PAPER, 0);
        p.box_(
            Rect::new(18, 49, ctx.width.saturating_sub(36), 53),
            Color::rgb(226, 234, 215),
            27,
        );
        google(p, 38, 66, 20);
        p.text(
            80,
            65,
            ctx.width.saturating_sub(114),
            if ctx.search.is_empty() {
                "Search your apps"
            } else {
                ctx.search
            },
            16,
            Color::rgb(78, 86, 72),
        );
        p.region(
            Rect::new(18, 49, ctx.width.saturating_sub(36), 53),
            "shell:search",
            "Search installed applications",
        );
        let apps = [
            ("browser", "Browser"),
            ("calendar", "Calendar"),
            ("chat", "Chat"),
            ("docs", "Documents"),
            ("editor", "Editor"),
            ("files", "Files"),
            ("mail", "Mail"),
            ("terminal", "Terminal"),
        ];
        let cols = 4i32;
        let size = (ctx.width / 7).clamp(40, 58);
        for (i, (kind, label)) in apps
            .iter()
            .filter(|(kind, label)| {
                ctx.installed(kind)
                    && (ctx.search.is_empty()
                        || label.to_lowercase().contains(&ctx.search.to_lowercase())
                        || kind.contains(&ctx.search.to_lowercase()))
            })
            .enumerate()
        {
            let x = (i as i32 % cols * 2 + 1) * w / (cols * 2) - size as i32 / 2;
            let y = 132 + (i as i32 / cols) * 107;
            app(p, x, y, size, kind, label, true);
        }
    }
    if ctx.panel == Some("overview") {
        overview(p, ctx);
    }
    status(p, ctx);
    p.region(
        Rect::new(w - 110, 0, 110, 38),
        "shell:quick-settings",
        "Open quick settings",
    );
    p.region(
        Rect::new(0, 0, 95, 38),
        "shell:notifications",
        "Open notifications",
    );
    p.region(
        Rect::new(w - 72, h - 28, 72, 28),
        "shell:overview",
        "Recent applications gesture",
    );
    p.region(
        Rect::new(0, h - 28, 72, 28),
        "shell:mobile-back",
        "Back gesture",
    );
    // The gesture target is generous, even though the visible handle is minimal.
    p.region(
        Rect::new(w / 2 - 75, h - 28, 150, 28),
        "shell:home",
        "Home gesture",
    );
    p.box_(Rect::new(w / 2 - 47, h - 13, 94, 4), INK, 2);
}

pub fn window_frame(p: &mut Painter, ctx: &ShellContext<'_>, window: &WindowView) {
    let r = window.rect;
    p.box_(r, PAPER, 0);
    p.box_(Rect::new(r.x, r.y + 40, r.width, 56), PAPER, 0);
    p.line(
        vec![
            (r.x + 27, r.y + 61),
            (r.x + 20, r.y + 68),
            (r.x + 27, r.y + 75),
        ],
        INK,
        2,
    );
    p.region(
        Rect::new(r.x + 8, r.y + 45, 42, 43),
        "shell:mobile-back",
        "Back",
    );
    p.text(
        r.x + 58,
        r.y + 56,
        r.width.saturating_sub(112),
        &window.title,
        21,
        INK,
    );
    // Android keeps application content below status + standard 56 dp toolbar.
    let _ = ctx;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_scene::DesktopTheme;
    #[test]
    fn android_date_tracks_world_time_across_months_and_leap_years() {
        assert_eq!(date(0), "Thu, Sep 17");
        assert_eq!(date(14 * 86_400_000_000), "Thu, Oct 1");
        assert_eq!(date(105 * 86_400_000_000), "Thu, Dec 31");
        assert_eq!(date(106 * 86_400_000_000), "Fri, Jan 1");
    }
    #[test]
    fn android_home_has_native_asset_and_action_regions() {
        let ctx = ShellContext {
            theme: DesktopTheme::Android,
            width: 412,
            height: 892,
            clock_us: 0,
            title: "",
            launcher_open: false,
            active: false,
            windows: &[],
            installed_apps: &[],
            panel: None,
            search: "",
            hover: None,
        };
        let mut p = Painter::new(412, 892);
        background(&mut p, &ctx);
        chrome(&mut p, &ctx);
        let json = serde_json::to_string(&p.scene).unwrap();
        assert!(json.contains("wallpaper/android"));
        assert!(json.contains("shell:launcher"));
        assert!(json.contains("shell:home"));
        assert!(json.contains("shell:launch:browser"));
    }
}
