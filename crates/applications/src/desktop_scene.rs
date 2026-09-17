//! Native OS shell composition. Application scenes retain independent state and coordinates.
use cw_scene::{Node, Primitive, Rect, Scene};
mod app_content;
pub mod shared;
pub use app_content::app_content;
pub use shared::{Painter, ShellContext, ShellOptions, WindowView};
mod android;
mod ios;
mod macos;
mod ubuntu;
mod windows;
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
    pub fn mobile(self) -> bool {
        matches!(self, Self::Ios | Self::Android)
    }
}
pub fn work_area(theme: DesktopTheme, width: u32, height: u32) -> Rect {
    let (x, y, right, bottom) = match theme {
        DesktopTheme::Macos => (0, 28, 0, 78),
        DesktopTheme::Windows => (0, 0, 0, 52),
        DesktopTheme::Ubuntu => (68, 32, 0, 0),
        _ => (0, 0, 0, 0),
    };
    Rect::new(
        x,
        y,
        width.saturating_sub((x + right) as u32).max(1),
        height.saturating_sub((y + bottom) as u32).max(1),
    )
}
pub fn window_content_rect(theme: DesktopTheme, frame: Rect) -> Rect {
    let top = match theme {
        DesktopTheme::Macos => 42,
        DesktopTheme::Windows => 38,
        DesktopTheme::Ubuntu => 46,
        _ => 96,
    };
    let bottom = if theme.mobile() { 32 } else { 1 };
    let side = if theme.mobile() { 0 } else { 1 };
    Rect::new(
        frame.x + side,
        frame.y + top,
        frame.width.saturating_sub((side * 2) as u32).max(1),
        frame.height.saturating_sub((top + bottom) as u32).max(1),
    )
}
pub fn window_content_rect_for_kind(theme: DesktopTheme, frame: Rect, kind: &str) -> Rect {
    let mut r = window_content_rect(theme, frame);
    if kind == "browser" {
        r.y += 40;
        r.height = r.height.saturating_sub(40).max(1);
    }
    r
}
fn default_frame(theme: DesktopTheme, width: u32, height: u32, maximized: bool) -> Rect {
    let area = work_area(theme, width, height);
    if maximized || theme.mobile() {
        return area;
    }
    let inset = (width / 15).min(72);
    let top = (height / 11).min(64);
    Rect::new(
        area.x + inset as i32,
        area.y + top as i32,
        area.width.saturating_sub(inset * 2).max(1),
        area.height.saturating_sub(top + 24).max(1),
    )
}
pub fn content_rect(theme: DesktopTheme, width: u32, height: u32, maximized: bool) -> Rect {
    window_content_rect(theme, default_frame(theme, width, height, maximized))
}
fn background(p: &mut Painter, c: &ShellContext) {
    match c.theme {
        DesktopTheme::Macos => macos::background(p, c),
        DesktopTheme::Windows => windows::background(p, c),
        DesktopTheme::Ubuntu => ubuntu::background(p, c),
        DesktopTheme::Ios => ios::background(p, c),
        DesktopTheme::Android => android::background(p, c),
    }
}
fn chrome(p: &mut Painter, c: &ShellContext) {
    match c.theme {
        DesktopTheme::Macos => macos::chrome(p, c),
        DesktopTheme::Windows => windows::chrome(p, c),
        DesktopTheme::Ubuntu => ubuntu::chrome(p, c),
        DesktopTheme::Ios => ios::chrome(p, c),
        DesktopTheme::Android => android::chrome(p, c),
    }
}
fn frame(p: &mut Painter, c: &ShellContext, w: &WindowView) {
    match c.theme {
        DesktopTheme::Macos => macos::window_frame(p, c, w),
        DesktopTheme::Windows => windows::window_frame(p, c, w),
        DesktopTheme::Ubuntu => ubuntu::window_frame(p, c, w),
        DesktopTheme::Ios => ios::window_frame(p, c, w),
        DesktopTheme::Android => android::window_frame(p, c, w),
    }
}
pub fn render_desktop(
    theme: DesktopTheme,
    width: u32,
    height: u32,
    clock_us: u64,
    launcher_open: bool,
    windows: Vec<WindowView>,
) -> Scene {
    render_desktop_with_options(
        theme,
        width,
        height,
        clock_us,
        launcher_open,
        windows,
        ShellOptions::default(),
    )
}
pub fn render_desktop_with_options(
    theme: DesktopTheme,
    width: u32,
    height: u32,
    clock_us: u64,
    launcher_open: bool,
    windows: Vec<WindowView>,
    options: ShellOptions,
) -> Scene {
    if width < 240 || height < 240 {
        // Tiny structured thumbnails need not construct an unusable OS shell.
        let mut scene = windows
            .iter()
            .rev()
            .find(|w| !w.minimized)
            .and_then(|w| w.content.clone())
            .unwrap_or_else(|| Scene::new(width, height));
        scene.width = width;
        scene.height = height;
        scene.revision = clock_us;
        for n in &mut scene.nodes {
            n.clip = Some(Rect::new(0, 0, width, height));
        }
        return scene;
    }
    let title = windows
        .iter()
        .find(|w| w.focused && !w.minimized)
        .map(|w| w.title.as_str())
        .unwrap_or("");
    let ctx = ShellContext {
        theme,
        width,
        height,
        clock_us,
        title,
        launcher_open,
        active: windows.iter().any(|w| !w.minimized),
        windows: &windows,
        installed_apps: &options.installed_apps,
        panel: options.panel.as_deref(),
        search: &options.search,
        hover: options.hover,
    };
    let mut p = Painter::new(width, height);
    background(&mut p, &ctx);
    for (index, w) in windows.iter().filter(|w| !w.minimized).enumerate() {
        p.z = 100 + index as i32 * 1000;
        p.region(w.rect, &w.action("focus"), &w.title);
        frame(&mut p, &ctx, w);
        if w.kind == "browser" {
            browser_bar(&mut p, &ctx, w);
        }
        if let Some(content) = &w.content {
            let r = window_content_rect_for_kind(theme, w.rect, &w.kind);
            p.z += 10;
            p.node(
                r,
                Primitive::Box {
                    fill: content.background,
                    border: None,
                    border_width: 0,
                },
                Some((&w.action("focus"), &w.title)),
            );
            let mut nodes: Vec<&Node> = content.nodes.iter().collect();
            nodes.sort_by_key(|n| (n.z, n.id));
            for node in nodes {
                let mut n = node.clone();
                n.id = p.next;
                p.next += 1;
                n.z = p.z;
                n.transform.tx = n.transform.tx.saturating_add(r.x);
                n.transform.ty = n.transform.ty.saturating_add(r.y);
                n.clip = match n.clip {
                    Some(c) => Rect::new(c.x + r.x, c.y + r.y, c.width, c.height).intersection(r),
                    None => Some(r),
                };
                if n.clip.is_none() {
                    continue;
                }
                if let Some(action) = &n.interaction {
                    n.interaction = Some(w.action(&format!("content:{action}")));
                }
                p.scene.nodes.push(n);
            }
        }
        if !theme.mobile() && !w.maximized {
            p.z += 20;
            resize_regions(&mut p, w);
        }
    }
    p.z = 1_000_000;
    chrome(&mut p, &ctx);
    p.scene.revision = clock_us;
    p.scene
}
fn browser_bar(p: &mut Painter, c: &ShellContext, w: &WindowView) {
    let r = window_content_rect(c.theme, w.rect);
    let ink = cw_scene::Color::rgb(66, 70, 80);
    p.box_(
        Rect::new(r.x, r.y, r.width, 40),
        cw_scene::Color::rgb(242, 243, 246),
        0,
    );
    for (i, action, label) in [
        (0, "shell:back", "Back"),
        (1, "shell:forward", "Forward"),
        (2, "shell:reload", "Reload"),
    ] {
        let x = r.x + 8 + i * 29;
        let y = r.y + 8;
        if c.hovered(Rect::new(x, y, 26, 26)) {
            p.box_(
                Rect::new(x, y, 26, 26),
                cw_scene::Color::rgb(222, 225, 230),
                5,
            );
        }
        p.region(
            Rect::new(x, y, 26, 26),
            &w.action(&format!("content:{action}")),
            label,
        );
        if i < 2 {
            let sign = if i == 0 { -1 } else { 1 };
            let center = x + 13;
            p.line(
                vec![
                    (center - sign * 4, y + 6),
                    (center + sign * 3, y + 12),
                    (center - sign * 4, y + 18),
                ],
                ink,
                1,
            );
        } else {
            p.border(
                Rect::new(x + 6, y + 6, 13, 13),
                cw_scene::Color::TRANSPARENT,
                7,
                ink,
            );
            p.line(
                vec![(x + 18, y + 4), (x + 18, y + 10), (x + 12, y + 10)],
                ink,
                1,
            );
        }
    }
    let address = Rect::new(r.x + 102, r.y + 6, r.width.saturating_sub(114), 28);
    p.button(
        address,
        cw_scene::Color::WHITE,
        7,
        &w.action("content:shell:address"),
        "Address and search",
    );
    let text = w
        .title
        .strip_prefix("Browser — ")
        .or_else(|| w.title.strip_prefix("Browser - "))
        .unwrap_or(&w.title);
    p.text(
        address.x + 12,
        address.y + 5,
        address.width.saturating_sub(24),
        text,
        12,
        ink,
    );
}
fn resize_regions(p: &mut Painter, w: &WindowView) {
    let r = w.rect;
    let x = r.x;
    let y = r.y;
    let right = x + r.width as i32;
    let bottom = y + r.height as i32;
    for (edge, rect) in [
        ("n", Rect::new(x + 8, y - 3, r.width.saturating_sub(16), 6)),
        (
            "s",
            Rect::new(x + 8, bottom - 3, r.width.saturating_sub(16), 6),
        ),
        ("w", Rect::new(x - 3, y + 8, 6, r.height.saturating_sub(16))),
        (
            "e",
            Rect::new(right - 3, y + 8, 6, r.height.saturating_sub(16)),
        ),
        ("nw", Rect::new(x - 4, y - 4, 12, 12)),
        ("ne", Rect::new(right - 8, y - 4, 12, 12)),
        ("sw", Rect::new(x - 4, bottom - 8, 12, 12)),
        ("se", Rect::new(right - 8, bottom - 8, 12, 12)),
    ] {
        p.region(
            rect,
            &w.action(&format!("resize:{edge}")),
            &format!("Resize {} {edge}", w.title),
        );
    }
}
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
    let windows = content
        .map(|content| WindowView {
            id: 0,
            title: title.into(),
            kind: if title.starts_with("Browser") {
                "browser".into()
            } else {
                "terminal".into()
            },
            rect: default_frame(theme, width, height, maximized),
            focused: true,
            maximized,
            minimized: false,
            content: Some(content),
        })
        .into_iter()
        .collect();
    render_desktop(theme, width, height, clock_us, launcher_open, windows)
}
#[cfg(test)]
mod tests {
    use super::*;
    use cw_scene::Color;
    #[test]
    fn every_window_is_composed_and_actions_are_namespaced() {
        let mut content = Scene::new(200, 100);
        content.nodes.push(
            Node::rectangle(1, Rect::new(5, 5, 20, 20), Color::WHITE)
                .interactive("open:0", "button", "File"),
        );
        let windows = (1..=2)
            .map(|id| WindowView {
                id,
                title: format!("Window {id}"),
                kind: "files".into(),
                rect: Rect::new(80 * id as i32, 70 * id as i32, 300, 220),
                focused: id == 2,
                maximized: false,
                minimized: false,
                content: Some(content.clone()),
            })
            .collect();
        let scene = render_desktop(DesktopTheme::Macos, 960, 640, 0, false, windows);
        for id in [1, 2] {
            assert!(scene
                .nodes
                .iter()
                .any(|n| n.interaction.as_deref() == Some(&format!("window:{id}:content:open:0"))));
            assert!(scene
                .nodes
                .iter()
                .any(|n| n.interaction.as_deref() == Some(&format!("window:{id}:resize:se"))));
        }
        assert_eq!(
            scene.hit_test(168, 189).unwrap().interaction.as_deref(),
            Some("window:2:content:open:0")
        );
    }
    #[test]
    fn browser_toolbar_is_separate_from_clipped_page() {
        for theme in [
            DesktopTheme::Macos,
            DesktopTheme::Windows,
            DesktopTheme::Ubuntu,
            DesktopTheme::Ios,
            DesktopTheme::Android,
        ] {
            let frame = Rect::new(80, 60, 500, 400);
            let content = Scene::new(500, 320);
            let scene = render_desktop(
                theme,
                960,
                640,
                0,
                false,
                vec![WindowView {
                    id: 7,
                    title: "Browser — http://intranet.internal/".into(),
                    kind: "browser".into(),
                    rect: frame,
                    focused: true,
                    maximized: false,
                    minimized: false,
                    content: Some(content),
                }],
            );
            let site = window_content_rect_for_kind(theme, frame, "browser");
            let base = window_content_rect(theme, frame);
            assert_eq!(site.y, base.y + 40);
            for action in ["back", "forward", "reload", "address"] {
                let target = format!("window:7:content:shell:{action}");
                let n = scene
                    .nodes
                    .iter()
                    .find(|n| n.interaction.as_deref() == Some(&target))
                    .unwrap();
                let r = n.bounds;
                assert!(r.y < site.y);
                assert_eq!(
                    scene
                        .hit_test(r.x + r.width as i32 / 2, r.y + r.height as i32 / 2)
                        .unwrap()
                        .interaction
                        .as_deref(),
                    Some(target.as_str())
                );
            }
        }
    }
    #[test]
    fn geometry_stays_positive() {
        for t in [
            DesktopTheme::Macos,
            DesktopTheme::Windows,
            DesktopTheme::Ubuntu,
            DesktopTheme::Ios,
            DesktopTheme::Android,
        ] {
            assert!(window_content_rect(t, Rect::new(0, 0, 1, 1)).height > 0);
        }
    }
}
