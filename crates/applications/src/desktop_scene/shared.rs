//! Small drawing API shared by independent platform shells.
use super::DesktopTheme;
use cw_scene::{Color, Node, Primitive, Rect, Scene};
#[derive(Clone, Debug)]
pub struct WindowView {
    pub id: u64,
    pub title: String,
    pub kind: String,
    pub rect: Rect,
    pub focused: bool,
    pub maximized: bool,
    pub minimized: bool,
    /// Application-local coordinates, excluding the native window frame.
    pub content: Option<Scene>,
}
impl WindowView {
    pub fn action(&self, name: &str) -> String {
        format!("window:{}:{name}", self.id)
    }
}
#[derive(Clone, Debug, Default)]
pub struct ShellOptions {
    pub installed_apps: Vec<String>,
    pub panel: Option<String>,
    pub search: String,
    pub hover: Option<(i32, i32)>,
}
pub struct ShellContext<'a> {
    pub theme: DesktopTheme,
    pub width: u32,
    pub height: u32,
    pub clock_us: u64,
    pub title: &'a str,
    pub launcher_open: bool,
    pub active: bool,
    pub windows: &'a [WindowView],
    pub installed_apps: &'a [String],
    pub panel: Option<&'a str>,
    pub search: &'a str,
    pub hover: Option<(i32, i32)>,
}
impl ShellContext<'_> {
    pub fn hovered(&self, rect: Rect) -> bool {
        self.hover.is_some_and(|(x, y)| rect.contains(x, y))
    }
    pub fn installed(&self, id: &str) -> bool {
        self.installed_apps.is_empty() || self.installed_apps.iter().any(|a| a == id)
    }
    pub fn time(&self) -> String {
        format!(
            "{:02}:{:02}",
            9 + (self.clock_us / 3_600_000_000) % 12,
            (self.clock_us / 60_000_000) % 60
        )
    }
}
pub struct Painter {
    pub scene: Scene,
    pub next: u64,
    pub z: i32,
}
impl Painter {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            scene: Scene::new(width, height),
            next: 1 << 60,
            z: 0,
        }
    }
    pub fn node(&mut self, r: Rect, p: Primitive, interaction: Option<(&str, &str)>) {
        let mut n = Node::new(self.next, r, p);
        self.next += 1;
        n.z = self.z;
        if let Some((action, label)) = interaction {
            n = n.interactive(action, "button", label);
        }
        self.scene.nodes.push(n);
    }
    pub fn region(&mut self, r: Rect, action: &str, label: &str) {
        self.node(r, Primitive::Region, Some((action, label)));
    }
    pub fn box_(&mut self, r: Rect, c: Color, radius: u32) {
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
    pub fn border(&mut self, r: Rect, c: Color, radius: u32, border: Color) {
        self.node(
            r,
            Primitive::RoundedBox {
                fill: c,
                border: Some(border),
                border_width: 1,
                radius,
            },
            None,
        );
    }
    pub fn text(&mut self, x: i32, y: i32, w: u32, text: &str, size: u16, c: Color) {
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
    pub fn button(&mut self, r: Rect, c: Color, radius: u32, action: &str, label: &str) {
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
    pub fn path(&mut self, points: Vec<(i32, i32)>, fill: Color) {
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
    pub fn line(&mut self, points: Vec<(i32, i32)>, color: Color, thickness: u16) {
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
    pub fn shadow(&mut self, r: Rect, radius: u32) {
        self.node(
            Rect::new(r.x - 18, r.y - 12, r.width + 36, r.height + 36),
            Primitive::Shadow {
                color: Color(0, 0, 0, 85),
                radius,
                blur: 18,
            },
            None,
        );
    }
    pub fn bold(&mut self, x: i32, y: i32, w: u32, text: &str, size: u16, c: Color) {
        self.node(
            Rect::new(x, y, w, u32::from(size) + 9),
            Primitive::UiTextBold {
                text: text.into(),
                color: c,
                size,
            },
            None,
        );
    }
    /// Asset ids are stable serialized identifiers, resolved by the renderer.
    pub fn asset(&mut self, r: Rect, id: &str) {
        self.node(r, Primitive::AssetImage { asset: id.into() }, None);
    }
    pub fn icon(&mut self, x: i32, y: i32, size: u32, kind: &str, label: bool) {
        self.asset(Rect::new(x, y, size, size), &format!("icon/common/{kind}"));
        self.region(
            Rect::new(x, y, size, size),
            &format!("shell:launch:{kind}"),
            &format!("Open {kind}"),
        );
        if label {
            self.text(
                x - 16,
                y + size as i32 + 5,
                size + 32,
                kind,
                12,
                Color::WHITE,
            );
        }
    }
    pub fn platform_icon(
        &mut self,
        r: Rect,
        platform: &str,
        kind: &str,
        action: &str,
        label: &str,
    ) {
        self.asset(r, &format!("icon/{platform}/{kind}"));
        self.region(r, action, label);
    }
}
