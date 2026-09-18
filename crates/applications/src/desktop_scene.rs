//! Native OS shell composition. Application scenes retain independent state and coordinates.
use cw_scene::{Node, Primitive, Rect, Scene};
mod app_content;
pub mod shared;
pub use app_content::{app_content, app_content_with};
pub use shared::{Painter, ShellContext, ShellOptions, WindowView};
mod android;
mod ios;
pub(crate) mod macos;
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
    /// Asset namespace of the platform's icon set.
    pub fn platform(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Ubuntu => "ubuntu",
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
    /// Bundled family standing in for the platform's system font.
    pub fn typeface(self) -> cw_scene::Typeface {
        match self {
            Self::Macos | Self::Ios => cw_scene::Typeface::Inter,
            Self::Windows => cw_scene::Typeface::OpenSans,
            Self::Ubuntu => cw_scene::Typeface::Ubuntu,
            Self::Android => cw_scene::Typeface::Roboto,
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
/// Height of Android's three-button navigation bar, 48 dp. Swipes that start on it are
/// presses of its buttons, not gestures.
pub const ANDROID_NAV_BAR: i32 = 48;
pub fn window_content_rect(theme: DesktopTheme, frame: Rect) -> Rect {
    let top = match theme {
        DesktopTheme::Macos => 42,
        DesktopTheme::Windows => 38,
        DesktopTheme::Ubuntu => 46,
        _ => 96,
    };
    // Phones keep their system bar clear: the home indicator on iOS, the 48 dp
    // three-button navigation bar on Android.
    let bottom = match theme {
        DesktopTheme::Ios => 32,
        DesktopTheme::Android => ANDROID_NAV_BAR,
        _ => 1,
    };
    let side = if theme.mobile() { 0 } else { 1 };
    Rect::new(
        frame.x + side,
        frame.y + top,
        frame.width.saturating_sub((side * 2) as u32).max(1),
        frame.height.saturating_sub((top + bottom) as u32).max(1),
    )
}
/// Height of the application bar a phone paints above ordinary application content.
pub const PHONE_APP_BAR: u32 = 46;
/// Client rectangle of a window. Browsers reserve room for their own toolbars: a
/// 40 px navigation row on desktops, Safari's address bar and bottom toolbar on iOS;
/// Chrome's toolbar on Android occupies the ordinary application bar.
pub fn window_content_rect_for_kind(theme: DesktopTheme, frame: Rect, kind: &str) -> Rect {
    let mut r = window_content_rect(theme, frame);
    // A phone's music player draws its own top bar (Apple Music's large titles, YouTube
    // Music's logo row), so it starts right under the status bar with no app bar above.
    if kind == "music" && theme.mobile() {
        r.y -= PHONE_APP_BAR as i32;
        r.height += PHONE_APP_BAR;
    }
    // Visual Studio Code draws its own 35 px title bar in place of the platform's.
    if kind == "code" && !theme.mobile() {
        let top = crate::apps::code::frame::TITLE_H;
        return Rect::new(
            frame.x + 1,
            frame.y + top as i32,
            frame.width.saturating_sub(2).max(1),
            frame.height.saturating_sub(top + 1).max(1),
        );
    }
    if kind == "browser" {
        let (top, bottom) = match theme {
            DesktopTheme::Ios => (8, 48),
            DesktopTheme::Android => (0, 0),
            _ => (40, 0),
        };
        r.y += top as i32;
        r.height = r.height.saturating_sub(top + bottom).max(1);
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
/// Pages of a paged home screen at this screen size, for these installed applications
/// (empty meaning all). Only iOS pages its home screen; every other shell has one.
/// The router asks this so a swipe walks exactly the pages the shell paints.
pub fn home_page_count(theme: DesktopTheme, installed: &[String], width: u32, height: u32) -> u32 {
    match theme {
        DesktopTheme::Ios => ios::home_pages(installed, width, height),
        _ => 1,
    }
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
        desktop_selection: options.desktop_selection.as_deref(),
        settings: &options.settings,
        screen: options.screen,
        panel_month: options.panel_month,
        text_entry: options.text_entry,
        keyboard: options.keyboard,
        bookmarks: &options.bookmarks,
        downloads: &options.downloads,
        notifications: &options.notifications,
        workspaces: options.workspaces.max(1),
        workspace: options.workspace,
        library_group: options.library_group.as_deref(),
        bookmarked: options.bookmarked,
        panel_over_launcher: options.panel_over_launcher,
        typed: &options.typed,
        home_page: options.home_page,
        user: &options.user,
        home: &options.home,
        recents: &options.recents,
    };
    let mut p = Painter::themed(theme, width, height, 1 << 60);
    background(&mut p, &ctx);
    for (index, w) in windows.iter().filter(|w| !w.minimized).enumerate() {
        p.z = 100 + index as i32 * 1000;
        p.region(w.rect, &w.action("focus"), &w.title);
        frame(&mut p, &ctx, w);
        let client = p.scene.nodes.len();
        if w.kind == "browser" {
            browser_chrome(&mut p, &ctx, w);
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
                // A rounded clip the application drew (rounded artwork) moves with it.
                if let Some(rounded) = &mut n.rounded_clip {
                    rounded.rect.x = rounded.rect.x.saturating_add(r.x);
                    rounded.rect.y = rounded.rect.y.saturating_add(r.y);
                }
                if let Some(action) = &n.interaction {
                    n.interaction = Some(w.action(&format!("content:{action}")));
                }
                // Platform-neutral page artwork adopts the host platform's icon set.
                if let Primitive::AssetImage { asset } = &mut n.primitive {
                    if let Some(name) = asset.strip_prefix("icon/common/") {
                        *asset = format!("icon/{}/{name}", theme.platform());
                    }
                }
                p.scene.nodes.push(n);
            }
        }
        // Client pixels follow the frame's rounded silhouette, inside its hairline.
        let radius = corner_radius(theme, w.maximized);
        p.round_clip_since(
            client,
            Rect::new(
                w.rect.x + 1,
                w.rect.y + 1,
                w.rect.width.saturating_sub(2),
                w.rect.height.saturating_sub(2),
            ),
            radius.saturating_sub(1),
        );
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
fn browser_chrome(p: &mut Painter, c: &ShellContext, w: &WindowView) {
    match c.theme {
        DesktopTheme::Macos => macos::browser_chrome(p, c, w),
        DesktopTheme::Windows => windows::browser_chrome(p, c, w),
        DesktopTheme::Ubuntu => ubuntu::browser_chrome(p, c, w),
        DesktopTheme::Ios => ios::browser_chrome(p, c, w),
        DesktopTheme::Android => android::browser_chrome(p, c, w),
    }
}
/// Corner radius of a free-floating window; maximized and phone windows are square.
pub fn corner_radius(theme: DesktopTheme, maximized: bool) -> u32 {
    match theme {
        _ if maximized => 0,
        DesktopTheme::Macos => 10,
        DesktopTheme::Windows => 8,
        DesktopTheme::Ubuntu => 12,
        DesktopTheme::Ios | DesktopTheme::Android => 0,
    }
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
            ..Default::default()
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
                ..Default::default()
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
                    // Toolbars grey history they have not got, so this window has some.
                    document: "http://intranet.internal/".into(),
                    can_go_back: true,
                    can_go_forward: true,
                    ..Default::default()
                }],
            );
            let site = window_content_rect_for_kind(theme, frame, "browser");
            let base = window_content_rect(theme, frame);
            assert_eq!(
                site.y,
                base.y + if theme.mobile() { site.y - base.y } else { 40 }
            );
            assert!(site.y >= base.y && site.y + site.height as i32 <= base.y + base.height as i32);
            for action in ["back", "forward", "reload", "address"] {
                let target = format!("window:7:content:shell:{action}");
                let n = scene
                    .nodes
                    .iter()
                    .find(|n| n.interaction.as_deref() == Some(&target))
                    .unwrap();
                let r = n.bounds;
                // Toolbars sit above the page, or below it for Safari on iOS.
                assert!(r.y + r.height as i32 <= site.y || r.y >= site.y + site.height as i32);
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
