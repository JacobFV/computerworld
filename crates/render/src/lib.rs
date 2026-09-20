//! Deterministic portable CPU compositor. No browser, GPU, host font or clock.
//! A renderer owns disposable glyph/text caches and a retained RGBA framebuffer.
mod assets;
mod colr;
mod faces;
mod font_pack;
mod glyph_fit;
#[cfg(test)]
mod script_tests;
mod symbols;
#[cfg(test)]
mod text_style_tests;
pub use assets::{decode, ASSET_IDS, SYMBOLS};
use cw_scene::{
    metrics,
    text::{self as shaping, terminal, FaceId, GlyphRef},
    text_cell, wrap_text, Color, Damage, Lang, Node, Primitive, Rect, Scene, Style, Transform,
    Typeface,
};
pub use font_pack::{
    font_pack_status, install_font, FontPackError, FontPackStatus, PackFile, FONT_PACK,
};
use fontdue::{Font, FontSettings};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const FONT_SHA256: &str = "0c8e3c7a3d59e41f73d593616c5c42ba3f4d916d9a5a93baeba1d56776a733ef";
pub const UI_FONT_SHA256: &str = "a8ef62637fccede99b4736e2a376aafb723807e217dba916bb607ce825627231";
/// The DejaVu faces are subset to the coverage the world can reach; the
/// full-Unicode masters stay in `assets/` but are not embedded. See
/// `assets/build-fonts.py` for the coverage set and why it is drawn that way.
/// Subsetting preserves outlines and advances exactly, so every retained glyph
/// rasterizes to the pixels the master produced.
const UI_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/dejavu-sans.ttf");
const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/dejavu-mono.ttf");
/// DejaVu Sans Oblique and Bold Oblique, subset to text scripts (see build-fonts.py).
static OBLIQUE_BYTES: [&[u8]; 2] = [
    include_bytes!("../assets/fonts/dejavu-sans-oblique.ttf"),
    include_bytes!("../assets/fonts/dejavu-sans-bold-oblique.ttf"),
];
use faces::{face_index, FACE_BYTES, FACE_COUNT};
/// Glyph-cache codes of the fonts a character can be drawn with.
const MONO: u8 = 0;
const PLATFORM: u8 = 3; // + face_index (the platform and web families), 3..=70
const OBLIQUE: u8 = PLATFORM + FACE_COUNT as u8; // + bold
const FALLBACK: u8 = 128; // + FaceId
/// Backdrop blur radius per pass; three passes reach three times this distance.
const MAX_BACKDROP_BLUR: u32 = 48;
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl Frame {
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y * self.width + x) * 4) as usize;
        Some(self.rgba[i..i + 4].try_into().unwrap())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RenderStats {
    pub frames: u64,
    pub painted_pixels: u64,
    pub glyph_cache_entries: usize,
    pub text_cache_entries: usize,
}
struct Glyph {
    metrics: fontdue::Metrics,
    alpha: Vec<u8>,
}
#[derive(Clone)]
struct Mask {
    width: u32,
    height: u32,
    alpha: Vec<u8>,
    /// Premultiplied RGBA of colour glyphs (emoji) drawn over the alpha coverage;
    /// empty when the block has none.
    color: Vec<u8>,
    spans: Vec<(u32, u32, u32)>,
}
/// (primitive, style, family, text, size, width, height) of a rasterized text block.
type TextKey = (u8, Style, Typeface, String, u16, u32, u32);
/// Cache limits bound resident text memory across arbitrary navigation histories.
pub struct Renderer {
    font: Font,
    ui_font: Option<Font>,
    bold_font: Option<Font>,
    faces: [Option<Font>; FACE_COUNT],
    oblique: [Option<Font>; 2],
    glyphs: BTreeMap<(u8, char, u16), Arc<Glyph>>,
    /// Shaped glyphs of the fallback faces, by glyph id.
    shaped: BTreeMap<(FaceId, u16, u16), Arc<Glyph>>,
    /// Colour emoji glyphs by glyph id and pixel size (`None`: not a colour glyph).
    colors: BTreeMap<(u16, u16), Arc<Option<colr::ColorGlyph>>>,
    texts: BTreeMap<TextKey, Arc<Mask>>,
    /// Font-pack generation the cached text masks were drawn with.
    pack_generation: u32,
    assets: BTreeMap<(String, u32, u32), Arc<Frame>>,
    shadows: BTreeMap<(u32, u32, u32, u32), Arc<Mask>>,
    frame: Frame,
    revision: Option<u64>,
    pub stats: RenderStats,
}
impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}
impl Renderer {
    pub fn new() -> Self {
        Self {
            font: Font::from_bytes(FONT_BYTES, FontSettings::default())
                .expect("bundled font is valid"),
            ui_font: None,
            bold_font: None,
            faces: [const { None }; FACE_COUNT],
            oblique: Default::default(),
            glyphs: BTreeMap::new(),
            shaped: BTreeMap::new(),
            colors: BTreeMap::new(),
            texts: BTreeMap::new(),
            pack_generation: font_pack::generation(),
            assets: BTreeMap::new(),
            shadows: BTreeMap::new(),
            frame: Frame::default(),
            revision: None,
            stats: RenderStats::default(),
        }
    }
    /// Convenience API for validated scenes. Use `try_render` at untrusted boundaries.
    pub fn render(&mut self, scene: &Scene) -> Frame {
        self.render_full(scene);
        self.frame.clone()
    }
    pub fn try_render(&mut self, scene: &Scene) -> Result<Frame, cw_scene::SceneError> {
        scene.validate()?;
        Ok(self.render(scene))
    }
    /// Render without copying the frame; its bytes remain valid until the next mutable call.
    pub fn render_ref(&mut self, scene: &Scene) -> &Frame {
        self.render_full(scene);
        &self.frame
    }
    /// A font-pack file arrived since the cached text was drawn: its boxes are stale.
    fn pack_changed(&mut self) -> bool {
        let generation = font_pack::generation();
        if generation == self.pack_generation {
            return false;
        }
        self.pack_generation = generation;
        self.texts.clear();
        self.glyphs.clear();
        self.shaped.clear();
        self.colors.clear();
        true
    }
    fn render_full(&mut self, scene: &Scene) {
        self.pack_changed();
        self.allocate(scene);
        let area = Rect::new(0, 0, scene.width, scene.height);
        self.paint(scene, &[area]);
        self.revision = Some(scene.revision);
    }
    /// The damage must describe every change from the prior frame. A size change or
    /// missing prior frame falls back to full repaint. Patches supply this contract.
    pub fn render_incremental(&mut self, scene: &Scene, damage: &Damage) -> &Frame {
        if self.revision.is_none()
            || self.pack_changed()
            || self.frame.width != scene.width
            || self.frame.height != scene.height
        {
            self.render_full(scene);
        } else {
            self.paint(scene, &damage.rects);
            self.revision = Some(scene.revision);
        }
        &self.frame
    }
    pub fn frame(&self) -> &Frame {
        &self.frame
    }
    fn allocate(&mut self, scene: &Scene) {
        let len = (scene.width as u128) * (scene.height as u128) * 4;
        assert!(
            scene.width <= i32::MAX as u32
                && scene.height <= i32::MAX as u32
                && len <= cw_scene::MAX_PIXELS as u128 * 4,
            "frame exceeds dimension safety limit; call try_render for untrusted scenes"
        );
        self.frame.width = scene.width;
        self.frame.height = scene.height;
        self.frame.rgba.resize(len as usize, 0);
    }
    fn glyph(&mut self, c: char, size: u16, ui: u8, typeface: Typeface) -> Arc<Glyph> {
        self.glyph_styled(c, size, ui, false, typeface)
    }
    /// A character of the table-driven faces: `ui` 0 is the terminal's monospace
    /// face, 1 and 2 the UI faces (regular, bold), `italic` slants the UI faces.
    fn glyph_styled(
        &mut self,
        c: char,
        size: u16,
        ui: u8,
        italic: bool,
        typeface: Typeface,
    ) -> Arc<Glyph> {
        let size = size.clamp(1, 256);
        // Platform families are Latin subsets; anything else falls back to DejaVu,
        // and italic text to the upright faces for what no italic face has. This is
        // `metrics::table_face`, so the advance measured is the glyph drawn.
        let bold = ui == 2;
        let face = match (
            ui,
            metrics::table_face(typeface, Style::new(bold, italic, Lang::Auto), c),
        ) {
            (0, _) => None,
            (_, Some((Typeface::Mono, _))) => Some(MONO),
            (_, Some((Typeface::DejaVu, true))) => Some(OBLIQUE + u8::from(bold)),
            (_, Some((family, slanted))) => {
                face_index(family, bold, slanted).map(|f| PLATFORM + f as u8)
            }
            (_, None) => None,
        };
        let ui = face.unwrap_or(ui);
        // Terminal text has no shaping, but a character DejaVu Sans Mono lacks still
        // draws from the fallback chain rather than as a box.
        let fallback = if ui == MONO && !shaping::dejavu_mono_covers(c) {
            shaping::script_face(false, c).and_then(|face| {
                let font = font_pack::face_font(face);
                if font.is_none() {
                    font_pack::note_missing(face);
                }
                font.map(|font| (face, font))
            })
        } else {
            None
        };
        let ui = fallback.map_or(ui, |(face, _)| FALLBACK + face as u8);
        if let Some(g) = self.glyphs.get(&(ui, c, size)) {
            return g.clone();
        }
        if self.glyphs.len() >= 8192
            || self.glyphs.values().map(|g| g.alpha.len()).sum::<usize>() > 8 * 1024 * 1024
        {
            self.glyphs.clear()
        }
        // A family without a file for the requested weight or slant is drawn from
        // the nearest face it has, with the rest synthesised (see `faces`).
        let (source, synthetic_bold, synthetic_oblique) = if (PLATFORM..OBLIQUE).contains(&ui) {
            faces::face_source(usize::from(ui - PLATFORM))
        } else {
            (0, false, false)
        };
        let font = if let Some((_, font)) = fallback {
            font
        } else if (OBLIQUE..OBLIQUE + 2).contains(&ui) {
            let i = usize::from(ui - OBLIQUE);
            self.oblique[i].get_or_insert_with(|| {
                Font::from_bytes(OBLIQUE_BYTES[i], FontSettings::default())
                    .expect("bundled oblique font is valid")
            })
        } else if (PLATFORM..OBLIQUE).contains(&ui) {
            self.faces[source].get_or_insert_with(|| {
                let bytes = FACE_BYTES[source].expect("face_source picks a bundled file");
                Font::from_bytes(bytes, FontSettings::default())
                    .expect("bundled platform font is valid")
            })
        } else if ui == 2 {
            self.bold_font.get_or_insert_with(|| {
                Font::from_bytes(
                    include_bytes!("../assets/fonts/dejavu-sans-bold.ttf") as &[u8],
                    FontSettings::default(),
                )
                .expect("bundled bold font is valid")
            })
        } else if ui > 0 {
            self.ui_font.get_or_insert_with(|| {
                Font::from_bytes(UI_FONT_BYTES, FontSettings::default())
                    .expect("bundled UI font is valid")
            })
        } else {
            &self.font
        };
        // A fallback glyph in a fixed terminal cell (CJK and emoji are a full em wide,
        // the cell 0.6 em) is drawn at the whole pixel size that fits the cell, so it
        // never overprints its neighbours. IEEE f32 multiply/divide/floor are exact
        // and identical on every target.
        let px = match fallback {
            Some(_) => {
                let cell = text_cell(size).0 as f32;
                let advance = font.metrics(c, size as f32).advance_width;
                if advance > cell {
                    (size as f32 * cell / advance).floor().max(1.0)
                } else {
                    size as f32
                }
            }
            None => size as f32,
        };
        let (metrics, alpha) = font.rasterize(c, px);
        // Terminal frames are what vision consumers OCR, so the fixed-pitch face is
        // grid-fitted; proportional UI text keeps the rasterizer's own output.
        let (metrics, alpha) = if ui == 0 {
            glyph_fit::fit(font, c, size, metrics, alpha)
        } else {
            (metrics, alpha)
        };
        let (metrics, alpha) = if synthetic_bold {
            faces::embolden(metrics, &alpha)
        } else {
            (metrics, alpha)
        };
        let (metrics, alpha) = if synthetic_oblique {
            faces::oblique(metrics, &alpha)
        } else {
            (metrics, alpha)
        };
        let glyph = Arc::new(Glyph { metrics, alpha });
        self.glyphs.insert((ui, c, size), glyph.clone());
        glyph
    }
    /// A shaped glyph of a fallback face, or `None` while its pack file is absent.
    fn shaped_glyph(&mut self, face: FaceId, index: u16, size: u16) -> Option<Arc<Glyph>> {
        let key = (face, index, size.clamp(1, 256));
        if let Some(g) = self.shaped.get(&key) {
            return Some(g.clone());
        }
        let Some(font) = font_pack::face_font(face) else {
            font_pack::note_missing(face);
            return None;
        };
        if self.shaped.len() >= 8192
            || self.shaped.values().map(|g| g.alpha.len()).sum::<usize>() > 8 * 1024 * 1024
        {
            self.shaped.clear()
        }
        let (metrics, alpha) = font.rasterize_indexed(index, key.2 as f32);
        let glyph = Arc::new(Glyph { metrics, alpha });
        self.shaped.insert(key, glyph.clone());
        Some(glyph)
    }
    fn text(
        &mut self,
        text: &str,
        size: u16,
        width: u32,
        height: u32,
        ui: u8,
        typeface: Typeface,
    ) -> Arc<Mask> {
        self.text_styled(
            text,
            size,
            width,
            height,
            ui,
            Style::from(ui == 2),
            typeface,
        )
    }
    /// Rasterize a text block: `ui` 0 is the terminal grid (`Text`), 1 and 2 are
    /// `UiText`/`UiTextBold` in `style` (italic and language; its weight is `ui`'s).
    #[allow(clippy::too_many_arguments)]
    fn text_styled(
        &mut self,
        text: &str,
        size: u16,
        width: u32,
        height: u32,
        ui: u8,
        style: Style,
        typeface: Typeface,
    ) -> Arc<Mask> {
        let style = if ui == 0 {
            Style::default()
        } else {
            Style {
                bold: ui == 2,
                ..style
            }
        };
        let key = (ui, style, typeface, text.to_owned(), size, width, height);
        if let Some(mask) = self.texts.get(&key) {
            return mask.clone();
        }
        if self.texts.len() >= 128
            || self
                .texts
                .values()
                .map(|m| m.alpha.len() + m.color.len() + m.spans.len() * 12)
                .sum::<usize>()
                > 16 * 1024 * 1024
        {
            self.texts.clear()
        }
        let len = width as u64 * height as u64;
        if len > 16 * 1024 * 1024 {
            return Arc::new(Mask {
                width: 0,
                height: 0,
                alpha: Vec::new(),
                color: Vec::new(),
                spans: Vec::new(),
            });
        }
        let mut mask = Mask {
            width,
            height,
            alpha: vec![0; len as usize],
            color: Vec::new(),
            spans: Vec::new(),
        };
        let size = size.clamp(1, 256);
        if ui > 0 {
            // Tabulated 1/64-pixel advances and word wrapping are shared with layout
            // code, avoiding platform floating-point drift. Raster origins are integers.
            let line_height = size as i64 + (size as i64 + 3) / 4;
            // Wrapping, fallback faces, bidi order and shaping all come from the same
            // layout the scene metrics measure with; Latin text takes its original
            // per-character path through it unchanged.
            let lines = shaping::layout(typeface, style, text, size, width);
            for (row, line) in lines.iter().enumerate() {
                let baseline = size as i64 + row as i64 * line_height;
                if baseline - size as i64 >= height as i64 {
                    break;
                }
                // Emoji clusters draw in colour when the colour face is installed;
                // otherwise their monochrome glyphs stay (and the page is told).
                let color = if line.emoji.is_empty() {
                    None
                } else {
                    let face = font_pack::color_emoji();
                    if face.is_none() {
                        font_pack::note_missing(FaceId::ColorEmoji);
                    }
                    face
                };
                for placed in &line.glyphs {
                    if color.is_some() && placed.face == Some(FaceId::Emoji) {
                        continue;
                    }
                    let g = match (placed.face, placed.glyph) {
                        (None, GlyphRef::Char(c)) => self.glyph_styled(
                            if c == '\t' { ' ' } else { c },
                            size,
                            ui,
                            style.italic,
                            typeface,
                        ),
                        (Some(face), GlyphRef::Index(index)) => {
                            match self.shaped_glyph(face, index, size) {
                                Some(g) => g,
                                // Pack file not installed yet: a box holds the place.
                                None => self.glyph('\u{FFFF}', size, ui, Typeface::DejaVu),
                            }
                        }
                        _ => continue,
                    };
                    let x = (placed.x + 32).div_euclid(64) + g.metrics.xmin as i64;
                    let y = baseline
                        - (placed.y + 32).div_euclid(64)
                        - g.metrics.height as i64
                        - g.metrics.ymin as i64;
                    blit(&mut mask, &g, x, y);
                }
                if let Some(face) = color {
                    for span in &line.emoji {
                        let cluster = &line.text[span.text.clone()];
                        self.color_cluster(
                            &mut mask, face, cluster, span.x0, span.x1, baseline, size,
                        );
                    }
                }
            }
        } else {
            let (cell, line_height) = text_cell(size);
            let columns = (width / cell).max(1) as usize;
            for (row, line) in wrap_text(text, columns).iter().enumerate() {
                let baseline = row as i64 * line_height as i64 + size as i64;
                if baseline - size as i64 >= height as i64 {
                    break;
                }
                if terminal::is_simple(line) {
                    for (col, c) in line.chars().enumerate() {
                        let g = self.glyph(c, size, 0, Typeface::DejaVu);
                        let x = col as i64 * cell as i64 + g.metrics.xmin as i64;
                        let y = baseline - g.metrics.height as i64 - g.metrics.ymin as i64;
                        blit(&mut mask, &g, x, y);
                    }
                } else {
                    self.terminal_row(&mut mask, line, size, cell, baseline);
                }
            }
        }
        for y in 0..height {
            let mut x = 0;
            let inked = |x: u32| {
                let i = (y * width + x) as usize;
                mask.alpha[i] != 0 || mask.color.get(i * 4 + 3).is_some_and(|a| *a != 0)
            };
            while x < width {
                while x < width && !inked(x) {
                    x += 1
                }
                let start = x;
                while x < width && inked(x) {
                    x += 1
                }
                if start < x {
                    mask.spans.push((y, start, x));
                }
            }
        }
        let mask = Arc::new(mask);
        if mask.alpha.len() + mask.color.len() + mask.spans.len() * 12 <= 16 * 1024 * 1024 {
            self.texts.insert(key, mask.clone());
        }
        mask
    }
    /// One terminal row that needs more than the plain grid: clusters in visual
    /// order, each drawn inside its own cells (see `cw_scene::text::terminal`).
    fn terminal_row(&mut self, mask: &mut Mask, line: &str, size: u16, cell: u32, baseline: i64) {
        for cluster in terminal::layout_line(line, size) {
            let x0 = i64::from(cluster.col) * i64::from(cell);
            let room = i64::from(cluster.width) * i64::from(cell) * 64;
            let text = &line[cluster.text.clone()];
            match cluster.face {
                None => {
                    // Monospace base with its marks: every character at the cell's
                    // origin, the way DejaVu Sans Mono positions combining marks.
                    for (k, c) in text.chars().enumerate() {
                        let c = if cluster.rtl && k == 0 {
                            unicode_mirror(c)
                        } else {
                            c
                        };
                        let g = self.glyph(c, size, 0, Typeface::DejaVu);
                        let x = x0 + g.metrics.xmin as i64;
                        let y = baseline - g.metrics.height as i64 - g.metrics.ymin as i64;
                        blit(mask, &g, x, y);
                    }
                }
                Some(FaceId::Emoji) if font_pack::color_emoji().is_some() => {
                    let face = font_pack::color_emoji().expect("checked");
                    let (glyphs, advance) = color_shape(face, text, size);
                    // Fit the cluster into its two cells.
                    let px = fit_size(size, advance, room);
                    let scaled = advance * i64::from(px) / i64::from(size);
                    let origin = x0 * 64 + (room - scaled) / 2;
                    for (glyph, x, y) in glyphs {
                        let x = origin + x * i64::from(px) / i64::from(size);
                        let y = y * i64::from(px) / i64::from(size);
                        self.blit_color(mask, face, glyph, px, x, y, baseline);
                    }
                }
                Some(face) => {
                    if face == FaceId::Emoji {
                        font_pack::note_missing(FaceId::ColorEmoji);
                    }
                    // Shaped glyphs of a wider cluster (a CJK glyph is 1 em, two cells
                    // 1.2 em; Arabic letters can exceed one cell) are drawn at the
                    // whole pixel size that fits, centred in the cells.
                    let px = fit_size(size, cluster.advance, room);
                    let scaled = cluster.advance * i64::from(px) / i64::from(size);
                    let origin = x0 * 64 + (room - scaled) / 2;
                    for placed in &cluster.glyphs {
                        let GlyphRef::Index(index) = placed.glyph else {
                            continue;
                        };
                        let g = match self.shaped_glyph(face, index, px) {
                            Some(g) => g,
                            None => self.glyph('\u{FFFF}', px, 0, Typeface::DejaVu),
                        };
                        let x = origin + placed.x * i64::from(px) / i64::from(size);
                        let y = placed.y * i64::from(px) / i64::from(size);
                        let x = (x + 32).div_euclid(64) + g.metrics.xmin as i64;
                        let y = baseline
                            - (y + 32).div_euclid(64)
                            - g.metrics.height as i64
                            - g.metrics.ymin as i64;
                        blit(mask, &g, x, y);
                    }
                }
            }
        }
    }
    /// A colour emoji cluster between pen positions `x0` and `x1` (1/64 pixel):
    /// shaped with the colour face and centred on the monochrome glyphs' span, so
    /// layout is the same with or without the colour face.
    #[allow(clippy::too_many_arguments)]
    fn color_cluster(
        &mut self,
        mask: &mut Mask,
        face: &'static rustybuzz::Face<'static>,
        cluster: &str,
        x0: i64,
        x1: i64,
        baseline: i64,
        size: u16,
    ) {
        let (glyphs, advance) = color_shape(face, cluster, size);
        let origin = x0 + (x1 - x0 - advance) / 2;
        for (glyph, x, y) in glyphs {
            self.blit_color(mask, face, glyph, size, origin + x, y, baseline);
        }
    }
    /// Composite one colour glyph (premultiplied) into the block's colour layer, its
    /// pen at `x` (1/64 pixel) and `y` above the baseline (1/64 pixel).
    #[allow(clippy::too_many_arguments)]
    fn blit_color(
        &mut self,
        mask: &mut Mask,
        face: &'static rustybuzz::Face<'static>,
        glyph: u16,
        size: u16,
        x: i64,
        y: i64,
        baseline: i64,
    ) {
        let key = (glyph, size);
        let raster = match self.colors.get(&key) {
            Some(r) => r.clone(),
            None => {
                if self.colors.len() >= 1024
                    || self
                        .colors
                        .values()
                        .filter_map(|g| g.as_ref().as_ref())
                        .map(|g| g.rgba.len())
                        .sum::<usize>()
                        > 16 * 1024 * 1024
                {
                    self.colors.clear();
                }
                let raster = Arc::new(colr::rasterize(
                    face,
                    rustybuzz::ttf_parser::GlyphId(glyph),
                    f32::from(size),
                    rustybuzz::ttf_parser::RgbaColor::new(0, 0, 0, 255),
                ));
                self.colors.insert(key, raster.clone());
                raster
            }
        };
        let Some(g) = raster.as_ref() else {
            return;
        };
        if mask.color.is_empty() {
            mask.color = vec![0; mask.alpha.len() * 4];
        }
        let left = (x + 32).div_euclid(64) + i64::from(g.left);
        let top = baseline - (y + 32).div_euclid(64) - i64::from(g.top);
        let (w, h) = (i64::from(mask.width), i64::from(mask.height));
        for gy in 0..i64::from(g.height) {
            let py = top + gy;
            if py < 0 || py >= h {
                continue;
            }
            for gx in 0..i64::from(g.width) {
                let px = left + gx;
                if px < 0 || px >= w {
                    continue;
                }
                let s = &g.rgba[((gy * i64::from(g.width) + gx) * 4) as usize..][..4];
                if s[3] == 0 {
                    continue;
                }
                let d = &mut mask.color[((py * w + px) * 4) as usize..][..4];
                let keep = 255 - u32::from(s[3]);
                for i in 0..4 {
                    d[i] = (u32::from(s[i]) + (u32::from(d[i]) * keep + 127) / 255).min(255) as u8;
                }
            }
        }
    }
    fn paint(&mut self, scene: &Scene, damage: &[Rect]) {
        let viewport = Rect::new(0, 0, scene.width, scene.height);
        let nodes = scene.ordered_nodes();
        let damage = backdrop_damage(normalize_damage(damage, viewport), &nodes, viewport);
        for damage in &damage {
            let Some(area) = damage.intersection(viewport) else {
                continue;
            };
            self.clear(area, scene.background);
            for node in &nodes {
                let Some(area) = node.painted_bounds().intersection(area) else {
                    continue;
                };
                if node.opacity == 0 {
                    continue;
                }
                let text = match &node.primitive {
                    Primitive::Text { text, size, .. } => Some(self.text(
                        text,
                        *size,
                        node.bounds.width,
                        node.bounds.height,
                        0,
                        scene.typeface,
                    )),
                    // A node set in its own family draws in it; the rest take the
                    // scene's.
                    Primitive::UiTextBold {
                        text,
                        size,
                        italic,
                        lang,
                        typeface,
                        ..
                    } => Some(self.text_styled(
                        text,
                        *size,
                        node.bounds.width,
                        node.bounds.height,
                        2,
                        Style::new(true, *italic, *lang),
                        typeface.unwrap_or(scene.typeface),
                    )),
                    Primitive::UiText {
                        text,
                        size,
                        italic,
                        lang,
                        typeface,
                        ..
                    } => Some(self.text_styled(
                        text,
                        *size,
                        node.bounds.width,
                        node.bounds.height,
                        1,
                        Style::new(false, *italic, *lang),
                        typeface.unwrap_or(scene.typeface),
                    )),
                    _ => None,
                };
                let asset = if let Primitive::AssetImage { asset }
                | Primitive::Symbol { asset, .. } = &node.primitive
                {
                    let key = (asset.clone(), node.bounds.width, node.bounds.height);
                    if !self.assets.contains_key(&key) {
                        if let Some(frame) = assets::decode(asset) {
                            // Antialias small icon resources once at their displayed size.
                            // Wallpapers remain shared at source size; avoid large copies.
                            let frame = if frame.width <= 256
                                && frame.height <= 256
                                && node.bounds.width > 0
                                && node.bounds.height > 0
                                && node.bounds.width as u64 * node.bounds.height as u64 <= 262_144
                            {
                                Arc::new(resample_icon(
                                    &frame,
                                    node.bounds.width,
                                    node.bounds.height,
                                ))
                            } else {
                                frame
                            };
                            if self.assets.len() >= 256
                                || self
                                    .assets
                                    .iter()
                                    .filter(|((id, _, _), _)| !id.starts_with("wallpaper/"))
                                    .map(|(_, f)| f.rgba.len())
                                    .sum::<usize>()
                                    > 8 * 1024 * 1024
                            {
                                self.assets.clear();
                            }
                            self.assets.insert(key.clone(), frame);
                        }
                    }
                    self.assets.get(&key).cloned()
                } else {
                    None
                };
                let shadow = if node.bounds.width as u64 * node.bounds.height as u64
                    > cw_scene::MAX_PIXELS
                {
                    None
                } else if let Primitive::Shadow { radius, blur, .. } = &node.primitive {
                    let key = (
                        node.bounds.width,
                        node.bounds.height,
                        *radius,
                        (*blur).min(128),
                    );
                    if !self.shadows.contains_key(&key) {
                        if self.shadows.values().map(|m| m.alpha.len()).sum::<usize>()
                            > 8 * 1024 * 1024
                        {
                            self.shadows.clear();
                        }
                        self.shadows
                            .insert(key, Arc::new(shadow_mask(key.0, key.1, key.2, key.3)));
                    }
                    self.shadows.get(&key).cloned()
                } else {
                    None
                };
                let backdrop = match &node.primitive {
                    Primitive::Backdrop { blur, .. } => {
                        self.blurred_backdrop(node, *blur, viewport)
                    }
                    _ => None,
                };
                self.paint_node(
                    node,
                    area,
                    text.as_deref(),
                    asset.as_deref(),
                    shadow.as_deref(),
                    backdrop.as_ref(),
                );
            }
        }
        self.stats.frames += 1;
        self.stats.glyph_cache_entries = self.glyphs.len();
        self.stats.text_cache_entries = self.texts.len();
    }
    /// Blurred copy of the pixels already composited beneath a backdrop node. The
    /// source extends three pass radii beyond the node so edge clamping never reaches
    /// visible pixels unless the viewport itself ends there.
    fn blurred_backdrop(&self, node: &Node, blur: u32, viewport: Rect) -> Option<(Rect, Vec<u8>)> {
        if node.transform != Transform::default() {
            return None;
        }
        let source = backdrop_source(node, blur, viewport)?;
        let (w, h) = (source.width as usize, source.height as usize);
        let mut rgb = vec![0u8; w * h * 3];
        for y in 0..h {
            let start =
                ((source.y as usize + y) * self.frame.width as usize + source.x as usize) * 4;
            for x in 0..w {
                rgb[(y * w + x) * 3..(y * w + x) * 3 + 3]
                    .copy_from_slice(&self.frame.rgba[start + x * 4..start + x * 4 + 3]);
            }
        }
        let r = blur.min(MAX_BACKDROP_BLUR) as usize;
        if r > 0 {
            let mut tmp = vec![0u8; rgb.len()];
            for _ in 0..3 {
                box_blur_rgb(&rgb, &mut tmp, w, h, r, true);
                box_blur_rgb(&tmp, &mut rgb, w, h, r, false);
            }
        }
        Some((source, rgb))
    }
    fn clear(&mut self, area: Rect, c: Color) {
        let pixel = [c.0, c.1, c.2, c.3];
        for y in area.y as u32..area.y as u32 + area.height {
            let start = ((y * self.frame.width + area.x as u32) * 4) as usize;
            for dst in self.frame.rgba[start..start + area.width as usize * 4]
                .as_chunks_mut::<4>()
                .0
            {
                dst.copy_from_slice(&pixel)
            }
        }
    }
    fn paint_node(
        &mut self,
        node: &Node,
        area: Rect,
        text: Option<&Mask>,
        asset: Option<&Frame>,
        shadow: Option<&Mask>,
        backdrop: Option<&(Rect, Vec<u8>)>,
    ) {
        let identity = node.transform == Transform::default();
        let clip_coverage = |x: i64, y: i64| {
            node.rounded_clip.map_or(255, |c| {
                rounded_coverage(
                    x - c.rect.x as i64,
                    y - c.rect.y as i64,
                    c.rect.width,
                    c.rect.height,
                    c.radius,
                )
            })
        };
        if identity {
            if let (
                Primitive::Text { color, .. }
                | Primitive::UiText { color, .. }
                | Primitive::UiTextBold { color, .. },
                Some(mask),
            ) = (&node.primitive, text)
            {
                for &(row, start, end) in &mask.spans {
                    let y = node.bounds.y as i64 + row as i64;
                    if y < area.y as i64 || y >= area.y as i64 + area.height as i64 {
                        continue;
                    }
                    let left = (node.bounds.x as i64 + start as i64).max(area.x as i64);
                    let right =
                        (node.bounds.x as i64 + end as i64).min(area.x as i64 + area.width as i64);
                    for x in left..right {
                        let i = (row * mask.width) as usize + (x - node.bounds.x as i64) as usize;
                        let alpha = mask.alpha[i];
                        let color = if mask.color.is_empty() {
                            Color(
                                color.0,
                                color.1,
                                color.2,
                                mul_alpha(
                                    mul_alpha(mul_alpha(color.3, alpha), node.opacity),
                                    clip_coverage(x, y),
                                ),
                            )
                        } else {
                            let c = text_pixel(*color, alpha, &mask.color[i * 4..i * 4 + 4]);
                            Color(
                                c.0,
                                c.1,
                                c.2,
                                mul_alpha(mul_alpha(c.3, node.opacity), clip_coverage(x, y)),
                            )
                        };
                        if color.3 == 0 {
                            continue;
                        }
                        let i = ((y as u32 * self.frame.width + x as u32) * 4) as usize;
                        blend(&mut self.frame.rgba[i..i + 4], color);
                        self.stats.painted_pixels += 1;
                    }
                }
                return;
            }
            if let Primitive::Box {
                fill, border: None, ..
            } = &node.primitive
            {
                if fill.3 == 255 && node.opacity == 255 {
                    let Some(clip) = node.rounded_clip else {
                        self.clear(area, *fill);
                        self.stats.painted_pixels += area.width as u64 * area.height as u64;
                        return;
                    };
                    // Rows clear of the clip's corners are constant; only the corner
                    // bands need per-pixel coverage.
                    let r = clip
                        .radius
                        .min(clip.rect.width / 2)
                        .min(clip.rect.height / 2);
                    let straight = Rect::new(
                        clip.rect.x,
                        clip.rect.y.saturating_add(r as i32),
                        clip.rect.width,
                        clip.rect.height - r * 2,
                    );
                    if let Some(fast) = area.intersection(straight) {
                        self.clear(fast, *fill);
                        self.stats.painted_pixels += fast.width as u64 * fast.height as u64;
                        let below = fast.y + fast.height as i32;
                        for band in [
                            Rect::new(area.x, area.y, area.width, (fast.y - area.y) as u32),
                            Rect::new(
                                area.x,
                                below,
                                area.width,
                                (area.y + area.height as i32 - below) as u32,
                            ),
                        ] {
                            if band.height > 0 {
                                self.paint_pixels(node, band, text, asset, shadow, backdrop);
                            }
                        }
                        return;
                    }
                }
            }
        }
        self.paint_pixels(node, area, text, asset, shadow, backdrop);
    }
    fn paint_pixels(
        &mut self,
        node: &Node,
        mut area: Rect,
        text: Option<&Mask>,
        asset: Option<&Frame>,
        shadow: Option<&Mask>,
        backdrop: Option<&(Rect, Vec<u8>)>,
    ) {
        let identity = node.transform == Transform::default();
        if let (
            true,
            Primitive::Path {
                points,
                stroke_width,
                ..
            },
        ) = (identity, &node.primitive)
        {
            // Shell glyphs use scene-sized bounds; only their extent can paint.
            let pad = i64::from(*stroke_width) / 2 + 2;
            let extent = |f: fn(&(i32, i32)) -> i32, origin: i32| {
                let lo = points.iter().map(f).min().unwrap_or(0) as i64 + origin as i64 - pad;
                let hi = points.iter().map(f).max().unwrap_or(0) as i64 + origin as i64 + pad;
                (
                    lo.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                    (hi - lo + 1).clamp(0, u32::MAX as i64) as u32,
                )
            };
            let (x, width) = extent(|p| p.0, node.bounds.x);
            let (y, height) = extent(|p| p.1, node.bounds.y);
            match area.intersection(Rect::new(x, y, width, height)) {
                Some(tight) => area = tight,
                None => return,
            }
        }
        let clip_coverage = |x: i64, y: i64| {
            node.rounded_clip.map_or(255, |c| {
                rounded_coverage(
                    x - c.rect.x as i64,
                    y - c.rect.y as i64,
                    c.rect.width,
                    c.rect.height,
                    c.radius,
                )
            })
        };
        for y in area.y..area.y + area.height as i32 {
            for x in area.x..area.x + area.width as i32 {
                let p = if identity {
                    Some((x, y))
                } else {
                    node.transform.inverse_point(x, y)
                };
                let Some((lx, ly)) = p else { continue };
                if !node.bounds.contains(lx, ly) {
                    continue;
                }
                let local_x = lx as i64 - node.bounds.x as i64;
                let local_y = ly as i64 - node.bounds.y as i64;
                let mut color = match &node.primitive {
                    Primitive::Region => continue,
                    Primitive::Box {
                        fill,
                        border,
                        border_width,
                    } => {
                        let bw = *border_width as i64;
                        if local_x < bw
                            || local_y < bw
                            || local_x >= node.bounds.width as i64 - bw
                            || local_y >= node.bounds.height as i64 - bw
                        {
                            border.unwrap_or(*fill)
                        } else {
                            *fill
                        }
                    }
                    Primitive::RoundedBox {
                        fill,
                        border,
                        border_width,
                        radius,
                    } => {
                        let outer = rounded_coverage(
                            local_x,
                            local_y,
                            node.bounds.width,
                            node.bounds.height,
                            *radius,
                        );
                        if outer == 0 {
                            continue;
                        }
                        let bw = (*border_width)
                            .min(node.bounds.width.div_ceil(2))
                            .min(node.bounds.height.div_ceil(2));
                        let inner = if border.is_some() && bw > 0 {
                            rounded_coverage(
                                local_x - bw as i64,
                                local_y - bw as i64,
                                node.bounds.width.saturating_sub(bw.saturating_mul(2)),
                                node.bounds.height.saturating_sub(bw.saturating_mul(2)),
                                radius.saturating_sub(bw),
                            )
                        } else {
                            outer
                        };
                        // Blend disjoint edge/interior coverage once, avoiding a dark
                        // seam where translucent antialiased fill meets its border.
                        let edge = outer.saturating_sub(inner);
                        let border = border.unwrap_or(*fill);
                        if inner == outer {
                            Color(fill.0, fill.1, fill.2, mul_alpha(fill.3, outer))
                        } else if inner == 0 {
                            Color(border.0, border.1, border.2, mul_alpha(border.3, outer))
                        } else {
                            let alpha =
                                fill.3 as u32 * inner as u32 + border.3 as u32 * edge as u32;
                            if alpha == 0 {
                                continue;
                            }
                            let channel = |f: u8, b: u8| {
                                ((f as u32 * fill.3 as u32 * inner as u32
                                    + b as u32 * border.3 as u32 * edge as u32
                                    + alpha / 2)
                                    / alpha) as u8
                            };
                            Color(
                                channel(fill.0, border.0),
                                channel(fill.1, border.1),
                                channel(fill.2, border.2),
                                ((alpha + 127) / 255) as u8,
                            )
                        }
                    }
                    Primitive::Text { color, .. }
                    | Primitive::UiText { color, .. }
                    | Primitive::UiTextBold { color, .. } => {
                        let Some(mask) = text else { continue };
                        if local_x >= mask.width as i64 || local_y >= mask.height as i64 {
                            continue;
                        }
                        let i = local_y as usize * mask.width as usize + local_x as usize;
                        let a = mask.alpha[i];
                        if mask.color.is_empty() {
                            Color(color.0, color.1, color.2, mul_alpha(color.3, a))
                        } else {
                            text_pixel(*color, a, &mask.color[i * 4..i * 4 + 4])
                        }
                    }
                    Primitive::Shadow { color, .. } => {
                        let Some(mask) = shadow else { continue };
                        let alpha =
                            mask.alpha[local_y as usize * mask.width as usize + local_x as usize];
                        Color(color.0, color.1, color.2, mul_alpha(color.3, alpha))
                    }
                    Primitive::AssetImage { .. } => {
                        let Some(frame) = asset else { continue };
                        if frame.width > 256 || frame.height > 256 {
                            // Wallpapers fill their bounds like a real desktop: centred,
                            // aspect-preserving crop with bilinear filtering.
                            cover_sample(frame, node.bounds, local_x, local_y)
                        } else {
                            let sx = local_x as u64 * frame.width as u64 / node.bounds.width as u64;
                            let sy =
                                local_y as u64 * frame.height as u64 / node.bounds.height as u64;
                            let i = ((sy * frame.width as u64 + sx) * 4) as usize;
                            let p = &frame.rgba[i..i + 4];
                            Color(p[0], p[1], p[2], p[3])
                        }
                    }
                    Primitive::Symbol { color, .. } => {
                        let Some(frame) = asset else { continue };
                        let sx = local_x as u64 * frame.width as u64 / node.bounds.width as u64;
                        let sy = local_y as u64 * frame.height as u64 / node.bounds.height as u64;
                        let alpha = frame.rgba[((sy * frame.width as u64 + sx) * 4) as usize + 3];
                        Color(color.0, color.1, color.2, mul_alpha(color.3, alpha))
                    }
                    Primitive::Image {
                        width,
                        height,
                        rgba,
                    } => {
                        if *width == 0 || *height == 0 {
                            continue;
                        }
                        let sx = local_x as u64 * (*width as u64) / node.bounds.width as u64;
                        let sy = local_y as u64 * (*height as u64) / node.bounds.height as u64;
                        let i = ((sy * (*width as u64) + sx) * 4) as usize;
                        let Some(p) = rgba.get(i..i + 4) else {
                            continue;
                        };
                        Color(p[0], p[1], p[2], p[3])
                    }
                    Primitive::Path {
                        points,
                        fill,
                        stroke,
                        stroke_width,
                        closed,
                    } => path_color(
                        (local_x as i32, local_y as i32),
                        points,
                        if *closed { *fill } else { None },
                        if *stroke_width > 0 { *stroke } else { None },
                        *stroke_width,
                        *closed,
                    ),
                    Primitive::Backdrop { radius, .. } => {
                        let Some((source, rgb)) = backdrop else {
                            continue;
                        };
                        let coverage = rounded_coverage(
                            local_x,
                            local_y,
                            node.bounds.width,
                            node.bounds.height,
                            *radius,
                        );
                        if !source.contains(x, y) {
                            continue;
                        }
                        let i = ((y - source.y) as usize * source.width as usize
                            + (x - source.x) as usize)
                            * 3;
                        Color(rgb[i], rgb[i + 1], rgb[i + 2], coverage)
                    }
                };
                color.3 = mul_alpha(
                    mul_alpha(color.3, node.opacity),
                    clip_coverage(x as i64, y as i64),
                );
                if color.3 == 0 {
                    continue;
                }
                let i = ((y as u32 * self.frame.width + x as u32) * 4) as usize;
                blend(&mut self.frame.rgba[i..i + 4], color);
                self.stats.painted_pixels += 1;
            }
        }
    }
}
// Four-by-four fixed sample grid is deterministic across native/Wasm. Only the
// curved corner pixels pay supersampling cost; rectangular interiors are constant.
fn rounded_coverage(x: i64, y: i64, width: u32, height: u32, radius: u32) -> u8 {
    if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
        return 0;
    }
    let r = radius.min(width / 2).min(height / 2) as i64;
    if r == 0 || (x >= r && x < width as i64 - r) || (y >= r && y < height as i64 - r) {
        return 255;
    }
    let r = r as i128 * 8;
    let w = width as i128 * 8;
    let h = height as i128 * 8;
    let mut coverage = 0u32;
    for sy in [1, 3, 5, 7] {
        for sx in [1, 3, 5, 7] {
            let px = x as i128 * 8 + sx;
            let py = y as i128 * 8 + sy;
            let dx = (r - px).max(px - (w - r)).max(0);
            let dy = (r - py).max(py - (h - r)).max(0);
            if dx * dx + dy * dy <= r * r {
                coverage += 1;
            }
        }
    }
    ((coverage * 255 + 8) / 16) as u8
}
// Coalesce redundant old/new damage and contiguous rectangles before rasterization.
// Extra pixels inside a merged bounding rectangle are safe to repaint. Restrict
// merging to cases where that rectangle costs no more than the two inputs.
fn normalize_damage(input: &[Rect], viewport: Rect) -> Vec<Rect> {
    let mut rects: Vec<_> = input
        .iter()
        .filter_map(|r| r.intersection(viewport))
        .collect();
    rects.sort_by_key(|r| (r.y, r.x, r.height, r.width));
    rects.dedup();
    let mut i = 0;
    while i < rects.len() {
        let mut j = i + 1;
        while j < rects.len() {
            let a = rects[i];
            let b = rects[j];
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            let right = (a.x as i64 + a.width as i64).max(b.x as i64 + b.width as i64);
            let bottom = (a.y as i64 + a.height as i64).max(b.y as i64 + b.height as i64);
            let merged = Rect::new(x, y, (right - x as i64) as u32, (bottom - y as i64) as u32);
            if merged.width as u64 * merged.height as u64
                <= a.width as u64 * a.height as u64 + b.width as u64 * b.height as u64
            {
                rects[i] = merged;
                rects.remove(j);
                j = i + 1;
            } else {
                j += 1
            }
        }
        i += 1;
    }
    rects
}
fn mul_alpha(a: u8, b: u8) -> u8 {
    ((a as u32 * b as u32 + 127) / 255) as u8
}
/// Max-composite a glyph's coverage into a text mask with its top-left at (x, y).
fn blit(mask: &mut Mask, g: &Glyph, x: i64, y: i64) {
    let (width, height) = (mask.width as i64, mask.height as i64);
    for gy in 0..g.metrics.height {
        let py = y + gy as i64;
        if py < 0 || py >= height {
            continue;
        }
        for gx in 0..g.metrics.width {
            let px = x + gx as i64;
            if px >= 0 && px < width {
                let i = py as usize * width as usize + px as usize;
                mask.alpha[i] = mask.alpha[i].max(g.alpha[gy * g.metrics.width + gx]);
            }
        }
    }
}
/// A text pixel with a colour layer: the colour glyph (premultiplied) over the
/// text colour at `alpha` coverage, as one straight-alpha colour.
fn text_pixel(text: Color, alpha: u8, glyph: &[u8]) -> Color {
    let ta = u32::from(mul_alpha(text.3, alpha));
    let ga = u32::from(glyph[3]);
    let out = ga + (ta * (255 - ga) + 127) / 255;
    if out == 0 {
        return Color(text.0, text.1, text.2, 0);
    }
    let channel = |t: u8, g: u8| {
        let premultiplied = u32::from(g) * 255 + u32::from(t) * ta * (255 - ga) / 255;
        ((premultiplied + out / 2) / out).min(255) as u8
    };
    Color(
        channel(text.0, glyph[0]),
        channel(text.1, glyph[1]),
        channel(text.2, glyph[2]),
        out.min(255) as u8,
    )
}
/// The whole pixel size at which something `advance` wide (1/64 pixel at `size`)
/// fits in `room`: `size` itself when it already fits.
fn fit_size(size: u16, advance: i64, room: i64) -> u16 {
    if advance > room && advance > 0 {
        (i64::from(size) * room / advance).clamp(1, i64::from(size)) as u16
    } else {
        size
    }
}
/// Shape an emoji cluster with the colour face: glyph ids with pen x and y (1/64
/// pixel at `size`), and the total advance.
fn color_shape(face: &rustybuzz::Face<'_>, text: &str, size: u16) -> (Vec<(u16, i64, i64)>, i64) {
    let mut pen = 0;
    let mut out = Vec::new();
    for g in shaping::shape_with(face, text, false) {
        out.push((
            g.glyph,
            pen + shaping::scale(g.x_offset, g.upem, size),
            shaping::scale(g.y_offset, g.upem, size),
        ));
        pen += shaping::scale(g.x_advance, g.upem, size);
    }
    (out, pen)
}
fn unicode_mirror(c: char) -> char {
    shaping::mirrored(c)
}
fn blend(dst: &mut [u8], src: Color) {
    if src.3 == 255 {
        dst.copy_from_slice(&[src.0, src.1, src.2, 255]);
        return;
    }
    let sa = src.3 as u32;
    let da = dst[3] as u32;
    let out = sa * 255 + da * (255 - sa);
    if out == 0 {
        dst.fill(0);
        return;
    }
    for (i, s) in [src.0, src.1, src.2].iter().enumerate() {
        dst[i] = (((*s as u32) * sa * 255 + dst[i] as u32 * da * (255 - sa) + out / 2) / out) as u8;
    }
    dst[3] = ((out + 127) / 255) as u8;
}
fn cover_sample(frame: &Frame, bounds: Rect, x: i64, y: i64) -> Color {
    let (fw, fh) = (frame.width as i64, frame.height as i64);
    let (w, h) = (bounds.width.max(1) as i64, bounds.height.max(1) as i64);
    let (crop_w, crop_h) = if fw * h > fh * w {
        ((fh * w / h).max(1), fh)
    } else {
        (fw, (fw * h / w).max(1))
    };
    let axis = |v: i64, out: i64, crop: i64, full: i64| {
        let p = (full - crop) / 2 * 256 + ((2 * v + 1) * crop * 256) / (2 * out) - 128;
        let p = p.clamp(0, (full - 1) * 256);
        (
            (p / 256) as usize,
            ((p / 256 + 1).min(full - 1)) as usize,
            (p % 256) as u32,
        )
    };
    let (x0, x1, fx) = axis(x, w, crop_w, fw);
    let (y0, y1, fy) = axis(y, h, crop_h, fh);
    let at = |x: usize, y: usize| &frame.rgba[(y * frame.width as usize + x) * 4..][..4];
    let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    let mix = |i: usize| {
        let top = a[i] as u32 * (256 - fx) + b[i] as u32 * fx;
        let bottom = c[i] as u32 * (256 - fx) + d[i] as u32 * fx;
        ((top * (256 - fy) + bottom * fy + 32768) >> 16) as u8
    };
    Color(mix(0), mix(1), mix(2), mix(3))
}
/// Source pixels a backdrop reads: its bounds grown by the three-pass blur reach.
fn backdrop_source(node: &Node, blur: u32, viewport: Rect) -> Option<Rect> {
    let reach = (blur.min(MAX_BACKDROP_BLUR) * 3) as i32;
    let b = node.bounds.intersection(viewport)?;
    Rect::new(
        b.x - reach,
        b.y - reach,
        b.width + reach as u32 * 2,
        b.height + reach as u32 * 2,
    )
    .intersection(viewport)
}
/// A repaint touching a backdrop's source must repaint the whole source in one pass,
/// because pixels outside the damage already hold layers composited above it.
fn backdrop_damage(mut rects: Vec<Rect>, nodes: &[&Node], viewport: Rect) -> Vec<Rect> {
    let sources: Vec<Rect> = nodes
        .iter()
        .filter(|n| n.opacity > 0)
        .filter_map(|n| match n.primitive {
            Primitive::Backdrop { blur, .. } => backdrop_source(n, blur, viewport),
            _ => None,
        })
        .collect();
    if sources.is_empty() {
        return rects;
    }
    loop {
        let mut changed = false;
        for source in &sources {
            let hits: Vec<usize> = (0..rects.len())
                .filter(|&i| rects[i].intersection(*source).is_some())
                .collect();
            let contained =
                hits.len() == 1 && rects[hits[0]].intersection(*source) == Some(*source);
            if hits.is_empty() || contained {
                continue;
            }
            let mut merged = *source;
            for &i in hits.iter().rev() {
                merged = union(merged, rects.remove(i));
            }
            rects.push(merged);
            changed = true;
        }
        if !changed {
            return rects;
        }
        rects = normalize_damage(&rects, viewport);
    }
}
fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = (a.x as i64 + a.width as i64).max(b.x as i64 + b.width as i64);
    let bottom = (a.y as i64 + a.height as i64).max(b.y as i64 + b.height as i64);
    Rect::new(x, y, (right - x as i64) as u32, (bottom - y as i64) as u32)
}
/// One clamped running-sum box pass over interleaved RGB rows or columns.
fn box_blur_rgb(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize, horizontal: bool) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let index = |line: usize, i: usize| {
        if horizontal {
            (line * w + i) * 3
        } else {
            (i * w + line) * 3
        }
    };
    let window = (r * 2 + 1) as u32;
    for line in 0..lines {
        for channel in 0..3 {
            let at =
                |i: i64| src[index(line, i.clamp(0, len as i64 - 1) as usize) + channel] as u32;
            let mut sum: u32 = (-(r as i64)..=r as i64).map(at).sum();
            for i in 0..len {
                dst[index(line, i) + channel] = ((sum + window / 2) / window) as u8;
                sum += at(i as i64 + r as i64 + 1);
                sum -= at(i as i64 - r as i64);
            }
        }
    }
}
// Path geometry is evaluated in eighths of a pixel so the same integer predicates
// serve both pixel centres and the 4x4 antialiasing grid.
fn inside_polygon((x, y): (i128, i128), points: &[(i32, i32)]) -> bool {
    let mut inside = false;
    if points.len() < 3 {
        return false;
    }
    for i in 0..points.len() {
        let (ax, ay) = (points[i].0 as i128 * 8, points[i].1 as i128 * 8);
        let b = points[(i + 1) % points.len()];
        let (bx, by) = (b.0 as i128 * 8, b.1 as i128 * 8);
        if (ay > y) != (by > y) {
            let lhs = (x - ax) * (by - ay);
            let rhs = (bx - ax) * (y - ay);
            if if by > ay { lhs < rhs } else { lhs > rhs } {
                inside = !inside
            }
        }
    }
    inside
}
/// Whether any segment lies within `distance` eighth-pixels of `p`.
fn near_path(p: (i128, i128), points: &[(i32, i32)], closed: bool, distance: i128) -> bool {
    if distance < 0 || points.len() < 2 {
        return false;
    }
    let n = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    let limit = distance * distance;
    (0..n).any(|i| {
        let a = (points[i].0 as i128 * 8, points[i].1 as i128 * 8);
        let b = points[(i + 1) % points.len()];
        let b = (b.0 as i128 * 8, b.1 as i128 * 8);
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let px = p.0 - a.0;
        let py = p.1 - a.1;
        let len = dx * dx + dy * dy;
        let dot = px * dx + py * dy;
        if len == 0 || dot <= 0 {
            px * px + py * py <= limit
        } else if dot >= len {
            let x = p.0 - b.0;
            let y = p.1 - b.1;
            x * x + y * y <= limit
        } else {
            let cross = px * dy - py * dx;
            cross * cross <= limit * len
        }
    })
}
/// Antialiased fill and stroke. Integer coordinates address pixel centres; pixels
/// provably clear of every edge take one sample, the rest a fixed 4x4 grid.
fn path_color(
    (x, y): (i32, i32),
    points: &[(i32, i32)],
    fill: Option<Color>,
    stroke: Option<Color>,
    stroke_width: u16,
    closed: bool,
) -> Color {
    let centre = (x as i128 * 8, y as i128 * 8);
    let half = stroke_width as i128 * 4;
    // Samples lie within 3√2 < 5 eighths of the centre.
    if let Some(stroke) = stroke {
        if half > 5 && near_path(centre, points, closed, half - 5) {
            return stroke;
        }
    }
    let stroke_edge = stroke.is_some() && near_path(centre, points, closed, half + 5);
    let fill_edge = fill.is_some() && near_path(centre, points, true, 5);
    if !stroke_edge && !fill_edge {
        return match fill {
            Some(fill) if inside_polygon(centre, points) => fill,
            _ => Color::TRANSPARENT,
        };
    }
    let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
    for sy in [-3, -1, 1, 3] {
        for sx in [-3, -1, 1, 3] {
            let p = (centre.0 + sx, centre.1 + sy);
            let sample = match (stroke, fill) {
                (Some(stroke), _) if near_path(p, points, closed, half) => stroke,
                (_, Some(fill)) if inside_polygon(p, points) => fill,
                _ => continue,
            };
            r += sample.0 as u32 * sample.3 as u32;
            g += sample.1 as u32 * sample.3 as u32;
            b += sample.2 as u32 * sample.3 as u32;
            a += sample.3 as u32;
        }
    }
    if a == 0 {
        return Color::TRANSPARENT;
    }
    Color(
        ((r + a / 2) / a) as u8,
        ((g + a / 2) / a) as u8,
        ((b + a / 2) / a) as u8,
        ((a + 8) / 16) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cw_scene::{PatchOp, ScenePatch};
    use sha2::{Digest, Sha256};
    fn sample() -> Scene {
        let mut s = Scene::new(160, 90);
        s.background = Color::rgb(10, 20, 30);
        s.nodes.push(Node::rectangle(
            1,
            Rect::new(3, 4, 100, 70),
            Color(100, 200, 30, 128),
        ));
        s.nodes.push(Node::text(
            2,
            Rect::new(8, 8, 130, 65),
            "Hello world\nDeterministic λ",
            16,
            Color::WHITE,
        ));
        s
    }
    #[test]
    fn deterministic() {
        let s = sample();
        let a = Renderer::new().render(&s);
        let b = Renderer::new().render(&s);
        assert_eq!(a, b);
        assert!(a.rgba.contains(&255));
        assert_eq!(
            format!("{:x}", Sha256::digest(&a.rgba)),
            "01455c4eaa6c1eca6900b545f69bba35ad9428fb66275a41df86268ae3595ec6"
        );
    }
    #[test]
    fn incremental_equals_full() {
        let mut s = sample();
        let mut r = Renderer::new();
        r.render(&s);
        let d = s
            .patch(ScenePatch {
                base_revision: 0,
                revision: 1,
                operations: vec![
                    PatchOp::Remove(1),
                    PatchOp::Upsert(Node::text(
                        2,
                        Rect::new(20, 10, 110, 60),
                        "Updated",
                        18,
                        Color::BLACK,
                    )),
                ],
            })
            .unwrap();
        let actual = r.render_incremental(&s, &d).clone();
        assert_eq!(actual, Renderer::new().render(&s));
    }
    #[test]
    fn transforms_clips_images_paths() {
        let mut s = Scene::new(40, 40);
        let mut n = Node::rectangle(1, Rect::new(0, 0, 20, 20), Color::BLACK);
        n.transform = Transform::translate(10, 10);
        n.clip = Some(Rect::new(10, 10, 5, 5));
        s.nodes.push(n);
        s.nodes.push(Node::new(
            2,
            Rect::new(25, 0, 10, 10),
            Primitive::Image {
                width: 1,
                height: 1,
                rgba: vec![255, 0, 0, 255],
            },
        ));
        s.nodes.push(Node::new(
            3,
            Rect::new(0, 25, 10, 10),
            Primitive::Path {
                points: vec![(0, 0), (9, 0), (0, 9)],
                fill: Some(Color::BLACK),
                stroke: None,
                stroke_width: 0,
                closed: true,
            },
        ));
        let f = Renderer::new().render(&s);
        assert_eq!(f.pixel(11, 11), Some([0, 0, 0, 255]));
        assert_eq!(f.pixel(16, 11), Some([255, 255, 255, 255]));
        assert_eq!(f.pixel(26, 1), Some([255, 0, 0, 255]));
        assert_eq!(f.pixel(1, 26), Some([0, 0, 0, 255]));
    }
    #[test]
    fn rounded_ui_golden_and_incremental() {
        let mut scene = Scene::new(220, 100);
        scene.background = Color::rgb(24, 31, 46);
        let mut panel =
            Node::rounded_rectangle(1, Rect::new(8, 8, 204, 84), Color(246, 248, 255, 230), 17);
        panel.primitive = Primitive::RoundedBox {
            fill: Color(246, 248, 255, 230),
            border: Some(Color::WHITE),
            border_width: 2,
            radius: 17,
        };
        scene.nodes.push(panel);
        scene.nodes.push(Node::ui_text(
            2,
            Rect::new(20, 17, 182, 62),
            "Window settings\nWiFi · λ · 09:41",
            17,
            Color::rgb(24, 31, 46),
        ));
        let mut renderer = Renderer::new();
        let frame = renderer.render(&scene);
        assert_eq!(frame.pixel(8, 8), Some([24, 31, 46, 255]));
        assert_eq!(frame.pixel(110, 8), Some([255, 255, 255, 255]));
        assert_eq!(frame, Renderer::new().render(&scene));
        let digest = format!("{:x}", Sha256::digest(&frame.rgba));
        assert_eq!(
            digest,
            "da928cab834c47449b5d838ccd2f3338e2e318dc63237f421dd7ac3d79324cf0"
        );
        let damage = scene
            .patch(ScenePatch {
                base_revision: 0,
                revision: 1,
                operations: vec![PatchOp::Upsert(Node::ui_text(
                    2,
                    Rect::new(20, 17, 182, 62),
                    "Connected\nWiFi · λ · 09:42",
                    17,
                    Color::BLACK,
                ))],
            })
            .unwrap();
        assert_eq!(
            renderer.render_incremental(&scene, &damage).clone(),
            Renderer::new().render(&scene)
        );
    }
    #[test]
    fn ui_proportional_advances_clipping_and_cache() {
        let mut renderer = Renderer::new();
        let narrow = renderer.text("iiii", 20, 180, 30, 1, Typeface::DejaVu);
        let wide = renderer.text("WWWW", 20, 180, 30, 1, Typeface::DejaVu);
        let right = |mask: &Mask| mask.spans.iter().map(|s| s.2).max().unwrap();
        assert!(right(&wide) > right(&narrow) * 2);
        let cached = renderer.text("iiii", 20, 180, 30, 1, Typeface::DejaVu);
        assert!(Arc::ptr_eq(&narrow, &cached));
        let mut scene = Scene::new(80, 30);
        let mut label = Node::ui_text(
            1,
            Rect::new(-5, -3, 180, 40),
            "Outside clip",
            24,
            Color::BLACK,
        );
        label.clip = Some(Rect::new(10, 5, 20, 15));
        scene.nodes.push(label);
        let frame = renderer.render(&scene);
        for y in 0..30 {
            for x in 0..80 {
                if !Rect::new(10, 5, 20, 15).contains(x, y) {
                    assert_eq!(frame.pixel(x as u32, y as u32), Some([255, 255, 255, 255]));
                }
            }
        }
    }
    #[test]
    fn rounded_coverage_symmetric_and_bounded() {
        for width in [1, 2, 13, 32] {
            for height in [1, 2, 15, 32] {
                for radius in [0, 1, 7, 500] {
                    for y in 0..height {
                        for x in 0..width {
                            let a = rounded_coverage(x as i64, y as i64, width, height, radius);
                            assert_eq!(
                                a,
                                rounded_coverage(
                                    (width - x - 1) as i64,
                                    y as i64,
                                    width,
                                    height,
                                    radius
                                )
                            );
                            assert_eq!(
                                a,
                                rounded_coverage(
                                    x as i64,
                                    (height - y - 1) as i64,
                                    width,
                                    height,
                                    radius
                                )
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(rounded_coverage(-1, 0, 20, 20, 4), 0);
        assert_eq!(rounded_coverage(20, 0, 20, 20, 4), 0);
        assert_eq!(rounded_coverage(0, 0, 20, 20, 0), 255);
    }
    #[test]
    fn font_hash() {
        assert_eq!(format!("{:x}", Sha256::digest(FONT_BYTES)), FONT_SHA256);
        assert_eq!(
            format!("{:x}", Sha256::digest(UI_FONT_BYTES)),
            UI_FONT_SHA256
        );
    }
    /// DejaVu is subset (see `assets/build-fonts.py`) and the Noto fallback faces
    /// cover the scripts listed in `cw_scene::text`, CJK and emoji; a codepoint outside
    /// all of them has nowhere left to go. It must then draw `.notdef` — a visible
    /// hollow box — rather than nothing at all, because a glyph that silently renders
    /// as blank is indistinguishable from a rendering bug and unreadable to an OCR
    /// consumer. (`script_tests` pins that the covered scripts draw real glyphs.)
    #[test]
    fn uncovered_codepoints_draw_a_visible_notdef_box() {
        // Tibetan, Syriac, Cherokee, Mongolian, Tifinagh: covered by no bundled face.
        for uncovered in ['\u{0F40}', '\u{0710}', '\u{13A0}', '\u{1820}', '\u{2D30}'] {
            for ui in [0u8, 1, 2] {
                let mut renderer = Renderer::new();
                let glyph = renderer.glyph(uncovered, 24, ui, Typeface::DejaVu);
                let ink: u32 = glyph.alpha.iter().map(|a| u32::from(*a)).sum();
                assert!(
                    ink > 0,
                    "U+{:04X} at ui={ui} rendered as blank, not .notdef",
                    uncovered as u32
                );
                // Every uncovered codepoint maps to glyph 0, so they are the
                // same mark: predictable, not merely non-empty.
                let other = renderer.glyph('\u{1401}', 24, ui, Typeface::DejaVu);
                assert_eq!(
                    glyph.alpha, other.alpha,
                    "U+{:04X} at ui={ui} is not the shared .notdef",
                    uncovered as u32
                );
            }
        }
        // A covered codepoint must still be its own glyph, or the assertion
        // above would pass with the whole face replaced by boxes.
        let mut renderer = Renderer::new();
        let lambda = renderer.glyph('\u{03BB}', 24, 1, Typeface::DejaVu);
        let notdef = renderer.glyph('\u{13A0}', 24, 1, Typeface::DejaVu);
        assert_ne!(lambda.alpha, notdef.alpha, "λ must not be .notdef");
    }
    #[test]
    fn structured_scene_does_not_load_font() {
        let s = sample();
        assert_eq!(s.nodes.len(), 2);
        assert!(s.validate().is_ok());
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use cw_scene::{PatchOp, ScenePatch};
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(12))]
        #[test]
        fn moved_alpha_layers_incremental_equal_full(changes in prop::collection::vec((0u64..8,-10i32..60,-10i32..60,0u8..255),1..20)) {
            let mut scene=Scene::new(64,64);let mut renderer=Renderer::new();renderer.render(&scene);
            for (id,x,y,alpha) in changes {
                let n=Node::rectangle(id,Rect::new(x,y,20,20),Color(20,80,160,alpha));
                let damage=scene.patch(ScenePatch{base_revision:scene.revision,revision:scene.revision+1,operations:vec![PatchOp::Upsert(n)]}).unwrap();
                let incremental=renderer.render_incremental(&scene,&damage).clone();
                let full=renderer.render(&scene);
                prop_assert_eq!(incremental,full);
            }
        }
    }
}

/// Three separable integer box passes approximate a Gaussian without host math.
fn shadow_mask(width: u32, height: u32, radius: u32, blur: u32) -> Mask {
    let mut alpha = vec![0; width as usize * height as usize];
    let inner_w = width.saturating_sub(blur * 2);
    let inner_h = height.saturating_sub(blur * 2);
    for y in 0..height {
        for x in 0..width {
            alpha[(y * width + x) as usize] = rounded_coverage(
                x as i64 - blur as i64,
                y as i64 - blur as i64,
                inner_w,
                inner_h,
                radius,
            );
        }
    }
    let r = blur.div_ceil(3) as usize;
    if r > 0 && width > 0 && height > 0 {
        let mut tmp = vec![0; alpha.len()];
        for _ in 0..3 {
            for y in 0..height as usize {
                let row = y * width as usize;
                let mut sum = (0..=r.min(width as usize - 1))
                    .map(|x| alpha[row + x] as u32)
                    .sum::<u32>();
                for x in 0..width as usize {
                    tmp[row + x] = ((sum + r as u32) / (r as u32 * 2 + 1)) as u8;
                    if x >= r {
                        sum -= alpha[row + x - r] as u32;
                    }
                    if x + r + 1 < width as usize {
                        sum += alpha[row + x + r + 1] as u32;
                    }
                }
            }
            for x in 0..width as usize {
                let mut sum = (0..=r.min(height as usize - 1))
                    .map(|y| tmp[y * width as usize + x] as u32)
                    .sum::<u32>();
                for y in 0..height as usize {
                    alpha[y * width as usize + x] = ((sum + r as u32) / (r as u32 * 2 + 1)) as u8;
                    if y >= r {
                        sum -= tmp[(y - r) * width as usize + x] as u32;
                    }
                    if y + r + 1 < height as usize {
                        sum += tmp[(y + r + 1) * width as usize + x] as u32;
                    }
                }
            }
        }
    }
    Mask {
        width,
        height,
        alpha,
        color: Vec::new(),
        spans: Vec::new(),
    }
}

#[cfg(test)]
mod desktop_asset_tests {
    use super::*;

    #[test]
    fn bundled_assets_decode_and_share_across_world_renderers() {
        for id in ASSET_IDS {
            let frame = assets::decode(id).expect(id);
            assert!(frame.width > 0 && frame.height > 0, "{id}");
            assert_eq!(
                frame.rgba.len(),
                frame.width as usize * frame.height as usize * 4
            );
            assert!(Arc::ptr_eq(&frame, &assets::decode(id).unwrap()));
        }
        assert!(Arc::ptr_eq(
            &assets::decode("icon/ios/editor").unwrap(),
            &assets::decode("icon/ios/docs").unwrap()
        ));
        assert!(assets::decode("https://example.org/image.png").is_none());
        assert!(assets::decode("/etc/passwd").is_none());
    }

    #[test]
    fn assets_shadows_bold_are_deterministic_and_incremental() {
        let mut scene = Scene::new(160, 120);
        scene.nodes.push(Node::asset(
            1,
            Rect::new(0, 0, 160, 120),
            "wallpaper/windows",
        ));
        scene.nodes.push(Node::new(
            2,
            Rect::new(20, 15, 120, 90),
            Primitive::Shadow {
                color: Color(0, 0, 0, 140),
                radius: 10,
                blur: 12,
            },
        ));
        scene.nodes.push(Node::rounded_rectangle(
            3,
            Rect::new(32, 27, 96, 66),
            Color::WHITE,
            10,
        ));
        scene.nodes.push(Node::asset(
            4,
            Rect::new(40, 40, 30, 30),
            "icon/windows/files",
        ));
        scene.nodes.push(Node::ui_text_bold(
            5,
            Rect::new(74, 40, 50, 30),
            "Files",
            13,
            Color::BLACK,
        ));
        let first = Renderer::new().render(&scene);
        let mut renderer = Renderer::new();
        assert_eq!(first, renderer.render(&scene));
        scene.nodes[4] = Node::ui_text_bold(5, Rect::new(74, 40, 50, 30), "Open", 13, Color::BLACK);
        let incremental = renderer
            .render_incremental(
                &scene,
                &Damage {
                    rects: vec![Rect::new(74, 40, 50, 30)],
                },
            )
            .clone();
        assert_eq!(incremental, Renderer::new().render(&scene));
        assert_eq!(renderer.shadows.len(), 1);
        assert_eq!(renderer.assets.len(), 2);
    }

    #[test]
    fn shadow_is_soft_symmetric_and_bounded() {
        let mask = shadow_mask(80, 60, 6, 9);
        let at = |x: usize, y: usize| mask.alpha[y * 80 + x];
        assert!(at(40, 30) > at(40, 4));
        assert!(at(40, 4) > 0);
        for y in 0..60 {
            for x in 0..80 {
                assert_eq!(at(x, y), at(79 - x, y));
            }
        }
    }
}

/// Fixed-point bilinear sampling of premultiplied colors avoids dark alpha fringes.
/// Coverage-weighted area resampling in 1/256 source pixels: a box filter when
/// shrinking (no aliasing at dock and status sizes) and a smooth step when enlarging.
fn resample_icon(source: &Frame, width: u32, height: u32) -> Frame {
    // Per output cell: the source span it covers, as (first index, weights).
    let spans = |from: u32, to: u32| -> Vec<(u32, Vec<u64>)> {
        (0..to as u64)
            .map(|o| {
                let start = o * from as u64 * 256 / to as u64;
                let end = ((o + 1) * from as u64 * 256 / to as u64).max(start + 1);
                let first = (start / 256) as u32;
                let last = (((end - 1) / 256) as u32).min(from - 1);
                let weights = (first..=last)
                    .map(|i| (end.min((i as u64 + 1) * 256)) - start.max(i as u64 * 256))
                    .collect();
                (first, weights)
            })
            .collect()
    };
    let columns = spans(source.width, width);
    let rows = spans(source.height, height);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for (y0, wy) in &rows {
        for (x0, wx) in &columns {
            let mut a = 0u64;
            let mut c = [0u64; 3];
            let mut total = 0u64;
            for (dy, wy) in wy.iter().enumerate() {
                for (dx, wx) in wx.iter().enumerate() {
                    let weight = wx * wy;
                    let i = (((y0 + dy as u32) * source.width + x0 + dx as u32) * 4) as usize;
                    let alpha = source.rgba[i + 3] as u64 * weight;
                    total += weight;
                    a += alpha;
                    for (channel, sum) in c.iter_mut().enumerate() {
                        *sum += source.rgba[i + channel] as u64 * alpha;
                    }
                }
            }
            for sum in c {
                rgba.push((sum + a / 2).checked_div(a).unwrap_or(0) as u8);
            }
            rgba.push(((a + total / 2) / total.max(1)) as u8);
        }
    }
    Frame {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod fidelity_tests {
    use super::*;
    use cw_scene::{PatchOp, RoundedClip, ScenePatch};

    fn glass_scene() -> Scene {
        let mut s = Scene::new(220, 160);
        s.background = Color::rgb(20, 40, 90);
        for i in 0..8 {
            s.nodes.push(Node::rectangle(
                i + 1,
                Rect::new(i as i32 * 27, 10 + i as i32 * 9, 20, 90),
                Color::rgb(250, (i * 30) as u8, 40),
            ));
        }
        let mut glass = Node::new(
            50,
            Rect::new(40, 40, 120, 70),
            Primitive::Backdrop {
                radius: 14,
                blur: 6,
            },
        );
        glass.z = 5;
        s.nodes.push(glass);
        let mut tint =
            Node::rounded_rectangle(51, Rect::new(40, 40, 120, 70), Color(255, 255, 255, 90), 14);
        tint.z = 6;
        s.nodes.push(tint);
        let mut label = Node::ui_text(
            52,
            Rect::new(52, 52, 100, 40),
            "Control Centre",
            13,
            Color::BLACK,
        );
        label.z = 7;
        s.nodes.push(label);
        s
    }
    #[test]
    fn backdrop_blurs_lower_layers_and_incremental_equals_full() {
        let mut scene = glass_scene();
        let mut renderer = Renderer::new();
        let frame = renderer.render(&scene);
        // Inside the glass a hard bar edge becomes a gradient; outside it stays crisp.
        let row = |y: u32| {
            (60..100)
                .map(|x| frame.pixel(x, y).unwrap()[1])
                .collect::<Vec<_>>()
        };
        let distinct = |v: Vec<u8>| {
            v.into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        };
        assert!(distinct(row(75)) > 6);
        assert!(distinct(row(30)) <= 4);
        assert_eq!(frame, Renderer::new().render(&scene));
        // Moving a bar beneath, beside, and far from the glass must match a full repaint.
        for (id, x, y) in [(3u64, 70, 20), (1, 8, 60), (8, 195, 120), (3, 100, 45)] {
            let mut node = scene.nodes.iter().find(|n| n.id == id).unwrap().clone();
            node.bounds.x = x;
            node.bounds.y = y;
            let revision = scene.revision + 1;
            let damage = scene
                .patch(ScenePatch {
                    base_revision: scene.revision,
                    revision,
                    operations: vec![PatchOp::Upsert(node)],
                })
                .unwrap();
            assert_eq!(
                renderer.render_incremental(&scene, &damage),
                &Renderer::new().render(&scene)
            );
        }
    }
    #[test]
    fn rounded_clip_antialiases_corners_and_matches_incrementally() {
        let mut scene = Scene::new(120, 90);
        scene.background = Color::BLACK;
        let clip = RoundedClip {
            rect: Rect::new(10, 10, 100, 70),
            radius: 12,
        };
        let mut content = Node::rectangle(1, Rect::new(10, 30, 100, 50), Color::WHITE);
        content.rounded_clip = Some(clip);
        scene.nodes.push(content);
        let mut text = Node::ui_text(
            2,
            Rect::new(11, 62, 90, 18),
            "gjpqy",
            14,
            Color::rgb(200, 0, 0),
        );
        text.rounded_clip = Some(clip);
        scene.nodes.push(text);
        let mut renderer = Renderer::new();
        let frame = renderer.render(&scene);
        assert_eq!(frame.pixel(10, 79).unwrap(), [0, 0, 0, 255]);
        assert_eq!(frame.pixel(10, 40).unwrap(), [255, 255, 255, 255]);
        assert_eq!(frame.pixel(60, 79).unwrap(), [255, 255, 255, 255]);
        let edge = frame.pixel(13, 76).unwrap()[0];
        assert!(edge > 0 && edge < 255, "corner must be antialiased: {edge}");
        assert!(scene.hit_test(10, 79).is_none());
        let damage = Damage {
            rects: vec![Rect::new(0, 60, 40, 30)],
        };
        assert_eq!(renderer.render_incremental(&scene, &damage), &frame);
    }
    #[test]
    fn paths_are_antialiased_with_crisp_axis_aligned_strokes() {
        let mut scene = Scene::new(64, 64);
        let path = |id, points: Vec<(i32, i32)>, closed: bool| {
            Node::new(
                id,
                Rect::new(0, 0, 64, 64),
                Primitive::Path {
                    points,
                    fill: closed.then_some(Color::BLACK),
                    stroke: (!closed).then_some(Color::BLACK),
                    stroke_width: 1,
                    closed,
                },
            )
        };
        scene.nodes.push(path(1, vec![(4, 10), (40, 10)], false));
        scene.nodes.push(path(2, vec![(4, 20), (30, 33)], false));
        scene
            .nodes
            .push(path(3, vec![(40, 40), (60, 44), (44, 60)], true));
        let frame = Renderer::new().render(&scene);
        assert_eq!(frame.pixel(20, 10).unwrap(), [0, 0, 0, 255]);
        assert_eq!(frame.pixel(20, 9).unwrap(), [255, 255, 255, 255]);
        assert_eq!(frame.pixel(20, 11).unwrap(), [255, 255, 255, 255]);
        let grey = |x0: u32, x1: u32, y0: u32, y1: u32| {
            (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .filter(|&(x, y)| !matches!(frame.pixel(x, y).unwrap()[0], 0 | 255))
                .count()
        };
        assert!(grey(4, 31, 19, 35) > 10, "diagonal stroke is antialiased");
        assert!(grey(38, 62, 38, 62) > 10, "polygon edge is antialiased");
        assert_eq!(frame.pixel(47, 47).unwrap(), [0, 0, 0, 255]);
    }
    #[test]
    fn platform_typefaces_render_distinctly_with_fallback_glyphs() {
        let mut frames = Vec::new();
        for typeface in Typeface::ALL.into_iter().filter(|t| *t != Typeface::Mono) {
            let mut scene = Scene::new(260, 60);
            scene.typeface = typeface;
            scene.nodes.push(Node::ui_text(
                1,
                Rect::new(4, 4, 250, 24),
                "Settings λ 09:41",
                15,
                Color::BLACK,
            ));
            scene.nodes.push(Node::ui_text_bold(
                2,
                Rect::new(4, 30, 250, 24),
                "Quick Settings",
                15,
                Color::BLACK,
            ));
            let frame = Renderer::new().render(&scene);
            assert_eq!(frame, Renderer::new().render(&scene));
            let json = serde_json::to_string(&scene).unwrap();
            assert_eq!(json.contains("typeface"), typeface != Typeface::DejaVu);
            assert_eq!(serde_json::from_str::<Scene>(&json).unwrap(), scene);
            assert!(!frames.contains(&frame), "{typeface:?}");
            frames.push(frame);
        }
    }
    /// Two nodes of one scene set in different families draw in their own faces and
    /// measure with their own tables; a node naming none keeps the scene's, and its
    /// JSON (and so its digest) is what it was before the field existed.
    #[test]
    fn a_node_can_name_its_own_typeface() {
        let text = "Weights and measures";
        let mut scene = Scene::new(300, 80);
        scene.typeface = Typeface::Inter;
        scene.nodes.push(Node::new(
            1,
            Rect::new(4, 4, 290, 24),
            Primitive::ui_text_face(text, Color::BLACK, 16, Style::default(), Typeface::Tinos),
        ));
        scene.nodes.push(Node::new(
            2,
            Rect::new(4, 30, 290, 24),
            Primitive::ui_text_face(text, Color::BLACK, 16, Style::default(), Typeface::Poppins),
        ));
        scene.nodes.push(Node::ui_text(
            3,
            Rect::new(4, 56, 290, 24),
            text,
            16,
            Color::BLACK,
        ));
        let frame = Renderer::new().render(&scene);
        let frame = &frame;
        let row = |y0: u32| -> Vec<u8> {
            (y0..y0 + 24)
                .flat_map(|y| (0..300).map(move |x| frame.pixel(x, y).unwrap()[0]))
                .collect()
        };
        assert_ne!(row(4), row(30));
        assert_ne!(row(30), row(56));
        // The third node is the scene's Inter: identical to the same node drawn in a
        // scene whose typeface is Inter and nothing else.
        let mut plain = Scene::new(300, 80);
        plain.typeface = Typeface::Inter;
        plain.nodes.push(Node::ui_text(
            3,
            Rect::new(4, 56, 290, 24),
            text,
            16,
            Color::BLACK,
        ));
        let plain_frame = Renderer::new().render(&plain);
        let plain_frame = &plain_frame;
        let plain_row: Vec<u8> = (56..80)
            .flat_map(|y| (0..300).map(move |x| plain_frame.pixel(x, y).unwrap()[0]))
            .collect();
        assert_eq!(row(56), plain_row);
        // Each node measures with its own family.
        let widths: Vec<u32> = scene
            .nodes
            .iter()
            .map(|n| {
                metrics::text_width(
                    n.primitive.typeface().unwrap_or(scene.typeface),
                    false,
                    text,
                    16,
                )
            })
            .collect();
        assert_eq!(
            widths[0],
            metrics::text_width(Typeface::Tinos, false, text, 16)
        );
        assert_eq!(
            widths[1],
            metrics::text_width(Typeface::Poppins, false, text, 16)
        );
        assert_eq!(
            widths[2],
            metrics::text_width(Typeface::Inter, false, text, 16)
        );
        assert!(widths[0] != widths[1] && widths[1] != widths[2]);
        // Serialisation carries the field only when set.
        let json = serde_json::to_string(&scene).unwrap();
        assert!(json.contains("\"typeface\":\"tinos\""));
        assert!(json.contains("\"typeface\":\"poppins\""));
        assert_eq!(serde_json::from_str::<Scene>(&json).unwrap(), scene);
        assert!(!serde_json::to_string(&plain.nodes[0])
            .unwrap()
            .contains("typeface"));
        scene.stamp();
        plain.stamp();
        assert_eq!(scene.nodes[2].revision, plain.nodes[0].revision);
    }
    #[test]
    fn ui_text_wraps_between_words() {
        let mut scene = Scene::new(120, 80);
        scene.typeface = Typeface::Inter;
        scene.nodes.push(Node::ui_text(
            1,
            Rect::new(0, 0, 120, 80),
            "checklist after release",
            14,
            Color::BLACK,
        ));
        let frame = Renderer::new().render(&scene);
        let lines =
            cw_scene::metrics::wrap(Typeface::Inter, false, "checklist after release", 14, 120);
        assert_eq!(lines, ["checklist after ", "release"]);
        let inked = |y0: u32, y1: u32| {
            (y0..y1).any(|y| (0..120).any(|x| frame.pixel(x, y).unwrap()[0] < 128))
        };
        assert!(inked(2, 16) && inked(20, 34) && !inked(40, 80));
    }
}
