//! Portable synthetic scene contracts. Geometry is integer pixels; affine coefficients
//! use 1/1024 units. This crate never rasterizes or consults host fonts.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub mod metrics;
pub mod text;
#[rustfmt::skip]
mod metrics_data;
#[rustfmt::skip]
mod metrics_italic;
pub use metrics::{Lang, Style, Typeface};

pub const SCENE_VERSION: u32 = 2;
/// Maximum raster target: 64 MiB of RGBA. Structured scenes share this viewport bound.
pub const MAX_PIXELS: u64 = 16_777_216;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        i64::from(x) >= i64::from(self.x)
            && i64::from(y) >= i64::from(self.y)
            && i64::from(x) < i64::from(self.x) + i64::from(self.width)
            && i64::from(y) < i64::from(self.y) + i64::from(self.height)
    }
    pub fn right(&self) -> i32 {
        (i64::from(self.x) + i64::from(self.width)).clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
    pub fn bottom(&self) -> i32 {
        (i64::from(self.y) + i64::from(self.height)).clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
    pub fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
    /// Smallest rectangle covering both; an empty operand is ignored.
    pub fn union(self, b: Self) -> Self {
        if self.area() == 0 {
            return b;
        }
        if b.area() == 0 {
            return self;
        }
        let x = self.x.min(b.x);
        let y = self.y.min(b.y);
        let r = self.right().max(b.right());
        let d = self.bottom().max(b.bottom());
        Self::new(
            x,
            y,
            (r as i64 - x as i64) as u32,
            (d as i64 - y as i64) as u32,
        )
    }
    /// The parts of `self` that `other` does not cover, as up to four rectangles.
    /// Occlusion is reported as real geometry rather than a boolean.
    pub fn subtract(self, other: Self) -> Vec<Self> {
        let Some(cut) = self.intersection(other) else {
            return vec![self];
        };
        let mut parts = Vec::new();
        if cut.y > self.y {
            parts.push(Self::new(
                self.x,
                self.y,
                self.width,
                (cut.y - self.y) as u32,
            ));
        }
        if cut.bottom() < self.bottom() {
            parts.push(Self::new(
                self.x,
                cut.bottom(),
                self.width,
                (self.bottom() as i64 - cut.bottom() as i64) as u32,
            ));
        }
        if cut.x > self.x {
            parts.push(Self::new(
                self.x,
                cut.y,
                (cut.x - self.x) as u32,
                cut.height,
            ));
        }
        if cut.right() < self.right() {
            parts.push(Self::new(
                cut.right(),
                cut.y,
                (self.right() as i64 - cut.right() as i64) as u32,
                cut.height,
            ));
        }
        parts
    }
    pub fn intersection(self, b: Self) -> Option<Self> {
        let x = i64::from(self.x).max(b.x as i64);
        let y = i64::from(self.y).max(b.y as i64);
        let r = (self.x as i64 + self.width as i64).min(b.x as i64 + b.width as i64);
        let d = (self.y as i64 + self.height as i64).min(b.y as i64 + b.height as i64);
        if r > x && d > y {
            Some(Self::new(
                x as i32,
                y as i32,
                (r - x) as u32,
                (d - y) as u32,
            ))
        } else {
            None
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);
impl Color {
    pub const WHITE: Self = Self(255, 255, 255, 255);
    pub const BLACK: Self = Self(0, 0, 0, 255);
    pub const TRANSPARENT: Self = Self(0, 0, 0, 0);
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 255)
    }
}
impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transform {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
    pub tx: i32,
    pub ty: i32,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            a: 1024,
            b: 0,
            c: 0,
            d: 1024,
            tx: 0,
            ty: 0,
        }
    }
}
impl Transform {
    pub fn translate(x: i32, y: i32) -> Self {
        Self {
            tx: x,
            ty: y,
            ..Self::default()
        }
    }
    pub fn point(&self, x: i32, y: i32) -> (i32, i32) {
        let x = x as i128;
        let y = y as i128;
        (
            ((self.a as i128 * x + self.c as i128 * y).div_euclid(1024) + self.tx as i128)
                .clamp(i32::MIN as i128, i32::MAX as i128) as i32,
            ((self.b as i128 * x + self.d as i128 * y).div_euclid(1024) + self.ty as i128)
                .clamp(i32::MIN as i128, i32::MAX as i128) as i32,
        )
    }
    pub fn inverse_point(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let det = self.a as i128 * self.d as i128 - self.b as i128 * self.c as i128;
        if det == 0 {
            return None;
        }
        let x = x as i128 - self.tx as i128;
        let y = y as i128 - self.ty as i128;
        let px = (1024 * (self.d as i128 * x - self.c as i128 * y)).div_euclid(det);
        let py = (1024 * (-(self.b as i128) * x + self.a as i128 * y)).div_euclid(det);
        Some((i32::try_from(px).ok()?, i32::try_from(py).ok()?))
    }
    pub fn bounds(&self, r: Rect) -> Rect {
        let right = (r.x as i64 + r.width as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let bottom = (r.y as i64 + r.height as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let pts = [
            self.point(r.x, r.y),
            self.point(right, r.y),
            self.point(r.x, bottom),
            self.point(right, bottom),
        ];
        let x = pts.iter().map(|p| p.0).min().unwrap();
        let y = pts.iter().map(|p| p.1).min().unwrap();
        let right = pts.iter().map(|p| p.0).max().unwrap();
        let bottom = pts.iter().map(|p| p.1).max().unwrap();
        Rect::new(
            x,
            y,
            (right as i64 - x as i64) as u32,
            (bottom as i64 - y as i64) as u32,
        )
    }
}
fn is_false(b: &bool) -> bool {
    !*b
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Primitive {
    Box {
        fill: Color,
        border: Option<Color>,
        border_width: u32,
    },
    /// Rounded rectangle; radius clamps to half the smaller extent.
    RoundedBox {
        fill: Color,
        border: Option<Color>,
        border_width: u32,
        radius: u32,
    },
    /// Proportional bundled sans-serif text, pixel-wrapped within the bounds.
    /// `italic` and `lang` are optional in scene JSON and omitted when default.
    UiText {
        text: String,
        color: Color,
        size: u16,
        #[serde(default, skip_serializing_if = "is_false")]
        italic: bool,
        /// Language of the text, which picks regional Han forms (see [`Lang`]).
        #[serde(default, skip_serializing_if = "Lang::is_auto")]
        lang: Lang,
    },
    UiTextBold {
        text: String,
        color: Color,
        size: u16,
        #[serde(default, skip_serializing_if = "is_false")]
        italic: bool,
        #[serde(default, skip_serializing_if = "Lang::is_auto")]
        lang: Lang,
    },
    Text {
        text: String,
        color: Color,
        size: u16,
    },
    /// Stable bundled resource identifier. No host paths or network fetches.
    AssetImage { asset: String },
    /// Soft rounded shadow; bounds include `blur` pixels of padding on every side.
    Shadow {
        color: Color,
        radius: u32,
        blur: u32,
    },
    Image {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    Path {
        points: Vec<(i32, i32)>,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: u16,
        closed: bool,
    },
    /// Bundled monochrome glyph (`symbol/...`) whose alpha mask is tinted with `color`.
    Symbol { asset: String, color: Color },
    /// Frosted glass: blurs everything already painted beneath the rounded bounds.
    /// `blur` is the box radius of each of three passes, capped by the renderer.
    Backdrop { radius: u32, blur: u32 },
    /// Invisible layout/interaction region.
    Region,
}
impl Primitive {
    /// UI text set in `style`: `UiTextBold` when bold, otherwise `UiText`.
    pub fn ui_text(text: impl Into<String>, color: Color, size: u16, style: Style) -> Self {
        let text = text.into();
        let Style { bold, italic, lang } = style;
        if bold {
            Self::UiTextBold {
                text,
                color,
                size,
                italic,
                lang,
            }
        } else {
            Self::UiText {
                text,
                color,
                size,
                italic,
                lang,
            }
        }
    }
    /// Weight, slant and language of a UI text primitive; `None` for anything else.
    pub fn text_style(&self) -> Option<Style> {
        match self {
            Self::UiText { italic, lang, .. } => Some(Style::new(false, *italic, *lang)),
            Self::UiTextBold { italic, lang, .. } => Some(Style::new(true, *italic, *lang)),
            _ => None,
        }
    }
}
/// Rounded clip in scene coordinates, applied in addition to `Node::clip`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundedClip {
    pub rect: Rect,
    pub radius: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Semantic {
    pub role: String,
    pub label: String,
    pub value: Option<String>,
    pub disabled: bool,
    pub focusable: bool,
}
/// Accessibility state that `Semantic` cannot carry: shells outside this crate build
/// `Semantic` with exhaustive struct literals, so it can never gain a field.
/// `None` means "not applicable to this role", which is distinct from `Some(false)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    /// Holds keyboard focus. Mirrors `Scene::focus`, per node.
    #[serde(default)]
    pub focused: bool,
}
impl NodeState {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}
/// Provenance of one painted line of text. A consumer that joins painted lines without
/// this corrupts wrapped text (`initial commi` + `t`); `continuation` says which joins
/// take no separator.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextLine {
    /// Index of the logical line in the owning pane's buffer.
    pub logical: u32,
    /// This fragment continues the previous visual line: join with no separator.
    pub continuation: bool,
    /// Character offset of this fragment within its logical line.
    #[serde(default)]
    pub offset: u32,
    /// Node id of the first visual fragment of the same logical line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapped_from: Option<u64>,
    /// Handle of the `Scene::buffers` entry this line was painted from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: u64,
    pub bounds: Rect,
    pub primitive: Primitive,
    #[serde(default)]
    pub semantic: Option<Semantic>,
    #[serde(default)]
    pub interaction: Option<String>,
    /// Clip rectangle in scene coordinates; nested clips are flattened at layout time.
    #[serde(default)]
    pub clip: Option<Rect>,
    /// Antialiased rounded clip, e.g. window corners. Coordinates are untransformed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rounded_clip: Option<RoundedClip>,
    #[serde(default)]
    pub transform: Transform,
    #[serde(default)]
    pub z: i32,
    #[serde(default = "opaque")]
    pub opacity: u8,
    /// Owning entry in `Scene::windows`. Authoritative when the node carries a
    /// `window:<id>:` interaction; attributed by geometry otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<NodeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<TextLine>,
    /// Content digest, filled by `Scene::stamp`. Equal revisions mean the node paints
    /// and announces exactly the same thing; 0 means the scene was never stamped.
    /// Derived from content, never counted, so it survives snapshot restore and fork.
    #[serde(default, skip_serializing_if = "is_unstamped")]
    pub revision: u64,
}
fn opaque() -> u8 {
    255
}
fn is_unstamped(revision: &u64) -> bool {
    *revision == 0
}
impl Node {
    pub fn new(id: u64, bounds: Rect, primitive: Primitive) -> Self {
        Self {
            id,
            bounds,
            primitive,
            semantic: None,
            interaction: None,
            clip: None,
            rounded_clip: None,
            transform: Transform::default(),
            z: 0,
            opacity: 255,
            window: None,
            state: None,
            line: None,
            revision: 0,
        }
    }
    /// Text this node paints, if any.
    pub fn painted_text(&self) -> Option<&str> {
        match &self.primitive {
            Primitive::Text { text, .. }
            | Primitive::UiText { text, .. }
            | Primitive::UiTextBold { text, .. } => Some(text),
            _ => None,
        }
    }
    /// Nominal advance of one character cell for this node's text primitive.
    pub fn cell(&self) -> Option<(u32, u32)> {
        match &self.primitive {
            Primitive::Text { size, .. }
            | Primitive::UiText { size, .. }
            | Primitive::UiTextBold { size, .. } => Some(text_cell(*size)),
            _ => None,
        }
    }
    /// Paints over everything beneath it inside its bounds. Used to make occlusion
    /// legible: `hit_test` only sees interactive nodes, painting sees all of them.
    pub fn is_opaque(&self) -> bool {
        if self.opacity != 255 {
            return false;
        }
        match &self.primitive {
            Primitive::Box { fill, .. } => fill.3 == 255,
            Primitive::RoundedBox { fill, radius, .. } => fill.3 == 255 && *radius == 0,
            Primitive::Image { .. } | Primitive::AssetImage { .. } => true,
            _ => false,
        }
    }
    pub fn asset(id: u64, bounds: Rect, asset: impl Into<String>) -> Self {
        Self::new(
            id,
            bounds,
            Primitive::AssetImage {
                asset: asset.into(),
            },
        )
    }
    pub fn text(id: u64, bounds: Rect, text: impl Into<String>, size: u16, color: Color) -> Self {
        Self::new(
            id,
            bounds,
            Primitive::Text {
                text: text.into(),
                color,
                size,
            },
        )
    }
    pub fn ui_text(
        id: u64,
        bounds: Rect,
        text: impl Into<String>,
        size: u16,
        color: Color,
    ) -> Self {
        Self::new(
            id,
            bounds,
            Primitive::ui_text(text, color, size, Style::default()),
        )
    }
    pub fn ui_text_bold(
        id: u64,
        bounds: Rect,
        text: impl Into<String>,
        size: u16,
        color: Color,
    ) -> Self {
        Self::new(
            id,
            bounds,
            Primitive::ui_text(text, color, size, true.into()),
        )
    }
    /// UI text in any [`Style`]: weight, italic and language.
    pub fn ui_text_styled(
        id: u64,
        bounds: Rect,
        text: impl Into<String>,
        size: u16,
        color: Color,
        style: Style,
    ) -> Self {
        Self::new(id, bounds, Primitive::ui_text(text, color, size, style))
    }
    pub fn rounded_rectangle(id: u64, bounds: Rect, fill: Color, radius: u32) -> Self {
        Self::new(
            id,
            bounds,
            Primitive::RoundedBox {
                fill,
                border: None,
                border_width: 0,
                radius,
            },
        )
    }
    pub fn rectangle(id: u64, bounds: Rect, fill: Color) -> Self {
        Self::new(
            id,
            bounds,
            Primitive::Box {
                fill,
                border: None,
                border_width: 0,
            },
        )
    }
    pub fn interactive(
        mut self,
        action: impl Into<String>,
        role: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        self.interaction = Some(action.into());
        self.semantic = Some(Semantic {
            role: role.into(),
            label: label.into(),
            focusable: true,
            ..Semantic::default()
        });
        self
    }
    /// Can receive pointer input: addressable and not announced as disabled.
    pub fn accepts_input(&self) -> bool {
        self.interaction.is_some() && !self.semantic.as_ref().is_some_and(|s| s.disabled)
    }
    /// Geometric containment only: clips, the inverse transform and rounded corners.
    pub fn covers(&self, x: i32, y: i32) -> bool {
        self.clip.is_none_or(|c| c.contains(x, y))
            && self
                .rounded_clip
                .is_none_or(|c| rounded_contains(c.rect, c.radius, x, y))
            && self.transform.inverse_point(x, y).is_some_and(|(x, y)| {
                self.bounds.contains(x, y)
                    && match self.primitive {
                        Primitive::RoundedBox { radius, .. } => {
                            rounded_contains(self.bounds, radius, x, y)
                        }
                        _ => true,
                    }
            })
    }
    pub fn painted_bounds(&self) -> Rect {
        let bounds = self.transform.bounds(self.bounds);
        self.clip
            .and_then(|c| bounds.intersection(c))
            .unwrap_or_else(|| {
                if self.clip.is_some() {
                    Rect::default()
                } else {
                    bounds
                }
            })
    }
}
/// Identity, geometry and stacking of one window, published so spatial reasoning does
/// not have to be reconstructed from painted text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneWindow {
    pub id: u64,
    pub title: String,
    /// Application kind: `browser`, `files`, `editor`, `terminal`, `custom`, or an app id.
    pub app: String,
    /// Outer frame in scene coordinates, including platform decoration.
    pub bounds: Rect,
    /// Client area, excluding decoration and any browser chrome.
    pub content: Rect,
    /// Stacking order among this scene's windows; 0 is bottom-most.
    pub z: u32,
    pub focused: bool,
    pub minimized: bool,
    pub maximized: bool,
    /// Path, URL or document the window presents; empty when none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub document: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<String>,
    #[serde(default)]
    pub active_tab: usize,
    /// Windows above this one that cover part of `bounds`, bottom-to-top.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occluded_by: Vec<u64>,
    /// Largest part of `bounds` no higher window covers. `None` when fully hidden or
    /// minimized: there is then no point at which this window can be clicked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposed: Option<Rect>,
}
/// Caret geometry. A synthetic world knows exactly where the insertion point is; no
/// consumer should have to find a solid block in the pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caret {
    /// Insertion point in scene coordinates: a bar or a block cell, never empty.
    pub bounds: Rect,
    /// 0-based logical line and character column within the focused text.
    pub line: u32,
    pub column: u32,
    /// 0-based character offset into the focused text.
    pub offset: u32,
}
/// Where the next keystroke is delivered. `route` names the dispatch arm, so an agent
/// can tell "this goes into the address bar" from "this goes into the document".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keyboard {
    /// `none`, `panel`, `application`, `address`, `page`, `terminal`, `editor` or `window`.
    pub route: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    /// Interaction id the keystroke reaches, when the target is an addressable control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Keystrokes insert text at the caret rather than invoking commands.
    pub text_entry: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Focus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    /// Scene node holding focus, when focus lands on a painted control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<u64>,
    /// Interaction id of the focused control: the same string pointer actions address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caret: Option<Caret>,
    pub keyboard: Keyboard,
}
/// A text pane's whole buffer, not only the part the scene paints. Terminals keep
/// scrollback here; the visible window is `first_visible..first_visible + visible`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextBuffer {
    /// Handle referenced by `TextLine::pane`.
    pub handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    /// `terminal`, `editor`, ...
    pub kind: String,
    /// Every logical line, unwrapped. `TextLine::logical` indexes this vector.
    pub lines: Vec<String>,
    /// First logical line any painted fragment comes from.
    pub first_visible: u32,
    /// Count of logical lines the scene paints at least part of.
    pub visible: u32,
    /// Older lines were dropped to bound the scene. Panes this large are pathological
    /// here; read the whole thing with `filesystem.v1` instead.
    #[serde(default)]
    pub truncated: bool,
}
/// A pane whose content is taller than the part it shows, and how far it is scrolled.
/// Published so an actor can see that more is there, how much, and where the view is,
/// without inferring it from a painted scroll bar. `pointer.v1 wheel` over `bounds`
/// (or, on a phone, a vertical swipe that starts there) moves `offset`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrollArea {
    /// `pane:<name>` inside an application; the compositor namespaces it the way it
    /// namespaces interactions (`window:<id>:content:pane:<name>`).
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    /// The viewport, in scene coordinates.
    pub bounds: Rect,
    /// Pixels of content scrolled above the viewport's top edge, `0..=max_offset()`.
    pub offset: i32,
    /// Height of everything the pane holds, shown or not.
    pub extent: u32,
    /// A large title at the top of the content that collapses into the navigation bar
    /// once it is scrolled away (iOS). `None` for panes without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Height of that large title's band: the offset at which it has collapsed.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub title_height: u32,
}
fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}
impl ScrollArea {
    /// The furthest the content can be scrolled.
    pub fn max_offset(&self) -> i32 {
        self.extent.saturating_sub(self.bounds.height) as i32
    }
    /// The large title has scrolled out of the content into the navigation bar.
    pub fn title_collapsed(&self) -> bool {
        self.title.is_some() && self.offset >= self.title_height as i32
    }
}
/// Published scrollback bound: lines, then characters. A scene is a perception payload,
/// not a file transfer.
pub const MAX_BUFFER_LINES: usize = 4096;
pub const MAX_BUFFER_CHARS: usize = 262_144;
impl TextBuffer {
    /// Keep the tail within both bounds, marking the buffer when anything was dropped.
    pub fn bound(mut self) -> Self {
        let mut keep = self.lines.len().min(MAX_BUFFER_LINES);
        let mut chars = 0;
        for (taken, line) in self.lines.iter().rev().take(keep).enumerate() {
            chars += line.len() + 1;
            if chars > MAX_BUFFER_CHARS {
                keep = taken;
                break;
            }
        }
        self.truncated = keep < self.lines.len();
        self.lines.drain(..self.lines.len() - keep);
        self
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    /// Producer-assigned frame counter. Use `digest` for "did anything change".
    pub revision: u64,
    #[serde(default = "white")]
    pub background: Color,
    /// Bundled UI font family used by `UiText`/`UiTextBold`; never a host font.
    #[serde(default, skip_serializing_if = "Typeface::is_default")]
    pub typeface: Typeface,
    pub nodes: Vec<Node>,
    /// Window identity, geometry and stacking. Empty for scenes with no compositor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<SceneWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<Focus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buffers: Vec<TextBuffer>,
    /// Scrollable panes and where each is scrolled to, topmost last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scrolls: Vec<ScrollArea>,
    /// Content digest of the whole scene, filled by `Scene::stamp`. Equal digests mean
    /// nothing changed; 0 means unstamped.
    #[serde(default, skip_serializing_if = "is_unstamped")]
    pub digest: u64,
}
fn white() -> Color {
    Color::WHITE
}
impl Default for Scene {
    fn default() -> Self {
        Self::new(800, 600)
    }
}
impl Scene {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            revision: 0,
            background: Color::WHITE,
            typeface: Typeface::default(),
            nodes: Vec::new(),
            windows: Vec::new(),
            focus: None,
            buffers: Vec::new(),
            scrolls: Vec::new(),
            digest: 0,
        }
    }
    pub fn window(&self, id: u64) -> Option<&SceneWindow> {
        self.windows.iter().find(|w| w.id == id)
    }
    pub fn buffer(&self, handle: &str) -> Option<&TextBuffer> {
        self.buffers.iter().find(|b| b.handle == handle)
    }
    /// Highest z wins, later insertion wins ties. Disabled controls cannot receive input.
    pub fn hit_test(&self, x: i32, y: i32) -> Option<&Node> {
        if !Rect::new(0, 0, self.width, self.height).contains(x, y) {
            return None;
        }
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.accepts_input() && n.covers(x, y))
            .max_by_key(|(i, n)| (n.z, *i))
            .map(|(_, n)| n)
    }
    /// Scroll areas of window `window` (any window for `None`) under `(x, y)`,
    /// innermost first: the order a wheel turn tries them in.
    pub fn scrolls_at(&self, window: Option<u64>, x: i32, y: i32) -> Vec<&ScrollArea> {
        let mut areas: Vec<(usize, &ScrollArea)> = self
            .scrolls
            .iter()
            .enumerate()
            .filter(|(_, a)| window.is_none() || a.window == window)
            .filter(|(_, a)| a.bounds.contains(x, y))
            .collect();
        areas.sort_by_key(|(i, a)| (a.bounds.area(), std::cmp::Reverse(*i)));
        areas.into_iter().map(|(_, a)| a).collect()
    }
    /// Every node covering `(x, y)`, topmost first: the `elementFromPoint` stack.
    /// Includes non-interactive nodes so occlusion is legible, not just answerable.
    pub fn hit_stack(&self, x: i32, y: i32) -> Vec<Hit> {
        if !Rect::new(0, 0, self.width, self.height).contains(x, y) {
            return Vec::new();
        }
        let mut hits: Vec<Hit> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.covers(x, y))
            .map(|(order, n)| Hit {
                node: n.id,
                z: n.z,
                order,
                interaction: n.interaction.clone(),
                window: n.window,
                role: n
                    .semantic
                    .as_ref()
                    .map(|s| s.role.clone())
                    .unwrap_or_default(),
                label: n
                    .semantic
                    .as_ref()
                    .map(|s| s.label.clone())
                    .unwrap_or_default(),
                interactive: n.accepts_input(),
                opaque: n.is_opaque(),
            })
            .collect();
        hits.sort_by_key(|h| std::cmp::Reverse((h.z, h.order)));
        hits
    }
    /// Would a click at `(x, y)` actually reach `node`? The question overlapping
    /// windows used to force consumers to guess at.
    pub fn hit_reaches(&self, node: u64, x: i32, y: i32) -> bool {
        self.hit_test(x, y).is_some_and(|n| n.id == node)
    }
    /// A point at which a click reaches `node`, or `None` when it is fully occluded,
    /// disabled or not interactive. Sampling is a fixed grid, so it is deterministic.
    pub fn reachable_point(&self, node: u64) -> Option<(i32, i32)> {
        let n = self.nodes.iter().find(|n| n.id == node)?;
        let b = n.painted_bounds();
        if b.area() == 0 {
            return None;
        }
        // Centre first (the usual answer), then a 7x7 interior grid for controls the
        // topmost window only partly covers.
        let sample = |i: u32, d: u32, origin: i32, extent: u32| {
            origin + ((u64::from(extent) * u64::from(2 * i + 1)) / u64::from(2 * d)) as i32
        };
        std::iter::once((b.x + b.width as i32 / 2, b.y + b.height as i32 / 2))
            .chain((0..7).flat_map(move |j| {
                (0..7).map(move |i| (sample(i, 7, b.x, b.width), sample(j, 7, b.y, b.height)))
            }))
            .find(|&(x, y)| self.hit_reaches(node, x, y))
    }
    /// Accessibility-shaped view: one entry per interactive control, merging the several
    /// nodes a shell paints for it. Ordered bottom-to-top in paint order.
    pub fn accessibility(&self) -> Vec<AxNode> {
        let focused = self.focus.as_ref().and_then(|f| f.interaction.clone());
        let mut order: Vec<String> = Vec::new();
        let mut merged: BTreeMap<String, AxNode> = BTreeMap::new();
        for (index, n) in self.nodes.iter().enumerate() {
            let Some(action) = &n.interaction else {
                continue;
            };
            let entry = merged.entry(action.clone()).or_insert_with(|| {
                order.push(action.clone());
                AxNode {
                    id: action.clone(),
                    enabled: true,
                    window: n.window.or_else(|| window_of(action)),
                    focused: focused.as_deref() == Some(action.as_str()),
                    z: n.z,
                    order: index,
                    ..AxNode::default()
                }
            });
            entry.nodes.push(n.id);
            entry.bounds = entry.bounds.union(n.painted_bounds());
            entry.z = entry.z.max(n.z);
            entry.revision = mix(entry.revision, n.revision);
            if let Some(s) = &n.semantic {
                if entry.role.is_empty() {
                    entry.role = s.role.clone();
                }
                if entry.name.is_empty() {
                    entry.name = s.label.clone();
                }
                if entry.value.is_none() {
                    entry.value.clone_from(&s.value);
                }
                entry.enabled &= !s.disabled;
                entry.focusable |= s.focusable;
            }
            if let Some(state) = &n.state {
                entry.checked = entry.checked.or(state.checked);
                entry.selected = entry.selected.or(state.selected);
                entry.expanded = entry.expanded.or(state.expanded);
                entry.focused |= state.focused;
            }
        }
        let mut out: Vec<AxNode> = order
            .into_iter()
            .filter_map(|k| merged.remove(&k))
            .collect();
        for ax in &mut out {
            ax.hit = self
                .reachable_point(*ax.nodes.first().unwrap_or(&0))
                .or_else(|| ax.nodes.iter().find_map(|id| self.reachable_point(*id)));
        }
        out.sort_by_key(|ax| (ax.z, ax.order));
        out
    }
    /// Fill every content revision and the scene digest. Revisions are digests of what a
    /// node paints and announces, so identical state yields identical ids after a
    /// snapshot restore, a fork or a process restart: never a mutable counter.
    pub fn stamp(&mut self) {
        for n in &mut self.nodes {
            // `revision` is skipped when zero, so the digest is over content alone.
            n.revision = 0;
        }
        for i in 0..self.nodes.len() {
            self.nodes[i].revision = node_digest(&self.nodes[i]);
        }
        self.digest = 0;
        self.digest = digest(&(
            self.width,
            self.height,
            self.background,
            &self.typeface,
            &self.windows,
            &self.focus,
            &self.buffers,
            self.nodes
                .iter()
                .map(|n| (n.id, n.revision))
                .collect::<Vec<_>>(),
        ));
        // Folded in only when present, so a scene without panes keeps the digest it
        // always had.
        if !self.scrolls.is_empty() {
            self.digest = digest(&(self.digest, &self.scrolls));
        }
    }
    /// What changed since `previous`. Both scenes must be stamped; an unstamped scene
    /// reports everything as changed rather than silently reporting nothing.
    pub fn diff(&self, previous: &Self) -> SceneDelta {
        let mut delta = SceneDelta {
            resized: self.width != previous.width || self.height != previous.height,
            background: self.background != previous.background,
            focus: self.focus != previous.focus,
            ..SceneDelta::default()
        };
        if self.digest != 0 && self.digest == previous.digest {
            return delta;
        }
        let before: BTreeMap<u64, &Node> = previous.nodes.iter().map(|n| (n.id, n)).collect();
        let mut seen = BTreeSet::new();
        let mut damage = Vec::new();
        for n in &self.nodes {
            seen.insert(n.id);
            match before.get(&n.id) {
                None => {
                    delta.added.push(n.id);
                    damage.push(n.painted_bounds());
                }
                Some(old) if old.revision != n.revision || n.revision == 0 => {
                    delta.updated.push(n.id);
                    damage.push(old.painted_bounds());
                    damage.push(n.painted_bounds());
                }
                Some(_) => {}
            }
        }
        for n in &previous.nodes {
            if !seen.contains(&n.id) {
                delta.removed.push(n.id);
                damage.push(n.painted_bounds());
            }
        }
        let windows: BTreeMap<u64, &SceneWindow> =
            previous.windows.iter().map(|w| (w.id, w)).collect();
        for w in &self.windows {
            if windows.get(&w.id).is_none_or(|old| *old != w) {
                delta.windows.push(w.id);
            }
        }
        for w in &previous.windows {
            if !self.windows.iter().any(|n| n.id == w.id) {
                delta.windows.push(w.id);
            }
        }
        if delta.resized || delta.background {
            damage.push(Rect::new(0, 0, self.width, self.height));
        }
        delta.changed = delta.resized
            || delta.background
            || delta.focus
            || !delta.added.is_empty()
            || !delta.removed.is_empty()
            || !delta.updated.is_empty()
            || !delta.windows.is_empty();
        delta.damage = Damage { rects: damage };
        delta
    }
    pub fn ordered_nodes(&self) -> Vec<&Node> {
        let mut nodes: Vec<_> = self.nodes.iter().collect();
        nodes.sort_by_key(|n| n.z);
        nodes
    }
    /// Atomic patch application. Revision mismatch/duplicate IDs leaves the scene unchanged.
    pub fn patch(&mut self, patch: ScenePatch) -> Result<Damage, SceneError> {
        if patch.base_revision != self.revision {
            return Err(SceneError::Revision);
        }
        if patch.revision <= patch.base_revision {
            return Err(SceneError::Revision);
        }
        let mut next = self.clone();
        let mut rects = Vec::new();
        for op in patch.operations {
            match op {
                PatchOp::Upsert(node) => {
                    if let Some(old) = next.nodes.iter_mut().find(|n| n.id == node.id) {
                        rects.push(old.painted_bounds());
                        rects.push(node.painted_bounds());
                        *old = node;
                    } else {
                        rects.push(node.painted_bounds());
                        next.nodes.push(node);
                    }
                }
                PatchOp::Remove(id) => {
                    if let Some(i) = next.nodes.iter().position(|n| n.id == id) {
                        rects.push(next.nodes.remove(i).painted_bounds());
                    }
                }
                PatchOp::Background(color) => {
                    next.background = color;
                    rects.push(Rect::new(0, 0, next.width, next.height));
                }
            }
        }
        next.validate()?;
        next.revision = patch.revision;
        *self = next;
        Ok(Damage { rects })
    }
    pub fn validate(&self) -> Result<(), SceneError> {
        if self.width as u64 * self.height as u64 > MAX_PIXELS
            || self.width > i32::MAX as u32
            || self.height > i32::MAX as u32
        {
            return Err(SceneError::Dimensions);
        }
        let mut ids = std::collections::BTreeSet::new();
        for n in &self.nodes {
            if !ids.insert(n.id) {
                return Err(SceneError::DuplicateId(n.id));
            }
            if let Primitive::Image {
                width,
                height,
                rgba,
            } = &n.primitive
            {
                if (*width as u64)
                    .checked_mul(*height as u64)
                    .and_then(|n| n.checked_mul(4))
                    != Some(rgba.len() as u64)
                {
                    return Err(SceneError::ImageSize);
                }
            }
            if let Primitive::Path { points, .. } = &n.primitive {
                if points
                    .iter()
                    .any(|(x, y)| x.unsigned_abs() > 16_777_216 || y.unsigned_abs() > 16_777_216)
                {
                    return Err(SceneError::PathCoordinates);
                }
            }
        }
        Ok(())
    }
}
/// Pixel-centre rounded rectangle containment, with integer-only geometry.
/// Used by hit testing; the compositor additionally samples edge coverage.
pub fn rounded_contains(bounds: Rect, radius: u32, x: i32, y: i32) -> bool {
    if !bounds.contains(x, y) {
        return false;
    }
    let r = radius.min(bounds.width / 2).min(bounds.height / 2) as i128 * 2;
    let x = (x as i128 - bounds.x as i128) * 2 + 1;
    let y = (y as i128 - bounds.y as i128) * 2 + 1;
    let w = bounds.width as i128 * 2;
    let h = bounds.height as i128 * 2;
    let dx = (r - x).max(x - (w - r)).max(0);
    let dy = (r - y).max(y - (h - r)).max(0);
    dx * dx + dy * dy <= r * r
}
/// One entry of the `elementFromPoint` stack at a scene coordinate.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    pub node: u64,
    pub z: i32,
    /// Index in `Scene::nodes`; later indices paint over earlier ones at equal `z`.
    pub order: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// A click here would be delivered to this node if nothing above it takes it.
    pub interactive: bool,
    /// Hides everything below it at this point.
    pub opaque: bool,
}
/// One control, merging every node a shell paints for it. `id` is the interaction id,
/// which is also the string pointer and keyboard actions address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxNode {
    pub id: String,
    pub role: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub enabled: bool,
    pub focusable: bool,
    pub focused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    /// Union of the painted bounds of every merged node.
    pub bounds: Rect,
    pub nodes: Vec<u64>,
    pub z: i32,
    /// Index of the first merged node in `Scene::nodes`.
    pub order: usize,
    /// A point a click reaches this control at; `None` when occluded or inert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit: Option<(i32, i32)>,
    /// Combined content revision of the merged nodes; unchanged means unchanged.
    #[serde(default, skip_serializing_if = "is_unstamped")]
    pub revision: u64,
}
/// What changed between two stamped scenes. `changed == false` is the cheap
/// "nothing happened" answer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneDelta {
    pub changed: bool,
    pub added: Vec<u64>,
    pub removed: Vec<u64>,
    pub updated: Vec<u64>,
    pub windows: Vec<u64>,
    pub background: bool,
    pub resized: bool,
    pub focus: bool,
    pub damage: Damage,
}
/// Owning window of a namespaced interaction id (`window:<id>:...`).
pub fn window_of(interaction: &str) -> Option<u64> {
    interaction
        .strip_prefix("window:")?
        .split_once(':')
        .and_then(|(id, _)| id.parse().ok())
}
/// Content hash over the canonical JSON encoding of a contract. Pure integer mixing on
/// whole words: no `DefaultHasher`, whose output is not stable across Rust versions, and
/// no host state, so the same content hashes the same everywhere and forever.
struct Digest {
    state: u64,
    word: [u8; 8],
    len: usize,
}
const K: u64 = 0x517c_c1b7_2722_0a95;
impl Digest {
    fn new() -> Self {
        Self {
            state: 0xcbf2_9ce4_8422_2325,
            word: [0; 8],
            len: 0,
        }
    }
    fn mix(&mut self, word: u64) {
        self.state = (self.state ^ word).wrapping_mul(K).rotate_left(31);
    }
    /// Never 0, which is reserved for "unstamped".
    fn finish(mut self) -> u64 {
        let tail = self.len;
        self.word[tail..].fill(0);
        let word = u64::from_le_bytes(self.word);
        self.mix(word);
        self.mix(tail as u64);
        (self.state ^ (self.state >> 29)).wrapping_mul(K) | 1
    }
}
impl std::io::Write for Digest {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Word boundaries follow the byte position, not the chunking, so a value hashes
        // the same however the serializer happens to split it.
        let mut rest = buf;
        if self.len > 0 {
            let take = rest.len().min(8 - self.len);
            self.word[self.len..self.len + take].copy_from_slice(&rest[..take]);
            self.len += take;
            rest = &rest[take..];
            if self.len < 8 {
                return Ok(buf.len()); // still short of a word; keep what we have
            }
            let word = u64::from_le_bytes(self.word);
            self.mix(word);
            self.len = 0;
        }
        let (words, tail) = rest.as_chunks::<8>();
        for word in words {
            self.mix(u64::from_le_bytes(*word));
        }
        self.word[..tail.len()].copy_from_slice(tail);
        self.len = tail.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn mix(a: u64, b: u64) -> u64 {
    ((a ^ b).wrapping_mul(K)) | 1
}
/// Content digest. Never 0, which is reserved for "unstamped".
/// A node's content digest. Pixel buffers are hashed as raw bytes rather than through
/// their JSON form: a view's worth of RGBA is megabytes, and spelling every byte out as
/// decimal text first made stamping a scene with one picture cost more than drawing it.
fn node_digest(n: &Node) -> u64 {
    let Primitive::Image {
        width,
        height,
        rgba,
    } = &n.primitive
    else {
        return digest(n);
    };
    let mut hasher = Digest::new();
    let mut shell = n.clone();
    shell.primitive = Primitive::Image {
        width: *width,
        height: *height,
        rgba: Vec::new(),
    };
    let _ = serde_json::to_writer(&mut hasher, &shell);
    let _ = std::io::Write::write_all(&mut hasher, rgba);
    hasher.finish()
}
pub fn digest(value: &impl Serialize) -> u64 {
    let mut hasher = Digest::new();
    // Serialization of these contracts cannot fail; a failure would only shorten the
    // input, which still yields a deterministic id.
    let _ = serde_json::to_writer(&mut hasher, value);
    hasher.finish()
}
/// Largest part of `bounds` that none of `covers` hides, with ties broken
/// top-left-first so the answer is deterministic. `covers` is truncated to keep the
/// rectangle subtraction from fragmenting without bound.
pub fn exposed(bounds: Rect, covers: &[Rect]) -> Option<Rect> {
    let mut parts = vec![bounds];
    for cover in covers.iter().take(32) {
        parts = parts
            .into_iter()
            .flat_map(|p| p.subtract(*cover))
            .filter(|p| p.area() > 0)
            .collect();
        if parts.is_empty() {
            return None;
        }
    }
    parts
        .into_iter()
        .max_by_key(|r| (r.area(), -i64::from(r.y), -i64::from(r.x)))
}
/// Re-attach painted visual lines to the logical lines they came from. A pane that
/// hard-wraps mid-word yields `continuation: true` fragments, so joining them cannot
/// turn `initial commit` into `initial commi t`. `visual` may start part-way into the
/// buffer, as a scrolled terminal does. Returns the first alignment, in buffer order,
/// that attaches every fragment; empty when the painted text is not in the buffer.
pub fn reflow(logical: &[&str], visual: &[&str]) -> Vec<TextLine> {
    if visual.is_empty() {
        return Vec::new();
    }
    let buffer: Vec<Vec<char>> = logical.iter().map(|l| l.chars().collect()).collect();
    let head: Vec<char> = visual[0].chars().collect();
    for (line, chars) in buffer.iter().enumerate() {
        for offset in 0..=chars.len() {
            // Prefilter on the first fragment: alignment is otherwise quadratic in a
            // long scrollback.
            if chars.len() < offset + head.len() || chars[offset..offset + head.len()] != head[..] {
                continue;
            }
            if let Some(lines) = attach(&buffer, visual, line, offset) {
                return lines;
            }
        }
    }
    Vec::new()
}
fn attach(
    buffer: &[Vec<char>],
    visual: &[&str],
    line: usize,
    offset: usize,
) -> Option<Vec<TextLine>> {
    let (mut line, mut offset) = (line, offset);
    let mut out = Vec::with_capacity(visual.len());
    for fragment in visual {
        let chars = buffer.get(line)?;
        let width = fragment.chars().count();
        // An empty fragment is a blank logical line, never a zero-width slice of a
        // non-empty one.
        if (width == 0 && !chars.is_empty()) || offset + width > chars.len() {
            return None;
        }
        if !chars[offset..offset + width]
            .iter()
            .copied()
            .eq(fragment.chars())
        {
            return None;
        }
        out.push(TextLine {
            logical: line as u32,
            continuation: offset > 0,
            offset: offset as u32,
            ..TextLine::default()
        });
        offset += width;
        if offset >= chars.len() {
            line += 1;
            offset = 0;
        }
    }
    Some(out)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenePatch {
    pub base_revision: u64,
    pub revision: u64,
    pub operations: Vec<PatchOp>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "value", rename_all = "snake_case")]
// Upserts dominate frame updates. Keeping nodes inline avoids a separate allocation
// and pointer chase for every patch operation in the renderer hot path.
#[allow(clippy::large_enum_variant)]
pub enum PatchOp {
    Upsert(Node),
    Remove(u64),
    Background(Color),
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Damage {
    pub rects: Vec<Rect>,
}
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SceneError {
    #[error("path coordinates exceed fixed-point raster range")]
    PathCoordinates,
    #[error("scene dimensions exceed raster bounds")]
    Dimensions,
    #[error("scene revision mismatch")]
    Revision,
    #[error("duplicate node id {0}")]
    DuplicateId(u64),
    #[error("image byte length mismatch")]
    ImageSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    Row,
    Column,
}
/// Predictable flow layout: explicit measured extents; no DOM or CSS dependency.
pub fn flow_layout(
    bounds: Rect,
    axis: Axis,
    gap: u32,
    padding: u32,
    sizes: &[(u32, u32)],
) -> Vec<Rect> {
    let mut x = bounds.x.saturating_add(padding.min(i32::MAX as u32) as i32);
    let mut y = bounds.y.saturating_add(padding.min(i32::MAX as u32) as i32);
    sizes
        .iter()
        .map(|&(w, h)| {
            let r = Rect::new(x, y, w, h);
            match axis {
                Axis::Row => {
                    x = x.saturating_add(w.saturating_add(gap).min(i32::MAX as u32) as i32)
                }
                Axis::Column => {
                    y = y.saturating_add(h.saturating_add(gap).min(i32::MAX as u32) as i32)
                }
            }
            r
        })
        .collect()
}
/// `wrap_text` with provenance, so a consumer that joins the result cannot corrupt it.
pub fn wrap_text_lines(text: &str, max_columns: usize) -> Vec<(String, TextLine)> {
    let mut out = Vec::new();
    for (logical, line) in text.split('\n').enumerate() {
        let mut offset = 0;
        for (index, range) in text::terminal::wrap(line, max_columns)
            .into_iter()
            .enumerate()
        {
            let chunk = &line[range];
            out.push((
                chunk.to_owned(),
                TextLine {
                    logical: logical as u32,
                    continuation: index > 0,
                    offset,
                    ..TextLine::default()
                },
            ));
            offset += chunk.chars().count() as u32;
        }
    }
    out
}
/// Fixed-cell text wrapping shared by layout and rasterization, preserving newlines.
/// Rows hold `max_columns` cells: wide characters (CJK, emoji) take two, combining
/// marks none (see [`text::terminal`]).
pub fn wrap_text(text: &str, max_columns: usize) -> Vec<String> {
    text.split('\n')
        .flat_map(|line| {
            text::terminal::wrap(line, max_columns)
                .into_iter()
                .map(move |range| line[range].to_owned())
        })
        .collect()
}
pub fn text_cell(size: u16) -> (u32, u32) {
    let size = u32::from(size.max(1));
    ((size * 3).div_ceil(5), size + size.div_ceil(4))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inverse_clip_z() {
        let mut s = Scene::new(100, 100);
        s.nodes.push(
            Node::rectangle(1, Rect::new(0, 0, 40, 40), Color::BLACK)
                .interactive("a", "button", "a"),
        );
        let mut n = Node::rectangle(2, Rect::new(0, 0, 40, 40), Color::BLACK)
            .interactive("b", "button", "b");
        n.transform = Transform::translate(20, 20);
        n.clip = Some(Rect::new(20, 20, 10, 10));
        s.nodes.push(n);
        assert_eq!(s.hit_test(25, 25).unwrap().id, 2);
        assert_eq!(s.hit_test(35, 35).unwrap().id, 1);
        assert!(s.hit_test(90, 90).is_none());
    }
    #[test]
    fn rounded_hit_test_omits_cutaway_corners() {
        let mut s = Scene::new(100, 100);
        let mut n = Node::rounded_rectangle(1, Rect::new(0, 0, 40, 40), Color::BLACK, 12)
            .interactive("round", "button", "Round");
        n.transform = Transform::translate(10, 10);
        s.nodes.push(n);
        assert!(s.hit_test(10, 10).is_none());
        assert_eq!(s.hit_test(30, 10).unwrap().id, 1);
        assert_eq!(s.hit_test(30, 30).unwrap().id, 1);
        assert!(s.hit_test(49, 49).is_none());
    }
    #[test]
    fn patch_atomic() {
        let mut s = Scene::default();
        let old = s.clone();
        assert!(s
            .patch(ScenePatch {
                base_revision: 2,
                revision: 3,
                operations: vec![]
            })
            .is_err());
        assert_eq!(s, old);
    }
    #[test]
    fn rows_wrap() {
        assert_eq!(wrap_text("abcde\n", 3), vec!["abc", "de", ""]);
        assert_eq!(
            flow_layout(
                Rect::new(0, 0, 100, 100),
                Axis::Column,
                2,
                4,
                &[(10, 10), (20, 20)]
            )[1]
            .y,
            16
        );
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn translated_points_round_trip(x in -100000i32..100000,y in -100000i32..100000,tx in -10000i32..10000,ty in -10000i32..10000) {
            let t=Transform::translate(tx,ty);let p=t.point(x,y);
            prop_assert_eq!(t.inverse_point(p.0,p.1),Some((x,y)));
        }
        #[test]
        fn intersection_is_commutative(x in -100i32..100,y in -100i32..100,w in 0u32..200,h in 0u32..200) {
            let a=Rect::new(x,y,w,h);let b=Rect::new(0,0,75,90);
            prop_assert_eq!(a.intersection(b),b.intersection(a));
        }
    }
    #[test]
    fn hostile_image_size_returns_error_without_overflow() {
        let mut s = Scene::new(1, 1);
        s.nodes.push(Node::new(
            1,
            Rect::new(0, 0, 1, 1),
            Primitive::Image {
                width: u32::MAX,
                height: u32::MAX,
                rgba: vec![],
            },
        ));
        assert_eq!(s.validate(), Err(SceneError::ImageSize));
    }
}

#[cfg(test)]
mod desktop_contract_tests {
    use super::*;
    #[test]
    fn asset_shadow_and_bold_contracts_round_trip_without_bitmap_payloads() {
        let mut scene = Scene::new(400, 300);
        scene
            .nodes
            .push(Node::asset(1, Rect::new(0, 0, 400, 300), "wallpaper/macos"));
        scene.nodes.push(Node::new(
            2,
            Rect::new(20, 20, 300, 200),
            Primitive::Shadow {
                color: Color(0, 0, 0, 90),
                radius: 12,
                blur: 18,
            },
        ));
        scene.nodes.push(Node::ui_text_bold(
            3,
            Rect::new(40, 40, 100, 24),
            "Finder",
            14,
            Color::BLACK,
        ));
        let json = serde_json::to_string(&scene).unwrap();
        assert!(json.len() < 1500);
        assert!(!json.contains("rgba"));
        assert_eq!(scene, serde_json::from_str(&json).unwrap());
        scene.validate().unwrap();
    }
}

#[cfg(test)]
mod perception_tests {
    use super::*;
    fn control(id: u64, bounds: Rect, action: &str, label: &str) -> Node {
        Node::rectangle(id, bounds, Color::BLACK).interactive(action, "button", label)
    }
    #[test]
    fn revisions_are_content_derived_not_counted() {
        let mut a = Scene::new(100, 100);
        a.nodes.push(control(1, Rect::new(0, 0, 10, 10), "a", "A"));
        a.nodes.push(control(2, Rect::new(20, 0, 10, 10), "b", "B"));
        a.stamp();
        // Round-tripping through a snapshot must reproduce the same ids.
        let mut restored: Scene =
            serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(restored, a);
        restored.stamp();
        assert_eq!(restored.digest, a.digest);
        assert!(!restored.diff(&a).changed);
        let mut b = a.clone();
        b.nodes[1].semantic.as_mut().unwrap().label = "B!".into();
        b.stamp();
        let delta = b.diff(&a);
        assert!(delta.changed);
        assert_eq!(delta.updated, vec![2]);
        assert!(delta.added.is_empty() && delta.removed.is_empty());
        assert_eq!(a.nodes[0].revision, b.nodes[0].revision);
        // A rebuilt-from-scratch scene with the same content hashes the same.
        let mut fresh = Scene::new(100, 100);
        fresh
            .nodes
            .push(control(1, Rect::new(0, 0, 10, 10), "a", "A"));
        fresh
            .nodes
            .push(control(2, Rect::new(20, 0, 10, 10), "b", "B"));
        fresh.stamp();
        assert_eq!(fresh.digest, a.digest);
    }
    #[test]
    fn digests_see_every_byte_at_every_word_boundary() {
        // The hasher buffers whole words, so a change in the last byte of an input of
        // any length must still move the digest.
        for n in 0..96usize {
            let base = "x".repeat(n);
            assert_ne!(
                digest(&format!("{base}a")),
                digest(&format!("{base}b")),
                "length {n}"
            );
            assert_ne!(digest(&base), digest(&format!("{base}x")), "length {n}");
        }
    }
    #[test]
    fn occlusion_is_legible_and_reachable_points_avoid_it() {
        let mut s = Scene::new(100, 100);
        s.nodes
            .push(control(1, Rect::new(0, 0, 40, 40), "under", "Under"));
        let mut cover = Node::rectangle(2, Rect::new(20, 0, 40, 40), Color::WHITE)
            .interactive("over", "button", "Over");
        cover.z = 10;
        s.nodes.push(cover);
        let stack = s.hit_stack(25, 5);
        assert_eq!(stack.len(), 2);
        assert_eq!(stack[0].node, 2);
        assert!(stack[0].opaque && stack[0].interactive);
        assert_eq!(stack[1].node, 1);
        assert!(!s.hit_reaches(1, 25, 5));
        let point = s.reachable_point(1).unwrap();
        assert!(point.0 < 20 && s.hit_reaches(1, point.0, point.1));
        // Fully covered controls report no reachable point at all.
        s.nodes[1].bounds = Rect::new(0, 0, 40, 40);
        assert_eq!(s.reachable_point(1), None);
        assert_eq!(
            exposed(Rect::new(0, 0, 40, 40), &[Rect::new(20, 0, 40, 40)]),
            Some(Rect::new(0, 0, 20, 40))
        );
        assert_eq!(
            exposed(Rect::new(0, 0, 40, 40), &[Rect::new(0, 0, 40, 40)]),
            None
        );
    }
    #[test]
    fn accessibility_merges_nodes_per_control_and_carries_state() {
        let mut s = Scene::new(200, 100);
        s.nodes.push(control(
            1,
            Rect::new(0, 0, 60, 20),
            "window:7:content:wifi",
            "Wi-Fi",
        ));
        let mut glyph = Node::ui_text(2, Rect::new(4, 4, 20, 12), "on", 11, Color::BLACK)
            .interactive("window:7:content:wifi", "switch", "Wi-Fi");
        glyph.state = Some(NodeState {
            checked: Some(true),
            ..NodeState::default()
        });
        s.nodes.push(glyph);
        s.nodes.push(control(
            3,
            Rect::new(80, 0, 40, 20),
            "window:7:close",
            "Close",
        ));
        s.focus = Some(Focus {
            interaction: Some("window:7:content:wifi".into()),
            ..Focus::default()
        });
        s.stamp();
        let ax = s.accessibility();
        assert_eq!(ax.len(), 2);
        let wifi = &ax[0];
        assert_eq!(wifi.id, "window:7:content:wifi");
        assert_eq!(wifi.window, Some(7));
        assert_eq!(wifi.nodes, vec![1, 2]);
        assert_eq!(wifi.bounds, Rect::new(0, 0, 60, 20));
        assert_eq!(wifi.checked, Some(true));
        assert!(wifi.enabled && wifi.focused && wifi.focusable);
        assert_eq!(wifi.role, "button");
        assert!(wifi.hit.is_some() && wifi.revision != 0);
        assert!(!ax[1].focused);
    }
    #[test]
    fn wrapped_lines_are_marked_so_joining_is_safe() {
        let logical = ["initial commit", "second line"];
        let visual = ["initial commi", "t", "second line"];
        let lines = reflow(&logical, &visual);
        assert!(!lines[0].continuation);
        assert_eq!(
            (lines[1].logical, lines[1].continuation, lines[1].offset),
            (0, true, 13)
        );
        assert_eq!((lines[2].logical, lines[2].continuation), (1, false));
        let mut joined = String::new();
        for (text, line) in visual.iter().zip(&lines) {
            if !line.continuation && !joined.is_empty() {
                joined.push('\n');
            }
            joined.push_str(text);
        }
        assert_eq!(joined, "initial commit\nsecond line");
        // A scrolled pane whose first fragment starts mid-line still aligns.
        assert_eq!(reflow(&logical, &["t", "second line"])[0].offset, 13);
        // Blank logical lines are kept distinct from wrapped ones.
        assert_eq!(reflow(&["a", "", "b"], &["a", "", "b"])[1].logical, 1);
        assert_eq!(
            wrap_text_lines("initial commit", 13)
                .iter()
                .map(|(t, l)| (t.as_str(), l.continuation))
                .collect::<Vec<_>>(),
            vec![("initial commi", false), ("t", true)]
        );
    }
    #[test]
    fn additive_fields_stay_out_of_legacy_payloads() {
        let mut s = Scene::new(40, 40);
        s.nodes.push(control(1, Rect::new(0, 0, 10, 10), "a", "A"));
        let json = serde_json::to_string(&s).unwrap();
        for absent in [
            "\"windows\"",
            "\"focus\"",
            "\"buffers\"",
            "\"digest\"",
            "\"state\"",
            "\"line\"",
        ] {
            assert!(
                !json.contains(absent),
                "{absent} leaked into an unstamped scene"
            );
        }
        // Only the pre-existing scene-level `revision`; nodes stay unstamped.
        assert_eq!(json.matches("\"revision\"").count(), 1);
        // Old payloads without the new fields still deserialize.
        let legacy = r#"{"width":4,"height":4,"revision":0,"background":[255,255,255,255],
            "nodes":[{"id":1,"bounds":{"x":0,"y":0,"width":1,"height":1},
            "primitive":{"kind":"region"}}]}"#;
        let scene: Scene = serde_json::from_str(legacy).unwrap();
        assert!(scene.windows.is_empty() && scene.focus.is_none() && scene.digest == 0);
    }
}
