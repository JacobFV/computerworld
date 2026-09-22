//! A deterministic, layered raster editor: the engine every image application in the
//! simulator draws with. Windows Paint, Preview, Pixelmator Pro, GIMP, Pinta, the phone
//! photo editors and the sketching app are different interfaces over this one model.
//!
//! Pixels are straight (non-premultiplied) 8-bit RGBA. Every operation is integer or
//! built from correctly rounded IEEE basic arithmetic (see [`fmath`]), so a native build
//! and a Wasm build produce identical bytes for identical edits.
// Per-channel pixel loops index several parallel arrays; iterator chains obscure them.
#![allow(clippy::needless_range_loop)]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub mod adjust;
pub mod blend;
pub mod bmp;
pub mod codec;
pub mod document;
pub mod draw;
pub mod filter;
pub mod fmath;
pub mod gradient;
pub mod heal;
pub mod jpeg;
pub mod mask;
pub mod path;
pub mod transform;
pub mod xcf;
pub mod zlib;

pub use adjust::Adjustment;
pub use blend::BlendMode;
pub use document::{Document, Layer};
pub use draw::{Brush, BrushKind, Shape, ShapeKind};
pub use filter::Filter;
pub use mask::{Mask, SelectMode};
pub use transform::Resample;

/// One pixel: straight red, green, blue, alpha.
pub type Rgba = [u8; 4];

pub const TRANSPARENT: Rgba = [0, 0, 0, 0];
pub const WHITE: Rgba = [255, 255, 255, 255];
pub const BLACK: Rgba = [0, 0, 0, 255];

/// The longest edge any canvas may have, and the most pixels it may hold. A simulated
/// application must not be able to ask the host for gigabytes.
pub const MAX_SIDE: u32 = 8192;
pub const MAX_PIXELS: u64 = 16 << 20;

/// Parse `rrggbb` or `rrggbbaa` (an optional leading `#`).
pub fn parse_hex(text: &str) -> Option<Rgba> {
    let text = text.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(text.get(i..i + 2)?, 16).ok();
    match text.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}
/// `rrggbb`, or `rrggbbaa` when not opaque.
pub fn hex(c: Rgba) -> String {
    if c[3] == 255 {
        format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else {
        format!("{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }
}

/// Integer rectangle in canvas pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}
impl IRect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }
    /// The rectangle spanning two corners, inclusive of both.
    pub fn spanning(ax: i32, ay: i32, bx: i32, by: i32) -> Self {
        let (x0, x1) = (ax.min(bx), ax.max(bx));
        let (y0, y1) = (ay.min(by), ay.max(by));
        Self::new(x0, y0, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32)
    }
    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
    pub fn intersect(&self, o: &Self) -> Option<Self> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        (x1 > x0 && y1 > y0).then(|| Self::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }
    pub fn union(&self, o: &Self) -> Self {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        Self::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }
    /// Clip to a `width` x `height` canvas.
    pub fn clip(&self, width: u32, height: u32) -> Option<Self> {
        self.intersect(&Self::new(0, 0, width, height))
    }
    pub fn grow(&self, by: u32) -> Self {
        Self::new(
            self.x - by as i32,
            self.y - by as i32,
            self.w + 2 * by,
            self.h + 2 * by,
        )
    }
}

/// A rectangular grid of pixels.
#[derive(Clone, PartialEq, Eq)]
pub struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
impl std::fmt::Debug for Canvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Canvas({}x{}, #{:016x})",
            self.width,
            self.height,
            self.hash()
        )
    }
}
pub fn check_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("an image needs at least one pixel".into());
    }
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(format!(
            "{width} x {height} is larger than the {MAX_SIDE} pixel limit"
        ));
    }
    Ok(())
}
impl Canvas {
    /// A canvas of one colour. Sizes are clamped to at least one pixel.
    pub fn filled(width: u32, height: u32, color: Rgba) -> Self {
        let (width, height) = (width.clamp(1, MAX_SIDE), height.clamp(1, MAX_SIDE));
        Self {
            width,
            height,
            pixels: color.repeat((width * height) as usize),
        }
    }
    pub fn new(width: u32, height: u32) -> Self {
        Self::filled(width, height, TRANSPARENT)
    }
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, String> {
        check_size(width, height)?;
        if pixels.len() != width as usize * height as usize * 4 {
            return Err("pixel buffer does not match the image size".into());
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn bounds(&self) -> IRect {
        IRect::new(0, 0, self.width, self.height)
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }
    #[inline]
    fn index(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }
    /// The pixel at `(x, y)`; outside the canvas is transparent.
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> Rgba {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return TRANSPARENT;
        }
        let i = self.index(x as u32, y as u32);
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
    /// The pixel nearest `(x, y)`, clamping at the edges.
    #[inline]
    pub fn get_clamped(&self, x: i32, y: i32) -> Rgba {
        self.get(
            x.clamp(0, self.width as i32 - 1),
            y.clamp(0, self.height as i32 - 1),
        )
    }
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = self.index(x as u32, y as u32);
        self.pixels[i..i + 4].copy_from_slice(&c);
    }
    /// A copy of `r` (clipped to the canvas); `None` when nothing of it is inside.
    pub fn region(&self, r: IRect) -> Option<Canvas> {
        let r = r.clip(self.width, self.height)?;
        let mut out = Vec::with_capacity(r.w as usize * r.h as usize * 4);
        for y in r.y..r.bottom() {
            let start = self.index(r.x as u32, y as u32);
            out.extend_from_slice(&self.pixels[start..start + r.w as usize * 4]);
        }
        Some(Canvas {
            width: r.w,
            height: r.h,
            pixels: out,
        })
    }
    /// Replace pixels at `(x, y)` with `src`, clipped to this canvas.
    pub fn put(&mut self, x: i32, y: i32, src: &Canvas) {
        let Some(r) = IRect::new(x, y, src.width, src.height).clip(self.width, self.height) else {
            return;
        };
        for row in r.y..r.bottom() {
            let sy = (row - y) as u32;
            let sx = (r.x - x) as u32;
            let s = src.index(sx, sy);
            let d = self.index(r.x as u32, row as u32);
            let n = r.w as usize * 4;
            self.pixels[d..d + n].copy_from_slice(&src.pixels[s..s + n]);
        }
    }
    /// Composite `src` over this canvas at `(x, y)` with `mode` and `opacity` (0..=255).
    pub fn draw(&mut self, x: i32, y: i32, src: &Canvas, mode: BlendMode, opacity: u8) {
        let Some(r) = IRect::new(x, y, src.width, src.height).clip(self.width, self.height) else {
            return;
        };
        for row in r.y..r.bottom() {
            for col in r.x..r.right() {
                let s = src.get(col - x, row - y);
                let d = self.get(col, row);
                self.set(col, row, blend::composite(d, s, opacity, mode));
            }
        }
    }
    /// The smallest rectangle holding every pixel with any alpha, if one does.
    pub fn opaque_bounds(&self) -> Option<IRect> {
        let mut found: Option<IRect> = None;
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                if self.get(x, y)[3] != 0 {
                    let p = IRect::new(x, y, 1, 1);
                    found = Some(found.map_or(p, |f| f.union(&p)));
                }
            }
        }
        found
    }
    /// FNV-1a over dimensions and bytes: a stable fingerprint for tests and snapshots.
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |b: u8| {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        };
        for b in self
            .width
            .to_le_bytes()
            .into_iter()
            .chain(self.height.to_le_bytes())
        {
            eat(b);
        }
        for b in &self.pixels {
            eat(*b);
        }
        h
    }
}

#[derive(Serialize, Deserialize)]
struct Wire {
    width: u32,
    height: u32,
    data: String,
}
impl Serialize for Canvas {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Wire {
            width: self.width,
            height: self.height,
            data: codec::encode(&self.pixels, 4),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for Canvas {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(d)?;
        check_size(wire.width, wire.height).map_err(serde::de::Error::custom)?;
        let expected = wire.width as usize * wire.height as usize * 4;
        let pixels = codec::decode(&wire.data, 4, expected).map_err(serde::de::Error::custom)?;
        Ok(Self {
            width: wire.width,
            height: wire.height,
            pixels,
        })
    }
}
impl Serialize for Mask {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Wire {
            width: self.width(),
            height: self.height(),
            data: codec::encode(self.data(), 1),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for Mask {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(d)?;
        check_size(wire.width, wire.height).map_err(serde::de::Error::custom)?;
        let expected = wire.width as usize * wire.height as usize;
        let data = codec::decode(&wire.data, 1, expected).map_err(serde::de::Error::custom)?;
        Mask::from_data(wire.width, wire.height, data).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canvases_copy_regions_and_serialize_compactly() {
        let mut c = Canvas::filled(40, 30, WHITE);
        c.set(3, 4, [10, 20, 30, 255]);
        let r = c.region(IRect::new(2, 3, 3, 3)).unwrap();
        assert_eq!(r.get(1, 1), [10, 20, 30, 255]);
        assert_eq!(r.get(0, 0), WHITE);
        let mut d = Canvas::new(40, 30);
        d.put(2, 3, &r);
        assert_eq!(d.get(3, 4), [10, 20, 30, 255]);
        assert_eq!(d.get(0, 0), TRANSPARENT);
        // Clipped at the edges without panicking.
        d.put(38, 28, &r);
        assert!(c.region(IRect::new(50, 50, 4, 4)).is_none());
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.len() < 80, "{json}");
        let back: Canvas = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.hash(), c.hash());
        // A tampered buffer is refused, not silently padded.
        let bad = json.replace("\"width\":40", "\"width\":41");
        assert!(serde_json::from_str::<Canvas>(&bad).is_err());
        assert!(Canvas::from_rgba(2, 2, vec![0; 15]).is_err());
        assert!(check_size(MAX_SIDE + 1, 1).is_err());
        assert_eq!(parse_hex("#ff8000"), Some([255, 128, 0, 255]));
        assert_eq!(parse_hex("ff800080"), Some([255, 128, 0, 128]));
        assert_eq!(hex([255, 128, 0, 255]), "ff8000");
        assert!(parse_hex("zz").is_none());
        assert_eq!(c.opaque_bounds(), Some(IRect::new(0, 0, 40, 30)));
        assert_eq!(Canvas::new(3, 3).opaque_bounds(), None);
    }
    #[test]
    fn rectangles_intersect_union_and_clip() {
        let a = IRect::new(0, 0, 10, 10);
        let b = IRect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Some(IRect::new(5, 5, 5, 5)));
        assert_eq!(a.union(&b), IRect::new(0, 0, 15, 15));
        assert_eq!(b.clip(8, 8), Some(IRect::new(5, 5, 3, 3)));
        assert_eq!(IRect::spanning(4, 9, 1, 2), IRect::new(1, 2, 4, 8));
        assert!(IRect::new(0, 0, 2, 2)
            .intersect(&IRect::new(2, 0, 2, 2))
            .is_none());
    }
}
