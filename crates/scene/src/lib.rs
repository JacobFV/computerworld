//! Portable synthetic scene contracts. Geometry is integer pixels; affine coefficients
//! use 1/1024 units. This crate never rasterizes or consults host fonts.
use serde::{Deserialize, Serialize};

pub const SCENE_VERSION: u32 = 1;
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
    UiText {
        text: String,
        color: Color,
        size: u16,
    },
    UiTextBold {
        text: String,
        color: Color,
        size: u16,
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
    /// Invisible layout/interaction region.
    Region,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Semantic {
    pub role: String,
    pub label: String,
    pub value: Option<String>,
    pub disabled: bool,
    pub focusable: bool,
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
    #[serde(default)]
    pub transform: Transform,
    #[serde(default)]
    pub z: i32,
    #[serde(default = "opaque")]
    pub opacity: u8,
}
fn opaque() -> u8 {
    255
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
            transform: Transform::default(),
            z: 0,
            opacity: 255,
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
            Primitive::UiText {
                text: text.into(),
                color,
                size,
            },
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
            Primitive::UiTextBold {
                text: text.into(),
                color,
                size,
            },
        )
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    pub revision: u64,
    #[serde(default = "white")]
    pub background: Color,
    pub nodes: Vec<Node>,
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
            nodes: Vec::new(),
        }
    }
    /// Highest z wins, later insertion wins ties. Disabled controls cannot receive input.
    pub fn hit_test(&self, x: i32, y: i32) -> Option<&Node> {
        if !Rect::new(0, 0, self.width, self.height).contains(x, y) {
            return None;
        }
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                n.interaction.is_some()
                    && !n.semantic.as_ref().is_some_and(|s| s.disabled)
                    && n.clip.is_none_or(|c| c.contains(x, y))
                    && n.transform.inverse_point(x, y).is_some_and(|(x, y)| {
                        n.bounds.contains(x, y)
                            && match n.primitive {
                                Primitive::RoundedBox { radius, .. } => {
                                    rounded_contains(n.bounds, radius, x, y)
                                }
                                _ => true,
                            }
                    })
            })
            .max_by_key(|(i, n)| (n.z, *i))
            .map(|(_, n)| n)
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
/// Fixed-cell text wrapping shared by layout and rasterization, preserving newlines.
pub fn wrap_text(text: &str, max_columns: usize) -> Vec<String> {
    let max_columns = max_columns.max(1);
    let mut lines = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut out = String::new();
        let mut count = 0;
        for c in line.chars() {
            if count == max_columns {
                lines.push(std::mem::take(&mut out));
                count = 0
            }
            out.push(c);
            count += 1;
        }
        lines.push(out);
    }
    lines
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
