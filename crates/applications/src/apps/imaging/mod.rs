//! Image editors. One engine ([`cw_raster`]) and one editing state ([`Studio`]) sit
//! under every product; each product module is only its interface, drawn the way that
//! product draws itself and offering only what that product offers:
//!
//! | Platform | Applications |
//! |---|---|
//! | Windows 11 | Paint |
//! | macOS | Preview (Markup, Adjust Color, Adjust Size), Pixelmator Pro |
//! | Ubuntu | GIMP, Pinta |
//! | iOS | Photos' edit mode and Markup (inside Photos) |
//! | Android | Google Photos' editor (inside Photos), Sketchbook |
//!
//! A command the product would not have is refused, not quietly honoured: Paint has no
//! Gaussian blur, so `paint:dialog:gaussian-blur` is an error.
use crate::desktop_scene::DesktopTheme;
use crate::{AppEffect, PointerPhase, CLIPBOARD_IMAGE};
use cw_raster::adjust::{Adjustment, Channel, Tone};
use cw_raster::document::StrokeMode;
use cw_raster::draw::{Brush, BrushKind, Shape, ShapeKind};
use cw_raster::filter::Filter;
use cw_raster::gradient::{Gradient, GradientShape, Repeat};
use cw_raster::jpeg::Subsampling;
use cw_raster::mask::{Mask, SelectMode, P16};
use cw_raster::path::{Anchor, Path};
use cw_raster::transform::Resample;
use cw_raster::{BlendMode, Canvas, Document, IRect, Rgba};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod gimp;
pub mod look;
mod paint;
mod photo_edit;
mod pinta;
mod pixelmator;
mod preview;
mod sketchbook;
pub mod view;

pub use photo_edit::{render_photo_editor, PHOTO_EDIT_PREFIX};

/// Image files the choosers list and the editors can open.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg"];
/// Longest text a text tool will take in one go.
pub const TEXT_LIMIT: usize = 200;
/// Retained chooser listing.
const LISTING_LIMIT: usize = 256;

pub fn is_image(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Product {
    Paint,
    Preview,
    Pixelmator,
    Gimp,
    Pinta,
    Sketchbook,
    IosPhotos,
    GooglePhotos,
}
impl Product {
    /// Target namespace of this product's controls.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Paint => "paint",
            Self::Preview => "preview",
            Self::Pixelmator => "pixelmator",
            Self::Gimp => "gimp",
            Self::Pinta => "pinta",
            Self::Sketchbook => "sketchbook",
            Self::IosPhotos | Self::GooglePhotos => PHOTO_EDIT_PREFIX,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Paint => "Paint",
            Self::Preview => "Preview",
            Self::Pixelmator => "Pixelmator Pro",
            Self::Gimp => "GNU Image Manipulation Program",
            Self::Pinta => "Pinta",
            Self::Sketchbook => "Sketchbook",
            Self::IosPhotos => "Photos",
            Self::GooglePhotos => "Google Photos",
        }
    }
    /// The tools this product's interface offers.
    pub fn tools(self) -> &'static [Tool] {
        use Tool::*;
        match self {
            Self::Paint => &[
                RectSelect, Lasso, Pencil, Fill, Text, Eraser, Picker, Zoom, Brush, Airbrush, Shape,
            ],
            Self::Preview => &[
                RectSelect,
                EllipseSelect,
                Lasso,
                Pen,
                Highlighter,
                Shape,
                Text,
            ],
            Self::Pixelmator => &[
                Move,
                RectSelect,
                EllipseSelect,
                Lasso,
                MagicWand,
                Crop,
                Brush,
                Pencil,
                Eraser,
                Fill,
                Gradient,
                Clone,
                Repair,
                Shape,
                Text,
                Picker,
                Zoom,
            ],
            Self::Gimp => &[
                Move,
                RectSelect,
                EllipseSelect,
                Lasso,
                MagicWand,
                Crop,
                Paths,
                Text,
                Fill,
                Gradient,
                Brush,
                Pencil,
                Airbrush,
                Eraser,
                Clone,
                Heal,
                Picker,
                Zoom,
                Pan,
            ],
            Self::Pinta => &[
                Move,
                RectSelect,
                EllipseSelect,
                Lasso,
                MagicWand,
                Zoom,
                Pan,
                Brush,
                Pencil,
                Eraser,
                Fill,
                Gradient,
                Picker,
                Text,
                Shape,
                Clone,
            ],
            Self::Sketchbook => &[
                Pencil, Brush, Airbrush, Marker, Eraser, Fill, RectSelect, Picker,
            ],
            Self::IosPhotos => &[Pen, Marker, Pencil, Eraser, Text],
            Self::GooglePhotos => &[Pen, Highlighter, Text],
        }
    }
    /// Parameter dialogs (adjustments, filters, image operations) this product has.
    pub fn dialogs(self) -> &'static [&'static str] {
        match self {
            Self::Paint => &["resize", "color", "new-image"],
            Self::Preview => &["adjust-color", "resize", "color"],
            Self::Pixelmator => &[
                "exposure",
                "brightness-contrast",
                "hue-saturation",
                "temperature",
                "levels",
                "curves",
                "shadows-highlights",
                "color-balance",
                "threshold",
                "posterize",
                "gaussian-blur",
                "box-blur",
                "sharpen",
                "median",
                "vignette",
                "pixelate",
                "resize",
                "rotate",
                "new-image",
                "color",
            ],
            Self::Gimp => &[
                "color-balance",
                "temperature",
                "hue-saturation",
                "saturation",
                "exposure",
                "shadows-highlights",
                "brightness-contrast",
                "levels",
                "curves",
                "threshold",
                "posterize",
                "gaussian-blur",
                "median",
                "pixelate",
                "unsharp-mask",
                "noise-reduction",
                "vignette",
                "resize",
                "rotate",
                "new-image",
                "color",
            ],
            Self::Pinta => &[
                "brightness-contrast",
                "curves",
                "hue-saturation",
                "levels",
                "posterize",
                "gaussian-blur",
                "median",
                "pixelate",
                "sharpen",
                "vignette",
                "resize",
                "rotate",
                "new-image",
                "color",
            ],
            Self::Sketchbook => &["color", "new-image"],
            Self::IosPhotos | Self::GooglePhotos => &["color"],
        }
    }
    /// One-shot operations (no parameters) this product has.
    pub fn actions(self) -> &'static [&'static str] {
        match self {
            Self::Paint => &[],
            Self::Preview => &["auto-levels"],
            Self::Pixelmator => &[
                "invert",
                "grayscale",
                "auto-levels",
                "edge-detect",
                "emboss",
            ],
            Self::Gimp => &[
                "invert",
                "grayscale",
                "auto-levels",
                "edge-detect",
                "emboss",
                "sharpen",
            ],
            Self::Pinta => &[
                "invert",
                "grayscale",
                "auto-levels",
                "sepia",
                "edge-detect",
                "emboss",
            ],
            Self::Sketchbook => &[],
            Self::IosPhotos | Self::GooglePhotos => &[],
        }
    }
    pub fn has_layers(self) -> bool {
        matches!(
            self,
            Self::Paint | Self::Pixelmator | Self::Gimp | Self::Pinta | Self::Sketchbook
        )
    }
    /// Formats a save sheet offers. GIMP saves only XCF and exports the others.
    pub fn save_formats(self, export: bool) -> &'static [Format] {
        use Format::*;
        match self {
            Self::Gimp if export => &[Png, Jpeg, Bmp],
            Self::Gimp => &[Xcf],
            Self::Paint | Self::Pinta => &[Png, Jpeg, Bmp],
            Self::Preview | Self::Pixelmator => &[Png, Jpeg],
            _ => &[Png],
        }
    }
    /// Whether the Open sheet lists this file, i.e. whether the product opens it.
    pub fn opens(self, name: &str) -> bool {
        match (name.rsplit_once('.'), Format::of(name)) {
            (Some((stem, _)), Some(format)) if !stem.is_empty() => match format {
                Format::Png | Format::Jpeg => true,
                Format::Bmp => !self.mobile(),
                Format::Xcf => self == Self::Gimp,
            },
            _ => false,
        }
    }
    /// The quality a JPEG export starts at: GIMP 90, Pinta 85, Preview and Pixelmator
    /// their sliders' starting point, and Paint the fixed quality it always writes.
    pub fn default_jpeg_quality(self) -> u8 {
        match self {
            Self::Gimp => 90,
            Self::Pinta => 85,
            Self::Paint => 90,
            _ => 80,
        }
    }
    /// The modifier a click holds to set a clone source: Ctrl in GIMP and Pinta,
    /// Option on a Mac.
    pub fn source_modifier(self) -> (u8, &'static str) {
        match self {
            Self::Pixelmator | Self::Preview => (MOD_ALT, "Option"),
            _ => (MOD_CTRL, "Ctrl"),
        }
    }
    pub fn mobile(self) -> bool {
        matches!(
            self,
            Self::Sketchbook | Self::IosPhotos | Self::GooglePhotos
        )
    }
    /// What the product holds when launched on nothing: a fresh canvas, or an empty
    /// window waiting for a file.
    fn blank(self) -> Option<(u32, u32, Option<Rgba>)> {
        match self {
            Self::Paint => Some((960, 540, Some(cw_raster::WHITE))),
            Self::Pinta => Some((800, 600, Some(cw_raster::WHITE))),
            Self::Sketchbook => Some((720, 1280, Some(cw_raster::WHITE))),
            _ => None,
        }
    }
    fn default_size(self) -> u32 {
        match self {
            Self::Paint => 3,
            Self::Gimp => 20,
            Self::Pinta => 2,
            Self::Sketchbook => 8,
            Self::IosPhotos | Self::GooglePhotos => 8,
            _ => 5,
        }
    }
}

/// An editing tool. Brush-like tools paint along a drag; the rest act on press, on
/// release, or on the rectangle a drag spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    RectSelect,
    EllipseSelect,
    Lasso,
    MagicWand,
    Move,
    Crop,
    Pencil,
    Brush,
    Airbrush,
    Pen,
    Marker,
    Highlighter,
    Eraser,
    Fill,
    Text,
    Picker,
    Zoom,
    Pan,
    Shape,
    /// A colour ramp along a dragged line.
    Gradient,
    /// Clone stamp: paints with pixels from a source point set by a modifier-click.
    Clone,
    /// GIMP's Heal: clones, blended into the colour around the brush.
    Heal,
    /// Pixelmator's Repair: paint over something and it is rebuilt from its
    /// surroundings.
    Repair,
    /// GIMP's Paths tool: Bézier anchors and handles.
    Paths,
}
impl Tool {
    pub const ALL: [Tool; 24] = [
        Self::RectSelect,
        Self::EllipseSelect,
        Self::Lasso,
        Self::MagicWand,
        Self::Move,
        Self::Crop,
        Self::Pencil,
        Self::Brush,
        Self::Airbrush,
        Self::Pen,
        Self::Marker,
        Self::Highlighter,
        Self::Eraser,
        Self::Fill,
        Self::Text,
        Self::Picker,
        Self::Zoom,
        Self::Pan,
        Self::Shape,
        Self::Gradient,
        Self::Clone,
        Self::Heal,
        Self::Repair,
        Self::Paths,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::RectSelect => "select-rect",
            Self::EllipseSelect => "select-ellipse",
            Self::Lasso => "lasso",
            Self::MagicWand => "wand",
            Self::Move => "move",
            Self::Crop => "crop",
            Self::Pencil => "pencil",
            Self::Brush => "brush",
            Self::Airbrush => "airbrush",
            Self::Pen => "pen",
            Self::Marker => "marker",
            Self::Highlighter => "highlighter",
            Self::Eraser => "eraser",
            Self::Fill => "fill",
            Self::Text => "text",
            Self::Picker => "picker",
            Self::Zoom => "zoom",
            Self::Pan => "pan",
            Self::Shape => "shape",
            Self::Gradient => "gradient",
            Self::Clone => "clone",
            Self::Heal => "heal",
            Self::Repair => "repair",
            Self::Paths => "paths",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }
    /// Monochrome glyph for the tool button.
    pub fn symbol(self) -> &'static str {
        match self {
            Self::RectSelect => "select-rect",
            Self::EllipseSelect => "select-ellipse",
            Self::Lasso => "lasso",
            Self::MagicWand => "wand",
            Self::Move => "move",
            Self::Crop => "crop",
            Self::Pencil => "pencil",
            Self::Brush | Self::Airbrush => "brush",
            Self::Pen => "pencil",
            Self::Marker => "marker",
            Self::Highlighter => "highlighter",
            Self::Eraser => "eraser",
            Self::Fill => "bucket",
            Self::Text => "text-tool",
            Self::Picker => "eyedropper",
            Self::Zoom => "zoom-in",
            Self::Pan => "hand",
            Self::Shape => "shapes",
            Self::Gradient => "contrast",
            Self::Clone => "stamp",
            Self::Heal | Self::Repair => "magic",
            Self::Paths => "edit",
        }
    }
    /// Brushes that copy or rebuild pixels rather than lay colour.
    pub fn retouches(self) -> bool {
        matches!(self, Self::Clone | Self::Heal | Self::Repair)
    }
    /// Tools with a round tip whose outline follows the pointer over the canvas.
    pub fn has_tip(self) -> bool {
        self.paints() || self.retouches()
    }
    pub fn paints(self) -> bool {
        matches!(
            self,
            Self::Pencil
                | Self::Brush
                | Self::Airbrush
                | Self::Pen
                | Self::Marker
                | Self::Highlighter
                | Self::Eraser
        )
    }
    /// Tools whose drag spans a rectangle (or, for a gradient, a line).
    fn spans(self) -> bool {
        matches!(
            self,
            Self::RectSelect
                | Self::EllipseSelect
                | Self::Crop
                | Self::Shape
                | Self::Move
                | Self::Gradient
        )
    }
}

/// A floating surface over the editor: a menu, a parameter dialog, or a file sheet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Panel {
    Menu {
        id: String,
    },
    Dialog {
        id: String,
        values: BTreeMap<String, i32>,
    },
    /// Choose an image to open. `entries` is the folder's real listing.
    Open {
        folder: String,
        entries: Vec<String>,
        loading: bool,
    },
    /// Name the file a save writes. Typing goes to `name`; its extension is the format.
    Save {
        folder: String,
        name: String,
        entries: Vec<String>,
        /// GIMP's Export As (PNG, JPEG, BMP) rather than Save As (XCF).
        #[serde(default)]
        export: bool,
    },
}

/// Image file formats the editors read and write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Png,
    Jpeg,
    Bmp,
    /// GIMP's native layered format.
    Xcf,
}
impl Format {
    pub fn of(path: &str) -> Option<Self> {
        let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match ext.as_str() {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "bmp" => Self::Bmp,
            "xcf" => Self::Xcf,
            _ => return None,
        })
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Bmp => "bmp",
            Self::Xcf => "xcf",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Bmp => "BMP",
            Self::Xcf => "XCF",
        }
    }
    /// Formats the environment decodes; the rest are read as bytes and decoded here.
    fn decoded_by_environment(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg)
    }
}

/// Clone and heal: where the source is, and how it follows the brush.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retouch {
    /// The source point set by the modifier-click, in image pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<(i32, i32)>,
    /// Aligned cloning keeps the offset of the first stroke after the source was set;
    /// otherwise every stroke starts again from the source point.
    #[serde(default)]
    pub aligned: bool,
    /// The offset an aligned clone keeps (source minus where its first stroke began).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<(i32, i32)>,
    /// The offset of the stroke under way, so the source marker follows the brush.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<(i32, i32)>,
}

/// Gradient tool options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GradientOptions {
    pub shape: GradientShape,
    pub repeat: Repeat,
    /// FG to Transparent rather than FG to BG.
    pub transparent: bool,
    pub reverse: bool,
}

/// A curve being shaped before it is drawn: Paint's Curve (a line, then two bends) or
/// Pinta's Line/Curve (a spline through control points that stays editable).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurveEdit {
    /// Paint: start, first control, second control, end. Pinta: the control points.
    pub points: Vec<P16>,
    /// Paint: how many bends have been placed (0, 1).
    pub bends: u8,
}

/// Modifier bits carried by a pointer press.
pub const MOD_CTRL: u8 = 1;
pub const MOD_ALT: u8 = 2;
pub const MOD_SHIFT: u8 = 4;
pub const MOD_META: u8 = 8;

fn is_zero(v: &u8) -> bool {
    *v == 0
}
fn yes() -> bool {
    true
}

/// A pointer drag in progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Gesture {
    /// A brush stroke; the stroke itself lives in the document.
    Paint,
    /// A rectangle between two image points (selection, shape, crop, move).
    Span {
        tool: Tool,
        start: P16,
        end: P16,
    },
    Lasso {
        points: Vec<P16>,
    },
    /// Pinta's Freeform Shape, drawn as the pointer goes.
    Freehand {
        points: Vec<P16>,
    },
    /// A point of the curve being shaped, held by the pointer.
    CurvePoint {
        index: usize,
    },
    /// A path anchor (`which` 0) or its incoming (1) or outgoing (2) handle; `fresh`
    /// while the drag that placed the anchor pulls out its handles.
    Anchor {
        index: usize,
        which: u8,
        fresh: bool,
    },
    /// Hand tool: where the drag began, and the scroll then.
    Pan {
        x: i32,
        y: i32,
        scroll: (i32, i32),
    },
    Slider {
        param: String,
        width: i32,
    },
    /// A phone's scrubbing dial: moves the value by the drag distance.
    Dial {
        param: String,
        x: i32,
        value: i32,
    },
    /// A tone curve graph: which of its five points is held.
    Curve {
        width: i32,
        height: i32,
        point: usize,
    },
    Scroll {
        horizontal: bool,
        track: i32,
    },
}

/// Text being typed on the canvas, and where.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEntry {
    pub x: i32,
    pub y: i32,
    pub text: String,
    /// Sent to be rasterised; waiting for the glyph coverage to come back.
    #[serde(default)]
    pub pending: bool,
}

/// The editing state every product shares.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Studio {
    pub product: Product,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<Document>,
    /// File the document came from or was last saved to; empty when untitled.
    pub path: String,
    /// A file asked for and not yet decoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading: Option<String>,
    pub modified: bool,
    pub tool: Tool,
    /// The tool to return to after a one-shot pick (Paint's colour picker does this).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<Tool>,
    pub primary: Rgba,
    pub secondary: Rgba,
    /// Which of Color 1 / Color 2 a palette click sets (Paint).
    pub slot: u8,
    /// Brush diameter / line width in pixels.
    pub size: u32,
    pub hardness: u8,
    /// Tool opacity, percent.
    pub opacity: u8,
    /// Fill and magic-wand tolerance, percent.
    pub tolerance: u8,
    pub antialias: bool,
    pub shape: ShapeKind,
    pub outline: bool,
    pub fill: bool,
    pub select_mode: SelectMode,
    /// Sample the merged image rather than the active layer (fill, wand, picker).
    pub merged: bool,
    pub font_size: u16,
    pub bold: bool,
    /// Percent; 0 fits the image to the window.
    pub zoom: u32,
    /// Top-left of the view in image pixels, when the image is larger than the view.
    pub scroll: (i32, i32),
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gesture: Option<Gesture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<IRect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<Panel>,
    /// Side panels a product shows or hides (Paint's Layers pane, iOS Markup).
    #[serde(default)]
    pub layers_open: bool,
    /// The last thing that happened worth telling: saved, refused, failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Parameters of a phone editor's non-destructive look, and of the active tab.
    #[serde(default)]
    pub look: BTreeMap<String, i32>,
    /// Which part of a phone editor is showing (adjust, filters, crop, markup…).
    #[serde(default)]
    pub tab: String,
    /// The adjustment a phone editor's dial is bound to.
    #[serde(default)]
    pub focus: String,
    /// Where new files are saved when the document has no folder of its own.
    #[serde(default)]
    pub folder: String,
    /// Where the pointer is over the image (sub16), from hover and drag events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover: Option<P16>,
    /// Modifier keys held for the pointer press being handled (`MOD_*` bits).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub modifiers: u8,
    #[serde(default)]
    pub retouch: Retouch,
    #[serde(default)]
    pub gradient: GradientOptions,
    /// GIMP's active path, being drawn with the Paths tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_edit: Option<Path>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<CurveEdit>,
    /// GIMP's dialogs' Preview checkbox.
    #[serde(default = "yes")]
    pub preview: bool,
    /// JPEG quality last chosen (0: the product's default).
    #[serde(default)]
    pub jpeg_quality: u8,
    #[serde(default)]
    pub subsampling: Subsampling,
    /// GIMP's "Save using better but slower compression" (zlib tiles).
    #[serde(default)]
    pub xcf_zlib: bool,
    /// A file named in the save sheet, waiting on its format's options dialog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<String>,
}

/// A `P16` from integer image pixel centres.
pub fn centre(x: i32, y: i32) -> P16 {
    (i64::from(x) * 16 + 8, i64::from(y) * 16 + 8)
}
fn pixel_of(p: P16) -> (i32, i32) {
    (p.0.div_euclid(16) as i32, p.1.div_euclid(16) as i32)
}
fn folder_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some(("", _)) => "/",
        Some((folder, _)) => folder,
        None => "",
    }
}
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{}/{name}", folder.trim_end_matches('/'))
    }
}
fn parse_i32(v: &str) -> Result<i32, String> {
    v.parse().map_err(|_| format!("{v} is not a number"))
}

/// Zoom steps every desktop editor walks through.
pub const ZOOM_STEPS: [u32; 12] = [10, 25, 33, 50, 67, 100, 150, 200, 300, 400, 600, 800];

/// Specification of a parameter: id, lowest, highest, default.
pub type Param = (&'static str, i32, i32, i32);

/// The parameters of a dialog, in the order its controls are drawn.
pub fn dialog_params(id: &str) -> &'static [Param] {
    match id {
        "brightness-contrast" => &[("brightness", -100, 100, 0), ("contrast", -100, 100, 0)],
        "exposure" => &[("exposure", -300, 300, 0)],
        "levels" => &[
            ("black", 0, 254, 0),
            ("gamma", 10, 999, 100),
            ("white", 1, 255, 255),
        ],
        "jpeg" => &[("quality", 0, 100, 90), ("subsampling", 0, 1, 0)],
        "jpeg-quality" => &[("quality", 1, 100, 85)],
        "stroke-path" => &[("line-width", 1, 500, 6), ("antialias", 0, 1, 1)],
        "curves" => &[
            ("channel", 0, 3, 0),
            ("c0", 0, 255, 0),
            ("c1", 0, 255, 64),
            ("c2", 0, 255, 128),
            ("c3", 0, 255, 192),
            ("c4", 0, 255, 255),
        ],
        "hue-saturation" => &[
            ("hue", -180, 180, 0),
            ("saturation", -100, 100, 0),
            ("lightness", -100, 100, 0),
        ],
        "saturation" => &[("saturation", -100, 100, 0)],
        "color-balance" => &[
            ("range", 0, 2, 1),
            ("cyan-red", -100, 100, 0),
            ("magenta-green", -100, 100, 0),
            ("yellow-blue", -100, 100, 0),
        ],
        "temperature" => &[("temperature", -100, 100, 0), ("tint", -100, 100, 0)],
        "shadows-highlights" => &[("shadows", -100, 100, 0), ("highlights", -100, 100, 0)],
        "threshold" => &[("low", 0, 255, 127), ("high", 0, 255, 255)],
        "posterize" => &[("levels", 2, 64, 4)],
        "gaussian-blur" => &[("radius", 1, 100, 3)],
        "box-blur" => &[("radius", 1, 100, 3)],
        "sharpen" => &[("amount", 1, 100, 50)],
        "unsharp-mask" => &[
            ("radius", 1, 50, 3),
            ("amount", 0, 500, 50),
            ("threshold", 0, 255, 0),
        ],
        "median" | "noise-reduction" => &[("radius", 1, 10, 2)],
        "pixelate" => &[("size", 2, 128, 10)],
        "vignette" => &[("amount", -100, 100, 50)],
        "resize" => &[
            ("width", 1, 8192, 0),
            ("height", 1, 8192, 0),
            ("ratio", 0, 1, 1),
            ("resample", 0, 2, 2),
        ],
        "rotate" => &[("angle", -180, 180, 0), ("expand", 0, 1, 1)],
        "new-image" => &[
            ("width", 1, 8192, 1024),
            ("height", 1, 8192, 768),
            ("background", 0, 1, 1),
        ],
        "color" => &[
            ("r", 0, 255, 0),
            ("g", 0, 255, 0),
            ("b", 0, 255, 0),
            ("slot", 0, 1, 0),
        ],
        // Preview's Adjust Color panel, applied together.
        "adjust-color" => &[
            ("exposure", -100, 100, 0),
            ("contrast", -100, 100, 0),
            ("highlights", -100, 100, 0),
            ("shadows", -100, 100, 0),
            ("saturation", -100, 100, 0),
            ("temperature", -100, 100, 0),
            ("tint", -100, 100, 0),
            ("sepia", 0, 100, 0),
            ("sharpness", 0, 100, 0),
        ],
        _ => &[],
    }
}

/// Tool-option parameters: id, lowest, highest.
fn option_range(id: &str) -> Option<(i32, i32)> {
    Some(match id {
        "size" => (1, 500),
        "hardness" | "opacity" | "tolerance" | "layer-opacity" => (0, 100),
        "font-size" => (6, 400),
        "jpeg-quality" => (1, 100),
        "zoom" => (10, 800),
        _ => return None,
    })
}

/// What a dialog's values do to the document.
enum Operation {
    Adjust(Vec<Adjustment>),
    Filter(Filter),
    Resize(u32, u32, Resample),
    Rotate(i32, bool),
    New(u32, u32, bool),
    Color(Rgba, u8),
    /// Write the pending JPEG at this quality and subsampling.
    Jpeg(u8, Subsampling),
    /// Stroke the active path this wide, antialiased or not.
    StrokePath(u32, bool),
}
fn operation(id: &str, v: &BTreeMap<String, i32>) -> Option<Operation> {
    let g = |k: &str| v.get(k).copied().unwrap_or(0);
    Some(match id {
        "brightness-contrast" => Operation::Adjust(vec![Adjustment::BrightnessContrast {
            brightness: g("brightness"),
            contrast: g("contrast"),
        }]),
        "exposure" => Operation::Adjust(vec![Adjustment::Exposure {
            stops: g("exposure"),
        }]),
        "levels" => Operation::Adjust(vec![Adjustment::Levels {
            channel: Channel::Value,
            in_black: g("black").clamp(0, 254) as u8,
            in_white: g("white").clamp(1, 255) as u8,
            gamma: g("gamma").clamp(10, 999) as u32,
            out_black: 0,
            out_white: 255,
        }]),
        "jpeg" | "jpeg-quality" => Operation::Jpeg(
            g("quality").clamp(1, 100) as u8,
            if g("subsampling") == 1 {
                Subsampling::Quartered
            } else {
                Subsampling::Full
            },
        ),
        "stroke-path" => Operation::StrokePath(g("line-width").max(1) as u32, g("antialias") != 0),
        "curves" => Operation::Adjust(vec![Adjustment::Curves {
            channel: Channel::ALL[g("channel").clamp(0, 3) as usize],
            points: (0..5)
                .map(|i| {
                    (
                        (i * 64).min(255) as u8,
                        g(&format!("c{i}")).clamp(0, 255) as u8,
                    )
                })
                .collect(),
        }]),
        "hue-saturation" => Operation::Adjust(vec![Adjustment::HueSaturation {
            hue: g("hue"),
            saturation: g("saturation"),
            lightness: g("lightness"),
        }]),
        "saturation" => Operation::Adjust(vec![Adjustment::HueSaturation {
            hue: 0,
            saturation: g("saturation"),
            lightness: 0,
        }]),
        "color-balance" => Operation::Adjust(vec![Adjustment::ColorBalance {
            tone: match g("range") {
                0 => Tone::Shadows,
                2 => Tone::Highlights,
                _ => Tone::Midtones,
            },
            cyan_red: g("cyan-red"),
            magenta_green: g("magenta-green"),
            yellow_blue: g("yellow-blue"),
        }]),
        "temperature" => Operation::Adjust(vec![Adjustment::Temperature {
            temperature: g("temperature"),
            tint: g("tint"),
        }]),
        "shadows-highlights" => Operation::Adjust(vec![Adjustment::ShadowsHighlights {
            shadows: g("shadows"),
            highlights: g("highlights"),
        }]),
        "threshold" => Operation::Adjust(vec![Adjustment::Threshold {
            low: g("low").clamp(0, 255) as u8,
            high: g("high").clamp(0, 255) as u8,
        }]),
        "posterize" => Operation::Adjust(vec![Adjustment::Posterize {
            levels: g("levels").clamp(2, 64) as u8,
        }]),
        "adjust-color" => {
            let mut out = vec![];
            if g("exposure") != 0 {
                out.push(Adjustment::Exposure {
                    stops: g("exposure") * 2,
                });
            }
            if g("contrast") != 0 {
                out.push(Adjustment::BrightnessContrast {
                    brightness: 0,
                    contrast: g("contrast") / 2,
                });
            }
            if g("highlights") != 0 || g("shadows") != 0 {
                out.push(Adjustment::ShadowsHighlights {
                    shadows: g("shadows"),
                    highlights: g("highlights"),
                });
            }
            if g("saturation") != 0 {
                out.push(Adjustment::HueSaturation {
                    hue: 0,
                    saturation: g("saturation"),
                    lightness: 0,
                });
            }
            if g("temperature") != 0 || g("tint") != 0 {
                out.push(Adjustment::Temperature {
                    temperature: g("temperature"),
                    tint: g("tint"),
                });
            }
            if g("sepia") != 0 {
                out.push(Adjustment::Sepia { amount: g("sepia") });
            }
            // Sharpness is spatial: `apply_dialog` runs it as a filter after the colour.
            Operation::Adjust(out)
        }
        "gaussian-blur" => Operation::Filter(Filter::GaussianBlur {
            radius: g("radius").max(1) as u32,
        }),
        "box-blur" => Operation::Filter(Filter::BoxBlur {
            radius: g("radius").max(1) as u32,
        }),
        "sharpen" => Operation::Filter(Filter::Sharpen {
            amount: g("amount").max(0) as u32,
        }),
        "unsharp-mask" => Operation::Filter(Filter::UnsharpMask {
            radius: g("radius").max(1) as u32,
            amount: g("amount").max(0) as u32,
            threshold: g("threshold").clamp(0, 255) as u8,
        }),
        "median" | "noise-reduction" => Operation::Filter(Filter::Median {
            radius: g("radius").max(1) as u32,
        }),
        "pixelate" => Operation::Filter(Filter::Pixelate {
            size: g("size").max(2) as u32,
        }),
        "vignette" => Operation::Filter(Filter::Vignette {
            amount: g("amount"),
        }),
        "resize" => Operation::Resize(
            g("width").max(1) as u32,
            g("height").max(1) as u32,
            match g("resample") {
                0 => Resample::Nearest,
                1 => Resample::Bilinear,
                _ => Resample::Bicubic,
            },
        ),
        "rotate" => Operation::Rotate(g("angle") * 100, g("expand") != 0),
        "new-image" => Operation::New(
            g("width").max(1) as u32,
            g("height").max(1) as u32,
            g("background") != 0,
        ),
        "color" => Operation::Color(
            [
                g("r").clamp(0, 255) as u8,
                g("g").clamp(0, 255) as u8,
                g("b").clamp(0, 255) as u8,
                255,
            ],
            g("slot").clamp(0, 1) as u8,
        ),
        _ => return None,
    })
}

/// Run a dialog's adjustments or filter on `layer` through `selection`: what OK does,
/// and what the canvas previews while the dialog is open. `None` for dialogs that do
/// not change pixels of the layer (resize, colours, export options).
fn run_dialog(
    id: &str,
    values: &BTreeMap<String, i32>,
    layer: &mut Canvas,
    selection: Option<&Mask>,
) -> Option<()> {
    match operation(id, values)? {
        Operation::Adjust(list) => {
            for adj in &list {
                cw_raster::adjust::apply(layer, adj, selection);
            }
        }
        Operation::Filter(f) => {
            cw_raster::filter::apply(layer, &f, selection);
        }
        _ => return None,
    }
    // Preview's sharpness rides along with its colour sliders.
    if id == "adjust-color" && values.get("sharpness").copied().unwrap_or(0) > 0 {
        let amount = values["sharpness"] as u32;
        cw_raster::filter::apply(layer, &Filter::Sharpen { amount }, selection);
    }
    Some(())
}

/// One-shot operations by id.
fn action(id: &str) -> Option<(&'static str, Result<Adjustment, Filter>)> {
    Some(match id {
        "invert" => ("Invert", Ok(Adjustment::Invert)),
        "grayscale" => ("Desaturate", Ok(Adjustment::Grayscale)),
        "auto-levels" => ("Auto Levels", Ok(Adjustment::AutoLevels)),
        "sepia" => ("Sepia", Ok(Adjustment::Sepia { amount: 100 })),
        "edge-detect" => ("Edge Detect", Err(Filter::EdgeDetect)),
        "emboss" => ("Emboss", Err(Filter::Emboss)),
        "sharpen" => ("Sharpen", Err(Filter::Sharpen { amount: 50 })),
        _ => return None,
    })
}

impl Studio {
    pub fn new(product: Product) -> Self {
        Self {
            product,
            doc: None,
            path: String::new(),
            loading: None,
            modified: false,
            tool: product.tools()[0],
            previous: None,
            primary: cw_raster::BLACK,
            secondary: cw_raster::WHITE,
            slot: 0,
            size: product.default_size(),
            hardness: 100,
            opacity: 100,
            tolerance: 10,
            antialias: product != Product::Paint,
            shape: ShapeKind::Rectangle,
            outline: true,
            fill: false,
            select_mode: SelectMode::Replace,
            merged: product == Product::Paint,
            font_size: 24,
            bold: false,
            zoom: 0,
            scroll: (0, 0),
            gesture: None,
            text: None,
            crop: None,
            panel: None,
            layers_open: false,
            status: None,
            look: BTreeMap::new(),
            tab: String::new(),
            focus: String::new(),
            folder: "Pictures".into(),
            hover: None,
            modifiers: 0,
            retouch: Retouch {
                // GIMP's Alignment defaults to None, Pinta restarts every stroke and
                // Pixelmator's "Fix source position" is off: the source follows.
                aligned: product == Product::Pixelmator,
                ..Retouch::default()
            },
            gradient: GradientOptions::default(),
            path_edit: None,
            curve: None,
            preview: true,
            jpeg_quality: 0,
            subsampling: Subsampling::Full,
            xcf_zlib: false,
            pending: None,
        }
    }
    /// Open on `argument` (an image path), or on what the product shows when it starts
    /// empty.
    pub fn launch(product: Product, argument: &str, window: u64) -> (Self, Vec<AppEffect>) {
        let mut studio = Self::new(product);
        studio.tool = match product {
            Product::Paint => Tool::Pencil,
            Product::Gimp | Product::Pinta | Product::Pixelmator => Tool::Brush,
            Product::Sketchbook => Tool::Pencil,
            Product::Preview => Tool::RectSelect,
            Product::IosPhotos | Product::GooglePhotos => Tool::Pen,
        };
        if !argument.is_empty() {
            let effects = studio.open_path(window, argument);
            return (studio, effects);
        }
        if let Some((w, h, bg)) = product.blank() {
            studio.doc = Document::new(w, h, bg).ok();
        } else if product == Product::Preview {
            // Preview with nothing open asks for a file, as it does on a Mac.
            return studio.open_sheet(window);
        }
        (studio, vec![])
    }
    pub fn title(&self) -> String {
        match &self.doc {
            None => self.product.name().into(),
            Some(_) => {
                let name = self.document_name();
                match self.product {
                    Product::Paint => format!("{name} - Paint"),
                    Product::Gimp => format!(
                        "[{name}]-{} ({} layer{}) {}x{} – GIMP",
                        if self.modified { "*" } else { "" },
                        self.doc.as_ref().map_or(0, |d| d.layers().len()),
                        if self.doc.as_ref().map_or(0, |d| d.layers().len()) == 1 {
                            ""
                        } else {
                            "s"
                        },
                        self.doc.as_ref().map_or(0, Document::width),
                        self.doc.as_ref().map_or(0, Document::height),
                    ),
                    Product::Pinta => format!("{name} - Pinta"),
                    _ => name,
                }
            }
        }
    }
    pub fn document_name(&self) -> String {
        if self.path.is_empty() {
            "Untitled".into()
        } else {
            file_name(&self.path).to_owned()
        }
    }
    pub fn caption(&self) -> String {
        match &self.doc {
            Some(d) => format!("{} x {} px", d.width(), d.height()),
            None => String::new(),
        }
    }
    fn prefix(&self) -> &'static str {
        self.product.prefix()
    }
    /// Full target for a command.
    pub fn target(&self, command: &str) -> String {
        format!("{}:{command}", self.prefix())
    }

    // ----- files -------------------------------------------------------------------

    fn open_path(&mut self, window: u64, path: &str) -> Vec<AppEffect> {
        self.loading = Some(path.to_owned());
        self.status = None;
        // PNG and JPEG are decoded by the environment; bitmaps and XCF files are read
        // as bytes and decoded by the engine.
        if Format::of(path).is_some_and(|f| !f.decoded_by_environment()) {
            return vec![AppEffect::ReadBytes {
                window,
                path: path.to_owned(),
            }];
        }
        vec![AppEffect::ReadImage {
            window,
            path: path.to_owned(),
        }]
    }
    /// A file read as bytes arrived (a bitmap or an XCF), or could not be read.
    pub fn bytes_loaded(&mut self, path: &str, result: Result<Vec<u8>, String>) {
        if self.loading.as_deref() != Some(path) {
            return;
        }
        let opened = result.and_then(|bytes| match Format::of(path) {
            Some(Format::Xcf) => cw_raster::xcf::read(&bytes).map(|o| (o.document, o.notes)),
            Some(Format::Bmp) => cw_raster::bmp::decode(&bytes)
                .map(|c| (Document::from_canvas(c, &self.layer_name(path)), vec![])),
            _ => Err("this file is not an image".into()),
        });
        match opened {
            Ok((doc, notes)) => {
                self.opened(path, doc);
                self.status = notes.into_iter().next();
            }
            Err(reason) => self.image_failed(path, &reason),
        }
    }
    /// The name a single-layer image's layer takes when opened.
    fn layer_name(&self, path: &str) -> String {
        match self.product {
            Product::Gimp | Product::Pinta | Product::Pixelmator => file_name(path).to_owned(),
            _ => "Background".to_owned(),
        }
    }
    /// A document was opened from `path`.
    fn opened(&mut self, path: &str, doc: Document) {
        self.loading = None;
        self.doc = Some(doc);
        self.path = path.to_owned();
        self.modified = false;
        self.zoom = 0;
        self.scroll = (0, 0);
        self.crop = None;
        self.text = None;
        self.panel = None;
        self.status = None;
        self.path_edit = None;
        self.curve = None;
        self.retouch.source = None;
        self.retouch.offset = None;
    }
    fn open_sheet(mut self, window: u64) -> (Self, Vec<AppEffect>) {
        let effects = self.show_open(window);
        (self, effects)
    }
    fn show_open(&mut self, window: u64) -> Vec<AppEffect> {
        let folder = if self.path.is_empty() {
            self.folder.clone()
        } else {
            folder_of(&self.path).to_owned()
        };
        self.panel = Some(Panel::Open {
            folder: folder.clone(),
            entries: vec![],
            loading: true,
        });
        vec![AppEffect::ListDirectory {
            window,
            tab: 0,
            path: folder,
        }]
    }
    /// A folder listing for the open or save sheet.
    pub fn listed(&mut self, mut listing: Vec<String>) {
        let product = self.product;
        listing.retain(|e| e.ends_with('/') || product.opens(e));
        listing.truncate(LISTING_LIMIT);
        match &mut self.panel {
            Some(Panel::Open {
                entries, loading, ..
            }) => {
                *entries = listing;
                *loading = false;
            }
            Some(Panel::Save { entries, .. }) => *entries = listing,
            _ => {}
        }
    }
    /// The sheet's folder could not be read.
    pub fn listing_failed(&mut self, reason: &str) {
        if let Some(Panel::Open { loading, .. }) = &mut self.panel {
            *loading = false;
        }
        self.status = Some(reason.to_owned());
    }
    /// Decoded pixels arrived: the file that was opened, or a paste.
    pub fn image(
        &mut self,
        path: &str,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), String> {
        let canvas = Canvas::from_rgba(width, height, rgba)?;
        if path == CLIPBOARD_IMAGE {
            let doc = self.doc.as_mut().ok_or("no image is open to paste into")?;
            let (x, y) = doc
                .selection()
                .and_then(Mask::bounds)
                .map_or((0, 0), |r| (r.x, r.y));
            doc.paste(&canvas, x, y)?;
            self.modified = true;
            self.tool = if self.product.tools().contains(&Tool::Move) {
                Tool::Move
            } else {
                self.tool
            };
            return Ok(());
        }
        if self.loading.as_deref() != Some(path) {
            return Err("that image was not asked for".into());
        }
        let name = self.layer_name(path);
        self.opened(path, Document::from_canvas(canvas, &name));
        Ok(())
    }
    pub fn image_failed(&mut self, path: &str, reason: &str) {
        if path == crate::TEXT_IMAGE {
            // The text could not be drawn: keep it editable rather than stuck pending.
            if let Some(t) = &mut self.text {
                t.pending = false;
            }
            self.status = Some(reason.to_owned());
            return;
        }
        if path == CLIPBOARD_IMAGE {
            self.status = Some(reason.to_owned());
            return;
        }
        if self.loading.as_deref() == Some(path) {
            self.loading = None;
            self.status = Some(format!(
                "“{}” could not be opened: {reason}",
                file_name(path)
            ));
        }
    }
    /// Where a save of this document goes without asking, if anywhere: its own file,
    /// when that is in a format the product saves (for GIMP, only XCF).
    pub fn save_target(&self) -> Option<String> {
        Format::of(&self.path)
            .filter(|f| self.product.save_formats(false).contains(f))
            .map(|_| self.path.clone())
    }
    /// GIMP's File ▸ Overwrite: the PNG, JPEG or bitmap the image came from.
    pub fn overwrite_target(&self) -> Option<String> {
        Format::of(&self.path)
            .filter(|f| self.product.save_formats(true).contains(f))
            .map(|_| self.path.clone())
    }
    /// A PNG name for this document that its folder does not already hold.
    pub fn suggested_name(&self, entries: &[String], suffix: &str) -> String {
        self.suggested_as(entries, suffix, "png")
    }
    /// A name with extension `ext` that the folder does not already hold.
    pub fn suggested_as(&self, entries: &[String], suffix: &str, ext: &str) -> String {
        let stem = if self.path.is_empty() {
            match self.product {
                Product::Paint | Product::Pinta | Product::Gimp => "Untitled".to_owned(),
                Product::Sketchbook => "Sketch".to_owned(),
                _ => "Image".to_owned(),
            }
        } else {
            let name = file_name(&self.path);
            name.rsplit_once('.').map_or(name, |(s, _)| s).to_owned()
        };
        let taken = |n: &str| entries.iter().any(|e| e == n);
        let first = format!("{stem}{suffix}.{ext}");
        if !taken(&first) {
            return first;
        }
        (2..1000)
            .map(|i| format!("{stem}{suffix} {i}.{ext}"))
            .find(|n| !taken(n))
            .unwrap_or(first)
    }
    /// Write `path` in its format. A JPEG from GIMP or Pinta first asks for its
    /// quality, as both do; everything else is written at once.
    fn write(&mut self, window: u64, path: String) -> Result<Vec<AppEffect>, String> {
        self.doc.as_ref().ok_or("there is no image to save")?;
        if Format::of(&path) == Some(Format::Jpeg)
            && matches!(self.product, Product::Gimp | Product::Pinta)
        {
            self.pending = Some(path);
            let id = if self.product == Product::Gimp {
                "jpeg"
            } else {
                "jpeg-quality"
            };
            let values = dialog_params(id)
                .iter()
                .map(|(k, _, _, d)| ((*k).to_owned(), *d))
                .collect();
            self.show_dialog(id, values)?;
            return Ok(vec![]);
        }
        self.write_now(window, path)
    }
    /// Encode the image as `path`'s format and hand it to the environment.
    fn write_now(&mut self, window: u64, path: String) -> Result<Vec<AppEffect>, String> {
        let doc = self.doc.as_ref().ok_or("there is no image to save")?;
        let format = Format::of(&path).unwrap_or(Format::Png);
        let bytes = match format {
            Format::Png => {
                let flat = doc.composite();
                return Ok(vec![AppEffect::WriteImage {
                    window,
                    path,
                    width: flat.width(),
                    height: flat.height(),
                    rgba: flat.into_pixels(),
                }]);
            }
            Format::Jpeg => {
                cw_raster::jpeg::encode(&doc.composite(), self.quality(), self.subsampling)
            }
            Format::Bmp => cw_raster::bmp::encode(&doc.composite()),
            Format::Xcf => cw_raster::xcf::write(
                doc,
                if self.xcf_zlib {
                    cw_raster::xcf::Compression::Zlib
                } else {
                    cw_raster::xcf::Compression::Rle
                },
            ),
        };
        Ok(vec![AppEffect::WriteBytes {
            window,
            path,
            bytes,
        }])
    }
    /// Save in place when possible, otherwise ask for a name.
    pub fn save(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        self.doc.as_ref().ok_or("there is no image to save")?;
        match self.save_target() {
            Some(path) => self.write(window, path),
            None => Ok(self.save_sheet(window, false)),
        }
    }
    pub fn save_as(&mut self, window: u64) -> Vec<AppEffect> {
        self.save_sheet(window, false)
    }
    /// The Save (or GIMP's Export) sheet, named for the document in a format it offers.
    fn save_sheet(&mut self, window: u64, export: bool) -> Vec<AppEffect> {
        let folder = if self.path.is_empty() {
            self.folder.clone()
        } else {
            folder_of(&self.path).to_owned()
        };
        let formats = self.product.save_formats(export);
        let format = Format::of(&self.path)
            .filter(|f| formats.contains(f))
            .unwrap_or(formats[0]);
        self.panel = Some(Panel::Save {
            folder: folder.clone(),
            name: self.suggested_as(&[], "", format.extension()),
            entries: vec![],
            export,
        });
        vec![AppEffect::ListDirectory {
            window,
            tab: 0,
            path: folder,
        }]
    }
    /// The environment wrote the file.
    pub fn saved(&mut self, path: &str) {
        if Format::of(path).is_some() {
            self.path = path.to_owned();
        }
        self.modified = false;
        if matches!(self.panel, Some(Panel::Save { .. })) {
            self.panel = None;
        }
        self.status = Some(format!("Saved {}", file_name(path)));
    }
    /// Bytes this editor wrote (a JPEG, a bitmap, an XCF) were saved, or not.
    pub fn bytes_saved(&mut self, path: &str, result: Result<(), String>) {
        match result {
            Ok(()) => self.saved(path),
            Err(reason) => {
                self.status = Some(format!(
                    "“{}” could not be saved: {reason}",
                    file_name(path)
                ))
            }
        }
    }

    // ----- text --------------------------------------------------------------------

    /// Keyboard text: into the save sheet's name field, or onto the canvas.
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        if let Some(Panel::Save { name, .. }) = &mut self.panel {
            super::push_bounded(name, text, 128);
            return Ok(());
        }
        let entry = self
            .text
            .as_mut()
            .filter(|t| !t.pending)
            .ok_or("click the image with the Text tool to type on it")?;
        super::push_bounded(&mut entry.text, text, TEXT_LIMIT);
        Ok(())
    }
    /// Whether typing goes somewhere: the text tool's box or the save sheet's name.
    pub fn accepts_text(&self) -> bool {
        self.text.as_ref().is_some_and(|t| !t.pending)
            || matches!(self.panel, Some(Panel::Save { .. }))
    }
    fn commit_text(&mut self, window: u64) -> Vec<AppEffect> {
        match &mut self.text {
            Some(t) if !t.text.trim().is_empty() && !t.pending => {
                t.pending = true;
                vec![AppEffect::RasterText {
                    window,
                    text: t.text.clone(),
                    size: self.font_size,
                    bold: self.bold,
                }]
            }
            Some(t) if !t.pending => {
                self.text = None;
                vec![]
            }
            _ => vec![],
        }
    }
    /// Glyph coverage for the committed text: stamp it in the primary colour.
    pub fn text_rasterized(
        &mut self,
        width: u32,
        height: u32,
        alpha: Vec<u8>,
    ) -> Result<(), String> {
        let entry = self
            .text
            .take()
            .filter(|t| t.pending)
            .ok_or("no text was waiting to be drawn")?;
        let mask = Mask::from_data(width, height, alpha)?;
        let doc = self.doc.as_mut().ok_or("no image is open")?;
        doc.stamp(entry.x, entry.y, &mask, self.primary);
        self.modified = true;
        Ok(())
    }

    // ----- brushes -----------------------------------------------------------------

    pub fn brush(&self) -> Brush {
        let color = self.primary;
        let opacity = cw_raster::fmath::percent255(self.opacity);
        let base = Brush {
            kind: BrushKind::Paint,
            color,
            size: self.size.max(1),
            hardness: self.hardness,
            opacity,
            antialias: true,
            blend: BlendMode::Normal,
            spacing: 20,
        };
        match self.tool {
            Tool::Pencil => Brush {
                antialias: false,
                hardness: 100,
                ..base
            },
            Tool::Airbrush => Brush {
                hardness: 0,
                opacity: opacity / 2,
                spacing: 10,
                ..base
            },
            Tool::Pen => Brush {
                hardness: 100,
                ..base
            },
            Tool::Marker => Brush {
                hardness: 80,
                opacity: (u32::from(opacity) * 7 / 10) as u8,
                blend: BlendMode::Multiply,
                ..base
            },
            Tool::Highlighter => Brush {
                hardness: 100,
                size: self.size.max(12),
                opacity: opacity / 2,
                blend: BlendMode::Multiply,
                ..base
            },
            Tool::Eraser => Brush {
                kind: BrushKind::Erase,
                antialias: self.product != Product::Paint,
                ..base
            },
            _ => base,
        }
    }

    // ----- view --------------------------------------------------------------------

    /// Zoom actually shown in a `vw` x `vh` view: the set zoom, or the largest that fits
    /// (never above 100%).
    pub fn effective_zoom(&self, vw: u32, vh: u32) -> u32 {
        let Some(doc) = &self.doc else {
            return 100;
        };
        if self.zoom > 0 {
            return self.zoom;
        }
        let fit = (u64::from(vw.saturating_sub(16)) * 100 / u64::from(doc.width()))
            .min(u64::from(vh.saturating_sub(16)) * 100 / u64::from(doc.height()));
        fit.clamp(1, 100) as u32
    }
    /// Where the image's top-left sits in the view, in view pixels.
    pub fn origin(&self, vw: u32, vh: u32) -> (i32, i32) {
        let Some(doc) = &self.doc else {
            return (0, 0);
        };
        let z = i64::from(self.effective_zoom(vw, vh));
        let (dw, dh) = (
            i64::from(doc.width()) * z / 100,
            i64::from(doc.height()) * z / 100,
        );
        let axis = |view: u32, shown: i64, scroll: i32| -> i32 {
            if shown <= i64::from(view) {
                ((i64::from(view) - shown) / 2) as i32
            } else {
                -((i64::from(scroll) * z / 100) as i32)
            }
        };
        (axis(vw, dw, self.scroll.0), axis(vh, dh, self.scroll.1))
    }
    /// A view point (pixels from the view's top-left) in image sub16 units.
    pub fn to_image(&self, vw: u32, vh: u32, x: i32, y: i32) -> P16 {
        let z = i64::from(self.effective_zoom(vw, vh));
        let (ox, oy) = self.origin(vw, vh);
        (
            (i64::from(x - ox) * 16 * 100 + 50) / z,
            (i64::from(y - oy) * 16 * 100 + 50) / z,
        )
    }
    /// The centre of view pixel `(x, y)` in image sub16 units: the point whose image
    /// pixel the view draws there, which a position readout names.
    pub fn view_centre(&self, vw: u32, vh: u32, x: i32, y: i32) -> P16 {
        let z = i64::from(self.effective_zoom(vw, vh));
        let (ox, oy) = self.origin(vw, vh);
        (
            (i64::from(x - ox) * 1600 + 800) / z,
            (i64::from(y - oy) * 1600 + 800) / z,
        )
    }
    /// Keep the scroll inside the image for a `vw` x `vh` view.
    fn clamp_scroll(&mut self, vw: u32, vh: u32) {
        let Some(doc) = &self.doc else {
            return;
        };
        let z = self.effective_zoom(vw, vh).max(1);
        let max_x = i64::from(doc.width()) - i64::from(vw) * 100 / i64::from(z);
        let max_y = i64::from(doc.height()) - i64::from(vh) * 100 / i64::from(z);
        self.scroll = (
            self.scroll.0.clamp(0, max_x.max(0) as i32),
            self.scroll.1.clamp(0, max_y.max(0) as i32),
        );
    }
    fn step_zoom(&mut self, zoom_in: bool, vw: u32, vh: u32) {
        let current = self.effective_zoom(vw, vh);
        self.zoom = if zoom_in {
            ZOOM_STEPS.into_iter().find(|z| *z > current).unwrap_or(800)
        } else {
            ZOOM_STEPS
                .into_iter()
                .rev()
                .find(|z| *z < current)
                .unwrap_or(10)
        };
    }

    // ----- commands ----------------------------------------------------------------

    fn doc(&mut self) -> Result<&mut Document, String> {
        self.doc.as_mut().ok_or_else(|| "no image is open".into())
    }
    /// Run an edit on the document and mark it modified if it changed.
    fn edit(&mut self, f: impl FnOnce(&mut Document) -> Result<(), String>) -> Result<(), String> {
        let doc = self.doc()?;
        let before = doc.can_undo().then(|| doc.undo_steps().len());
        f(doc)?;
        let after = doc.can_undo().then(|| doc.undo_steps().len());
        if after != before || doc.can_undo() {
            self.modified = true;
        }
        Ok(())
    }
    fn set_param(&mut self, id: &str, value: i32) -> Result<(), String> {
        if let Some(Panel::Dialog { id: dialog, values }) = &mut self.panel {
            if let Some((_, lo, hi, _)) = dialog_params(dialog).iter().find(|p| p.0 == id) {
                let value = value.clamp(*lo, *hi);
                values.insert(id.to_owned(), value);
                // Keep the aspect ratio of a resize locked when asked to.
                if dialog == "resize" && values.get("ratio") == Some(&1) {
                    if let Some(doc) = &self.doc {
                        let (w, h) = (i64::from(doc.width()), i64::from(doc.height()));
                        if id == "width" {
                            values.insert(
                                "height".into(),
                                ((i64::from(value) * h + w / 2) / w).max(1) as i32,
                            );
                        } else if id == "height" {
                            values.insert(
                                "width".into(),
                                ((i64::from(value) * w + h / 2) / h).max(1) as i32,
                            );
                        }
                    }
                }
                return Ok(());
            }
        }
        if let Some((lo, hi)) = look::range(self.product, id) {
            self.look.insert(id.to_owned(), value.clamp(lo, hi));
            return Ok(());
        }
        let (lo, hi) = option_range(id).ok_or_else(|| format!("unknown setting {id}"))?;
        let v = value.clamp(lo, hi);
        match id {
            "size" => self.size = v as u32,
            "hardness" => self.hardness = v as u8,
            "opacity" => self.opacity = v as u8,
            "tolerance" => self.tolerance = v as u8,
            "font-size" => self.font_size = v as u16,
            "jpeg-quality" => self.jpeg_quality = v as u8,
            "zoom" => self.zoom = v as u32,
            "layer-opacity" => {
                let active = self.doc()?.active();
                self.edit(|d| d.set_opacity(active, v as u8))?;
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    /// The range a slider's parameter spans.
    pub fn param_range(&self, id: &str) -> Option<(i32, i32)> {
        if let Some(Panel::Dialog { id: dialog, .. }) = &self.panel {
            if let Some((_, lo, hi, _)) = dialog_params(dialog).iter().find(|p| p.0 == id) {
                return Some((*lo, *hi));
            }
        }
        look::range(self.product, id).or_else(|| option_range(id))
    }
    /// Current value of a parameter, wherever it lives.
    pub fn param(&self, id: &str) -> i32 {
        if let Some(Panel::Dialog { id: dialog, values }) = &self.panel {
            if let Some((_, _, _, default)) = dialog_params(dialog).iter().find(|p| p.0 == id) {
                return values.get(id).copied().unwrap_or(*default);
            }
        }
        if look::range(self.product, id).is_some() {
            return self.look.get(id).copied().unwrap_or(0);
        }
        match id {
            "size" => self.size as i32,
            "hardness" => i32::from(self.hardness),
            "opacity" => i32::from(self.opacity),
            "tolerance" => i32::from(self.tolerance),
            "font-size" => i32::from(self.font_size),
            "jpeg-quality" => i32::from(self.quality()),
            "zoom" => self.zoom as i32,
            "layer-opacity" => self
                .doc
                .as_ref()
                .map_or(100, |d| i32::from(d.active_layer().opacity)),
            _ => 0,
        }
    }
    fn open_dialog(&mut self, id: &str) -> Result<(), String> {
        if !self.product.dialogs().contains(&id) {
            return Err(format!("{} has no {id} command", self.product.name()));
        }
        let values: BTreeMap<String, i32> = dialog_params(id)
            .iter()
            .map(|(k, _, _, d)| ((*k).to_owned(), *d))
            .collect();
        self.show_dialog(id, values)
    }
    /// Show dialog `id` without asking whether the product lists it: the export
    /// options a save leads to, and Stroke Path, open this way.
    fn show_dialog(&mut self, id: &str, mut values: BTreeMap<String, i32>) -> Result<(), String> {
        match id {
            "new-image" => {}
            "jpeg" | "jpeg-quality" => {
                values.insert("quality".into(), i32::from(self.quality()));
                values.insert(
                    "subsampling".into(),
                    i32::from(self.subsampling == Subsampling::Quartered),
                );
            }
            "color" => {
                let c = if self.slot == 1 {
                    self.secondary
                } else {
                    self.primary
                };
                values.insert("r".into(), i32::from(c[0]));
                values.insert("g".into(), i32::from(c[1]));
                values.insert("b".into(), i32::from(c[2]));
                values.insert("slot".into(), i32::from(self.slot));
            }
            _ => {
                let doc = self.doc.as_ref().ok_or("no image is open")?;
                if id == "resize" {
                    values.insert("width".into(), doc.width() as i32);
                    values.insert("height".into(), doc.height() as i32);
                }
            }
        }
        self.panel = Some(Panel::Dialog {
            id: id.to_owned(),
            values,
        });
        Ok(())
    }
    /// The JPEG quality in force: the last chosen, or the product's own default.
    pub fn quality(&self) -> u8 {
        if self.jpeg_quality == 0 {
            self.product.default_jpeg_quality()
        } else {
            self.jpeg_quality
        }
    }
    fn apply_dialog(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let Some(Panel::Dialog { id, values }) = self.panel.clone() else {
            return Err("no dialog is open".into());
        };
        let op = operation(&id, &values).ok_or("unknown dialog")?;
        let label = view::dialog_title(self.product, &id);
        match op {
            // Exactly what the canvas previewed, as one step.
            Operation::Adjust(_) | Operation::Filter(_) => self.edit(|d| {
                d.edit_layer(&label, |layer, sel| {
                    run_dialog(&id, &values, layer, sel)?;
                    match sel {
                        Some(m) => m.bounds(),
                        None => Some(layer.bounds()),
                    }
                });
                Ok(())
            })?,
            Operation::Jpeg(quality, subsampling) => {
                let path = self.pending.take().ok_or("there is no export waiting")?;
                self.jpeg_quality = quality;
                self.subsampling = subsampling;
                self.panel = None;
                return self.write_now(window, path);
            }
            Operation::StrokePath(width, antialias) => {
                let path = self.path_edit.clone().ok_or("there is no path to stroke")?;
                let shape = path.stroke(self.primary, width, antialias);
                self.edit(|d| {
                    d.edit_layer("Stroke Path", |layer, sel| shape.draw(layer, sel));
                    Ok(())
                })?;
            }
            Operation::Resize(w, h, r) => self.edit(|d| d.resize(w, h, r))?,
            Operation::Rotate(a, expand) => {
                if a != 0 {
                    self.edit(|d| d.rotate(a, expand))?
                }
            }
            Operation::New(w, h, white) => {
                self.doc = Some(Document::new(w, h, white.then_some(cw_raster::WHITE))?);
                self.path.clear();
                self.modified = false;
                self.zoom = 0;
                self.scroll = (0, 0);
            }
            Operation::Color(c, slot) => {
                if slot == 1 {
                    self.secondary = c;
                } else {
                    self.primary = c;
                }
            }
        }
        self.panel = None;
        Ok(vec![])
    }
    /// Whether an open dialog shows its effect on the canvas: always in Pinta,
    /// Pixelmator and Preview, and in GIMP while its Preview box is ticked.
    pub fn previewing(&self) -> bool {
        self.product != Product::Gimp || self.preview
    }
    /// The active layer as the edit in progress would leave it, for the canvas to show
    /// before anything is committed: an open dialog's adjustment or filter, a layer
    /// being dragged with the Move tool, a gradient being dragged out. `None` when the
    /// layer shows as it is. Cancelling any of these restores the view exactly, since
    /// the document itself was never touched.
    pub fn preview_layer(&self) -> Option<Canvas> {
        let doc = self.doc.as_ref()?;
        let active = &doc.active_layer().canvas;
        if let Some(Panel::Dialog { id, values }) = &self.panel {
            if !self.previewing() {
                return None;
            }
            let mut layer = active.clone();
            run_dialog(id, values, &mut layer, doc.selection())?;
            return Some(layer);
        }
        match &self.gesture {
            Some(Gesture::Span {
                tool: Tool::Move,
                start,
                end,
            }) => {
                let (a, b) = (pixel_of(*start), pixel_of(*end));
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let mut out = Canvas::new(active.width(), active.height());
                out.put(dx, dy, active);
                Some(out)
            }
            Some(Gesture::Span {
                tool: Tool::Gradient,
                start,
                end,
            }) => {
                let mut layer = active.clone();
                self.gradient_for(*start, *end)
                    .apply(&mut layer, doc.selection())?;
                Some(layer)
            }
            _ => None,
        }
    }
    /// The gradient the tool lays between two points, with its options.
    pub fn gradient_for(&self, start: P16, end: P16) -> Gradient {
        let fg = self.primary;
        Gradient {
            shape: self.gradient.shape,
            repeat: self.gradient.repeat,
            from: fg,
            to: if self.gradient.transparent {
                [fg[0], fg[1], fg[2], 0]
            } else {
                self.secondary
            },
            reverse: self.gradient.reverse,
            start,
            end,
            opacity: cw_raster::fmath::percent255(self.opacity),
            blend: BlendMode::Normal,
        }
    }
    /// Apply a one-shot operation.
    fn run_action(&mut self, id: &str) -> Result<(), String> {
        if !self.product.actions().contains(&id) {
            return Err(format!("{} has no {id} command", self.product.name()));
        }
        let (label, op) = action(id).ok_or("unknown action")?;
        self.edit(|d| {
            match &op {
                Ok(adj) => d.adjust(label, adj),
                Err(f) => d.filter(label, f),
            };
            Ok(())
        })
    }
    fn set_color(&mut self, hex: &str, slot: Option<u8>) -> Result<(), String> {
        let c = cw_raster::parse_hex(hex).ok_or("not a colour")?;
        match slot.unwrap_or(self.slot) {
            1 => self.secondary = c,
            _ => self.primary = c,
        }
        Ok(())
    }

    /// The wheel over the canvas scrolls an image larger than its view (Shift, or a
    /// sideways turn, scrolls across), and Ctrl+wheel zooms one step a notch keeping
    /// the image point under the pointer where it is, as every one of these editors
    /// does. Returns whether the view moved.
    pub fn wheel(
        &mut self,
        target: &str,
        x: i32,
        y: i32,
        wheel: crate::Wheel,
    ) -> Result<bool, String> {
        let Some(command) = target
            .strip_prefix(self.prefix())
            .and_then(|t| t.strip_prefix(':'))
        else {
            return Ok(false);
        };
        let mut parts = command.split(':');
        if parts.next() != Some("canvas") || self.doc.is_none() {
            return Ok(false);
        }
        let (Some(vw), Some(vh)) = (
            parts.next().and_then(|v| v.parse::<u32>().ok()),
            parts.next().and_then(|v| v.parse::<u32>().ok()),
        ) else {
            return Ok(false);
        };
        let before = (self.scroll, self.zoom);
        if wheel.ctrl {
            let at = self.to_image(vw, vh, x, y);
            let (px, py) = ((at.0 / 16) as i32, (at.1 / 16) as i32);
            let notches = crate::wheel_steps(-wheel.dy, 120);
            for _ in 0..notches.unsigned_abs().min(8) {
                self.step_zoom(notches > 0, vw, vh);
            }
            let z = i64::from(self.effective_zoom(vw, vh).max(1));
            self.scroll = (
                px - (i64::from(x) * 100 / z) as i32,
                py - (i64::from(y) * 100 / z) as i32,
            );
        } else {
            let z = i64::from(self.effective_zoom(vw, vh).max(1));
            let across = wheel.horizontal();
            let down = if wheel.shift && wheel.dx == 0 {
                0
            } else {
                wheel.dy
            };
            self.scroll.0 += (i64::from(across) * 100 / z) as i32;
            self.scroll.1 += (i64::from(down) * 100 / z) as i32;
        }
        self.clamp_scroll(vw, vh);
        Ok(before != (self.scroll, self.zoom))
    }
    /// Whether `target` (a full target) is one of this editor's drag surfaces.
    pub fn drags(&self, target: &str) -> bool {
        let Some(command) = target
            .strip_prefix(self.prefix())
            .and_then(|t| t.strip_prefix(':'))
        else {
            return false;
        };
        let head = command.split(':').next().unwrap_or("");
        matches!(
            head,
            "canvas" | "slider" | "dial" | "curve" | "hscroll" | "vscroll"
        )
    }

    /// A command from a control, without the product prefix.
    pub fn command(&mut self, window: u64, command: &str) -> Result<Vec<AppEffect>, String> {
        let (head, rest) = command.split_once(':').unwrap_or((command, ""));
        // Menus close when anything in them is chosen.
        let keep_panel = matches!(head, "menu" | "set" | "open" | "folder" | "folder-up")
            || matches!(self.panel, Some(Panel::Dialog { .. }) if head == "set");
        if !keep_panel && matches!(self.panel, Some(Panel::Menu { .. })) {
            self.panel = None;
        }
        match head {
            "noop" => {}
            "tool" => {
                let tool = Tool::parse(rest).ok_or("unknown tool")?;
                if !self.product.tools().contains(&tool) {
                    return Err(format!("{} has no {rest} tool", self.product.name()));
                }
                let effects = self.commit_text(window);
                self.commit_curve();
                if self.tool != tool {
                    self.crop = None;
                }
                self.tool = tool;
                self.previous = None;
                // Pixelmator's tools sidebar shows the chosen tool's options in place of
                // the Adjust Colors or Effects browser.
                if self.product == Product::Pixelmator {
                    self.tab.clear();
                }
                return Ok(effects);
            }
            "shape" => {
                let kind = ShapeKind::parse(rest).ok_or("unknown shape")?;
                if !self.product.tools().contains(&Tool::Shape) {
                    return Err(format!("{} has no shapes", self.product.name()));
                }
                self.commit_curve();
                self.shape = kind;
                self.tool = Tool::Shape;
            }
            // Gradient tool options.
            "gradient-shape" => {
                self.gradient.shape = GradientShape::parse(rest).ok_or("unknown gradient shape")?
            }
            "gradient-repeat" => {
                self.gradient.repeat = Repeat::parse(rest).ok_or("unknown repeat mode")?
            }
            "gradient-colors" => {
                self.gradient.transparent = match rest {
                    "fg-bg" => false,
                    "fg-transparent" => true,
                    _ => return Err("gradient colours are fg-bg or fg-transparent".into()),
                }
            }
            "gradient-reverse" => self.gradient.reverse = !self.gradient.reverse,
            // Clone: whether the source follows the brush from stroke to stroke.
            "aligned" => {
                self.retouch.aligned = match rest {
                    "on" => true,
                    "off" => false,
                    "" => !self.retouch.aligned,
                    _ => return Err("aligned is on or off".into()),
                };
                self.retouch.offset = None;
            }
            "clone-source" => {
                // `clone-source:<x>:<y>`: set the source without a modifier-click.
                let (x, y) = rest.split_once(':').ok_or("clone-source:<x>:<y>")?;
                let (x, y) = (parse_i32(x)?, parse_i32(y)?);
                let doc = self.doc.as_ref().ok_or("no image is open")?;
                if !doc.bounds().contains(x, y) {
                    return Err("the source must be on the image".into());
                }
                self.set_source(x, y);
            }
            "path" => return self.path_command(rest),
            "curve-edit" => match rest {
                "commit" => self.commit_curve(),
                "cancel" => self.curve = None,
                _ => return Err("curve-edit commit or cancel".into()),
            },
            "preview-toggle" => self.preview = !self.preview,
            "xcf-compression" => self.xcf_zlib = !self.xcf_zlib,
            "format" => {
                let format = Format::of(&format!("x.{rest}")).ok_or("unknown format")?;
                let product = self.product;
                let Some(Panel::Save { name, export, .. }) = &mut self.panel else {
                    return Err("the save sheet is not showing".into());
                };
                if !product.save_formats(*export).contains(&format) {
                    return Err(format!("{} cannot save {}", product.name(), format.label()));
                }
                let stem = match name.rsplit_once('.') {
                    Some((stem, ext)) if Format::of(&format!("x.{ext}")).is_some() => stem,
                    _ => name.as_str(),
                };
                *name = format!("{stem}.{}", format.extension());
            }
            "overwrite" => {
                let path = self
                    .overwrite_target()
                    .ok_or("the image did not come from a PNG, JPEG or BMP file")?;
                return self.write_now(window, path);
            }
            "outline" => self.outline = rest != "none",
            "fill" => self.fill = rest != "none",
            "fill-style" => {
                (self.outline, self.fill) = match rest {
                    "outline" => (true, false),
                    "fill" => (false, true),
                    "both" => (true, true),
                    _ => return Err("fill style is outline, fill or both".into()),
                }
            }
            "antialias" => self.antialias = !self.antialias,
            "merged" => self.merged = !self.merged,
            "bold" => self.bold = !self.bold,
            "mode" => self.select_mode = SelectMode::parse(rest).ok_or("unknown selection mode")?,
            "color" => self.set_color(rest, None)?,
            "fg" => self.set_color(rest, Some(0))?,
            "bg" => {
                self.set_color(rest, Some(1))?;
                // Choosing a fill colour in Markup turns the fill on.
                if self.product == Product::Preview {
                    self.fill = true;
                }
            }
            "slot" => {
                self.slot = match rest {
                    "1" => 0,
                    "2" => 1,
                    _ => return Err("there are two colour slots".into()),
                }
            }
            "swap-colors" => std::mem::swap(&mut self.primary, &mut self.secondary),
            "reset-colors" => {
                self.primary = cw_raster::BLACK;
                self.secondary = cw_raster::WHITE;
            }
            "set" => {
                let (id, value) = rest.rsplit_once(':').ok_or("set needs a value")?;
                self.set_param(id, parse_i32(value)?)?;
            }
            "undo" => {
                let doc = self.doc()?;
                doc.undo()?;
                self.modified = true;
            }
            "redo" => {
                let doc = self.doc()?;
                doc.redo()?;
                self.modified = true;
            }
            "new" => {
                if self.product.dialogs().contains(&"new-image") {
                    self.open_dialog("new-image")?;
                } else if let Some((w, h, bg)) = self.product.blank() {
                    self.doc = Some(Document::new(w, h, bg)?);
                    self.path.clear();
                    self.modified = false;
                } else {
                    return Err(format!("{} cannot make a new image", self.product.name()));
                }
            }
            "open" => {
                if rest.is_empty() {
                    return Ok(self.show_open(window));
                }
                let Some(Panel::Open {
                    folder, entries, ..
                }) = &self.panel
                else {
                    return Err("the Open sheet is not showing".into());
                };
                if !entries.iter().any(|e| e == rest) {
                    return Err("that file is not in this folder".into());
                }
                let path = join(folder, rest);
                self.panel = None;
                return Ok(self.open_path(window, &path));
            }
            "folder" | "folder-up" => {
                let (current, entries) = match &self.panel {
                    Some(Panel::Open {
                        folder, entries, ..
                    })
                    | Some(Panel::Save {
                        folder, entries, ..
                    }) => (folder.clone(), entries.clone()),
                    _ => return Err("no file sheet is showing".into()),
                };
                let next = if head == "folder-up" {
                    folder_of(current.trim_end_matches('/')).to_owned()
                } else {
                    let entry = format!("{rest}/");
                    if !entries.contains(&entry) {
                        return Err("that folder is not here".into());
                    }
                    join(&current, rest)
                };
                if next.is_empty() || next == current {
                    return Err("there is no folder above this one".into());
                }
                match &mut self.panel {
                    Some(Panel::Open {
                        folder,
                        entries,
                        loading,
                    }) => {
                        *folder = next.clone();
                        entries.clear();
                        *loading = true;
                    }
                    Some(Panel::Save {
                        folder, entries, ..
                    }) => {
                        *folder = next.clone();
                        entries.clear();
                    }
                    _ => {}
                }
                return Ok(vec![AppEffect::ListDirectory {
                    window,
                    tab: 0,
                    path: next,
                }]);
            }
            "save" => return self.save(window),
            "save-as" | "export" => {
                self.doc.as_ref().ok_or("there is no image to save")?;
                let effects = self.save_sheet(window, head == "export");
                // `save-as:<ext>`: Paint's "Save as ▸ JPEG picture" names the format.
                if !rest.is_empty() {
                    self.command(window, &format!("format:{rest}"))?;
                }
                return Ok(effects);
            }
            "save-confirm" => {
                let Some(Panel::Save {
                    folder,
                    name,
                    export,
                    ..
                }) = self.panel.clone()
                else {
                    return Err("the save sheet is not showing".into());
                };
                let name = name.trim();
                if name.is_empty() || name.contains('/') {
                    return Err("that is not a usable file name".into());
                }
                let formats = self.product.save_formats(export);
                let name = match Format::of(name) {
                    Some(f) if formats.contains(&f) => name.to_owned(),
                    // GIMP's own answer to a PNG named in Save: the sheet stays, with
                    // the reason.
                    Some(f) => {
                        self.status = Some(if self.product == Product::Gimp && !export {
                            format!(
                                "{} is not XCF: use File ▸ Export As to write other formats",
                                f.label()
                            )
                        } else {
                            format!("{} cannot save {} files", self.product.name(), f.label())
                        });
                        return Ok(vec![]);
                    }
                    // No extension: the sheet's first format.
                    None => format!("{name}.{}", formats[0].extension()),
                };
                return self.write(window, join(&folder, &name));
            }
            "close-panel" | "cancel" => {
                self.panel = None;
                self.crop = None;
                self.pending = None;
            }
            "menu" => {
                let open = matches!(&self.panel, Some(Panel::Menu { id }) if id == rest);
                self.panel = (!open).then(|| Panel::Menu {
                    id: rest.to_owned(),
                });
            }
            "dialog" => self.open_dialog(rest)?,
            "apply" => return self.apply_dialog(window),
            "reset" => {
                let Some(Panel::Dialog { id, values }) = &mut self.panel else {
                    return Err("no dialog is open".into());
                };
                for (k, _, _, d) in dialog_params(id) {
                    if !matches!(*k, "width" | "height" | "slot" | "r" | "g" | "b") {
                        values.insert((*k).to_owned(), *d);
                    }
                }
            }
            "action" => self.run_action(rest)?,
            "select-all" => self.edit(|d| {
                d.select_all();
                Ok(())
            })?,
            "select-none" => self.edit(|d| {
                d.select_none();
                Ok(())
            })?,
            "select-invert" => self.edit(|d| {
                d.invert_selection();
                Ok(())
            })?,
            "delete" => {
                self.doc()?
                    .delete_selection()
                    .ok_or("nothing is selected")?;
                self.modified = true;
            }
            "crop-selection" => self.edit(Document::crop_to_selection)?,
            "crop-apply" => {
                let r = self.crop.take().ok_or("drag a crop rectangle first")?;
                self.edit(|d| d.crop(r))?;
            }
            "copy" | "cut" => {
                let doc = self.doc.as_ref().ok_or("no image is open")?;
                let pixels = doc.copy();
                if head == "cut" {
                    self.doc()?
                        .delete_selection()
                        .ok_or("nothing is selected")?;
                    self.modified = true;
                }
                return Ok(vec![AppEffect::CopyImage {
                    window,
                    width: pixels.width(),
                    height: pixels.height(),
                    rgba: pixels.into_pixels(),
                }]);
            }
            "paste" => {
                self.doc.as_ref().ok_or("no image is open to paste into")?;
                return Ok(vec![AppEffect::PasteImage { window }]);
            }
            "rotate" => {
                let turns = match rest {
                    "cw" => 1,
                    "180" => 2,
                    "ccw" => 3,
                    _ => return Err("rotate cw, ccw or 180".into()),
                };
                self.edit(|d| d.rotate_quarter(turns))?;
            }
            "flip" => {
                let horizontal = match rest {
                    "h" => true,
                    "v" => false,
                    _ => return Err("flip h or v".into()),
                };
                self.edit(|d| d.flip(horizontal))?;
            }
            "flatten" => self.edit(Document::flatten)?,
            "layers" => self.layers_open = !self.layers_open,
            "layer" => {
                if !self.product.has_layers() {
                    return Err(format!("{} has no layers", self.product.name()));
                }
                let (op, arg) = rest.split_once(':').unwrap_or((rest, ""));
                match op {
                    "new" => {
                        let n = self.doc()?.layers().len();
                        self.edit(|d| d.add_layer(&format!("Layer {n}")))?
                    }
                    "delete" => self.edit(Document::delete_layer)?,
                    "duplicate" => self.edit(Document::duplicate_layer)?,
                    "up" => self.edit(|d| d.move_layer(true))?,
                    "down" => self.edit(|d| d.move_layer(false))?,
                    "merge" => self.edit(Document::merge_down)?,
                    "select" => {
                        let i: usize = arg.parse().map_err(|_| "invalid layer")?;
                        self.doc()?.select_layer(i)?;
                    }
                    "toggle" => {
                        let i: usize = arg.parse().map_err(|_| "invalid layer")?;
                        let visible = self.doc()?.layers().get(i).ok_or("no such layer")?.visible;
                        self.edit(|d| d.set_visible(i, !visible))?;
                    }
                    "blend" => {
                        let mode = BlendMode::parse(arg).ok_or("unknown blend mode")?;
                        let active = self.doc()?.active();
                        self.edit(|d| d.set_blend(active, mode))?;
                    }
                    _ => return Err(format!("unknown layer command {op}")),
                }
            }
            "zoom" => {
                // `zoom:in:<view width>:<view height>`: the view size makes a step from
                // "fit" start at the zoom that was really on screen.
                let mut parts = rest.split(':');
                let rest = parts.next().unwrap_or("");
                let vw = parts.next().and_then(|v| v.parse().ok()).unwrap_or(800);
                let vh = parts.next().and_then(|v| v.parse().ok()).unwrap_or(600);
                match rest {
                    "in" => self.step_zoom(true, vw, vh),
                    "out" => self.step_zoom(false, vw, vh),
                    "fit" => self.zoom = 0,
                    v => {
                        let z: u32 = v.parse().map_err(|_| "unknown zoom")?;
                        self.zoom = z.clamp(10, 800);
                    }
                }
            }
            "text" => match rest {
                "commit" => return Ok(self.commit_text(window)),
                "cancel" => self.text = None,
                _ => return Err("text commit or cancel".into()),
            },
            "tab" => {
                self.tab = match rest {
                    // Preview's Markup button shows and hides its toolbar.
                    "markup-toggle" if self.tab == "markup" => String::new(),
                    "markup-toggle" => "markup".into(),
                    other => other.to_owned(),
                };
                self.panel = None;
            }
            "focus" => {
                if look::range(self.product, rest).is_none() {
                    return Err("unknown adjustment".into());
                }
                self.focus = rest.to_owned();
            }
            "preset" => look::preset(self, rest)?,
            "look" => return look::command(self, window, rest),
            "canvas" | "slider" | "dial" | "curve" | "hscroll" | "vscroll" => {
                // A click without coordinates lands at the surface's origin.
                let mut effects = self.pointer(window, command, PointerPhase::Down, 0, 0)?;
                effects.extend(self.pointer(window, command, PointerPhase::Up, 0, 0)?);
                return Ok(effects);
            }
            _ => return Err(format!("unknown {} command {command}", self.product.name())),
        }
        Ok(vec![])
    }

    /// Pointer events on a drag surface, relative to its top-left. `command` is the
    /// surface's target without the product prefix.
    pub fn pointer(
        &mut self,
        window: u64,
        command: &str,
        phase: PointerPhase,
        x: i32,
        y: i32,
    ) -> Result<Vec<AppEffect>, String> {
        let mut parts = command.split(':');
        let head = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.collect();
        let num = |i: usize| -> i32 { args.get(i).and_then(|v| v.parse().ok()).unwrap_or(0) };
        match head {
            "canvas" => {
                let (vw, vh) = (num(0).max(1) as u32, num(1).max(1) as u32);
                self.canvas_pointer(window, phase, vw, vh, x, y)
            }
            "slider" => {
                let param = args.first().copied().unwrap_or("").to_owned();
                let width = num(1).max(1);
                let (lo, hi) = self
                    .param_range(&param)
                    .ok_or_else(|| format!("unknown setting {param}"))?;
                if phase == PointerPhase::Cancel {
                    return Ok(vec![]);
                }
                let t = x.clamp(0, width);
                let value = lo
                    + ((i64::from(t) * i64::from(hi - lo) + i64::from(width) / 2)
                        / i64::from(width)) as i32;
                self.set_param(&param, value)?;
                Ok(vec![])
            }
            "dial" => {
                let param = args.first().copied().unwrap_or("").to_owned();
                match phase {
                    PointerPhase::Down => {
                        let value = self.param(&param);
                        self.gesture = Some(Gesture::Dial { param, x, value });
                    }
                    PointerPhase::Move | PointerPhase::Up => {
                        if let Some(Gesture::Dial {
                            param,
                            x: x0,
                            value,
                        }) = self.gesture.clone()
                        {
                            // Dragging the scale left moves the value up, as on a phone.
                            self.set_param(&param, value - (x - x0) / 3)?;
                        }
                        if phase == PointerPhase::Up {
                            self.gesture = None;
                        }
                    }
                    PointerPhase::Cancel => self.gesture = None,
                }
                Ok(vec![])
            }
            "curve" => {
                let (w, h) = (num(0).max(1), num(1).max(1));
                if phase == PointerPhase::Down {
                    let point = ((x.clamp(0, w) * 4 + w / 2) / w).clamp(0, 4) as usize;
                    self.gesture = Some(Gesture::Curve {
                        width: w,
                        height: h,
                        point,
                    });
                }
                if let Some(Gesture::Curve { point, .. }) = self.gesture.clone() {
                    let value = 255 - (y.clamp(0, h) * 255 + h / 2) / h;
                    self.set_param(&format!("c{point}"), value)?;
                }
                if matches!(phase, PointerPhase::Up | PointerPhase::Cancel) {
                    self.gesture = None;
                }
                Ok(vec![])
            }
            "hscroll" | "vscroll" => {
                let track = num(0).max(1);
                let horizontal = head == "hscroll";
                let doc = self.doc.as_ref().ok_or("no image is open")?;
                let extent = if horizontal {
                    doc.width()
                } else {
                    doc.height()
                } as i32;
                let v = (i64::from(if horizontal { x } else { y }.clamp(0, track))
                    * i64::from(extent)
                    / i64::from(track)) as i32;
                let (vw, vh) = (num(1).max(1) as u32, num(2).max(1) as u32);
                if horizontal {
                    self.scroll.0 = v - (vw * 50 / self.effective_zoom(vw, vh).max(1)) as i32;
                } else {
                    self.scroll.1 = v - (vh * 50 / self.effective_zoom(vw, vh).max(1)) as i32;
                }
                self.clamp_scroll(vw, vh);
                Ok(vec![])
            }
            _ => Err(format!("{command} is not a drag surface")),
        }
    }

    fn canvas_pointer(
        &mut self,
        window: u64,
        phase: PointerPhase,
        vw: u32,
        vh: u32,
        x: i32,
        y: i32,
    ) -> Result<Vec<AppEffect>, String> {
        if self.doc.is_none() {
            return Err("no image is open".into());
        }
        if matches!(self.product, Product::IosPhotos | Product::GooglePhotos)
            && self.tab != "markup"
        {
            return Err("switch to Markup to draw on the photo".into());
        }
        if matches!(self.panel, Some(Panel::Menu { .. })) {
            self.panel = None;
        }
        let p = self.to_image(vw, vh, x, y);
        let (px, py) = pixel_of(p);
        self.hover = Some(self.view_centre(vw, vh, x, y));
        if phase == PointerPhase::Cancel {
            if matches!(self.gesture, Some(Gesture::Paint)) {
                self.doc()?.end_stroke();
                self.retouch.active = None;
            }
            self.gesture = None;
            return Ok(vec![]);
        }
        let tool = self.tool;
        let mut effects = vec![];
        if phase == PointerPhase::Down {
            // Starting anything else on the canvas commits text being typed.
            if tool != Tool::Text {
                effects.extend(self.commit_text(window));
            }
            match tool {
                t if t.paints() => {
                    let brush = self.brush();
                    self.doc()?.begin_stroke(brush, p);
                    self.gesture = Some(Gesture::Paint);
                }
                t if t.retouches() => self.retouch_down(p)?,
                Tool::Paths => self.path_down(p, vw, vh)?,
                Tool::Shape if self.shape == ShapeKind::Freeform => {
                    self.gesture = Some(Gesture::Freehand { points: vec![p] })
                }
                Tool::Shape if self.curve.is_some() => self.curve_down(p, vw, vh),
                t if t.spans() => {
                    self.gesture = Some(Gesture::Span {
                        tool: t,
                        start: p,
                        end: p,
                    })
                }
                Tool::Lasso => self.gesture = Some(Gesture::Lasso { points: vec![p] }),
                Tool::Pan => {
                    self.gesture = Some(Gesture::Pan {
                        x,
                        y,
                        scroll: self.scroll,
                    })
                }
                Tool::Fill => {
                    let color = self.primary;
                    let tolerance = (u32::from(self.tolerance) * 255 / 100) as u8;
                    let merged = self.merged;
                    let doc = self.doc()?;
                    if doc.fill(px, py, color, tolerance, merged).is_some() {
                        self.modified = true;
                    }
                }
                Tool::MagicWand => {
                    let tolerance = (u32::from(self.tolerance) * 255 / 100) as u8;
                    let (merged, mode) = (self.merged, self.select_mode);
                    let doc = self.doc()?;
                    if !doc.bounds().contains(px, py) {
                        return Ok(effects);
                    }
                    doc.select_similar(px, py, tolerance, merged, mode);
                }
                Tool::Picker => {
                    let merged = self.merged;
                    if let Some(c) = self.doc()?.pick(px, py, merged) {
                        let c = [c[0], c[1], c[2], 255];
                        if self.slot == 1 {
                            self.secondary = c;
                        } else {
                            self.primary = c;
                        }
                        // Paint goes back to the tool it came from.
                        if let Some(prev) = self.previous.take() {
                            self.tool = prev;
                        }
                    }
                }
                Tool::Zoom => {
                    let z = self.effective_zoom(vw, vh);
                    self.step_zoom(true, vw, vh);
                    let nz = self.zoom.max(1);
                    // Keep the clicked point under the pointer.
                    self.scroll = (
                        px - (x as i64 * 100 / i64::from(nz)) as i32,
                        py - (y as i64 * 100 / i64::from(nz)) as i32,
                    );
                    let _ = z;
                    self.clamp_scroll(vw, vh);
                }
                Tool::Text => {
                    let doc = self.doc.as_ref().ok_or("no image is open")?;
                    if !doc.bounds().contains(px, py) {
                        return Ok(effects);
                    }
                    effects.extend(self.commit_text(window));
                    if self.text.as_ref().is_none_or(|t| !t.pending) {
                        self.text = Some(TextEntry {
                            x: px,
                            y: py,
                            text: String::new(),
                            pending: false,
                        });
                    }
                }
                _ => {}
            }
            return Ok(effects);
        }
        // Move or release.
        match self.gesture.clone() {
            Some(Gesture::Paint) => {
                let doc = self.doc()?;
                doc.stroke_to(p);
                if phase == PointerPhase::Up {
                    if doc.end_stroke().is_some() {
                        self.modified = true;
                    }
                    self.gesture = None;
                    self.retouch.active = None;
                }
            }
            Some(Gesture::Freehand { mut points }) => {
                if points
                    .last()
                    .is_none_or(|l| (l.0 - p.0).abs() + (l.1 - p.1).abs() >= 16)
                {
                    points.push(p);
                }
                if phase == PointerPhase::Move {
                    self.gesture = Some(Gesture::Freehand { points });
                    return Ok(effects);
                }
                self.gesture = None;
                if points.len() >= 3 {
                    let shape = Shape {
                        kind: ShapeKind::Freeform,
                        points,
                        outline: self.outline.then_some(self.primary),
                        fill: self.fill.then_some(self.secondary),
                        width: self.size.max(1),
                        antialias: self.antialias,
                    };
                    if self.doc()?.shape(&shape).is_some() {
                        self.modified = true;
                    }
                }
            }
            Some(Gesture::CurvePoint { index }) => {
                let paint = self.product == Product::Paint;
                if let Some(curve) = &mut self.curve {
                    if let Some(q) = curve.points.get_mut(index) {
                        *q = p;
                    }
                    // Paint's first bend pulls both control points together.
                    if paint && curve.bends == 0 && index == 1 {
                        curve.points[2] = p;
                    }
                    if phase == PointerPhase::Up {
                        self.gesture = None;
                        if paint {
                            curve.bends += 1;
                            if curve.bends >= 2 {
                                self.commit_curve();
                            }
                        }
                    }
                }
            }
            Some(Gesture::Anchor {
                index,
                which,
                fresh,
            }) => {
                if let Some(path) = &mut self.path_edit {
                    if let Some(a) = path.anchors.get_mut(index) {
                        match (fresh, which) {
                            // Dragging out of a new anchor pulls symmetric handles.
                            (true, _) => {
                                if p != a.point {
                                    *a = Anchor::smooth(a.point, p);
                                }
                            }
                            (false, 0) => {
                                let (dx, dy) = (p.0 - a.point.0, p.1 - a.point.1);
                                a.point = p;
                                a.cin = (a.cin.0 + dx, a.cin.1 + dy);
                                a.cout = (a.cout.0 + dx, a.cout.1 + dy);
                            }
                            (false, 1) => a.cin = p,
                            (false, _) => a.cout = p,
                        }
                    }
                }
                if phase == PointerPhase::Up {
                    self.gesture = None;
                }
            }
            Some(Gesture::Span { tool, start, .. }) => {
                if phase == PointerPhase::Move {
                    self.gesture = Some(Gesture::Span {
                        tool,
                        start,
                        end: p,
                    });
                    return Ok(effects);
                }
                self.gesture = None;
                self.finish_span(tool, start, p)?;
            }
            Some(Gesture::Lasso { mut points }) => {
                if points
                    .last()
                    .is_none_or(|l| (l.0 - p.0).abs() + (l.1 - p.1).abs() >= 16)
                {
                    points.push(p);
                }
                if phase == PointerPhase::Move {
                    self.gesture = Some(Gesture::Lasso { points });
                    return Ok(effects);
                }
                self.gesture = None;
                let (w, h) = {
                    let d = self.doc()?;
                    (d.width(), d.height())
                };
                let mode = self.select_mode;
                let mask = Mask::polygon(w, h, &points);
                self.doc()?.select(mask, mode);
            }
            Some(Gesture::Pan {
                x: x0,
                y: y0,
                scroll,
            }) => {
                let z = i64::from(self.effective_zoom(vw, vh).max(1));
                self.scroll = (
                    scroll.0 - (i64::from(x - x0) * 100 / z) as i32,
                    scroll.1 - (i64::from(y - y0) * 100 / z) as i32,
                );
                self.clamp_scroll(vw, vh);
                if phase == PointerPhase::Up {
                    self.gesture = None;
                }
            }
            _ => {
                if phase == PointerPhase::Up {
                    self.gesture = None;
                }
            }
        }
        Ok(effects)
    }

    /// Release of a drag that spanned a rectangle.
    fn finish_span(&mut self, tool: Tool, start: P16, end: P16) -> Result<(), String> {
        let (w, h) = {
            let d = self.doc()?;
            (d.width(), d.height())
        };
        let (a, b) = (pixel_of(start), pixel_of(end));
        let rect = IRect::spanning(a.0, a.1, b.0, b.1);
        let clicked = a == b;
        match tool {
            Tool::RectSelect | Tool::EllipseSelect => {
                let mode = self.select_mode;
                let doc = self.doc()?;
                if clicked {
                    // A click without a drag drops the selection, in every editor.
                    doc.select_none();
                } else if tool == Tool::RectSelect {
                    doc.select(Mask::rect(w, h, rect), mode);
                } else {
                    doc.select(Mask::ellipse(w, h, rect), mode);
                }
            }
            Tool::Crop => {
                self.crop = (!clicked).then_some(rect).and_then(|r| r.clip(w, h));
            }
            Tool::Move => {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                if self.doc()?.translate(dx, dy).is_some() {
                    self.modified = true;
                }
            }
            Tool::Gradient => {
                if clicked {
                    return Ok(());
                }
                let gradient = self.gradient_for(start, end);
                if self.doc()?.gradient(&gradient).is_some() {
                    self.modified = true;
                }
            }
            Tool::Shape => {
                if clicked {
                    return Ok(());
                }
                // Paint's Curve and Pinta's Line/Curve stay open for bending.
                if self.product == Product::Paint && self.shape == ShapeKind::Curve {
                    self.curve = Some(CurveEdit {
                        points: vec![start, start, end, end],
                        bends: 0,
                    });
                    return Ok(());
                }
                if self.product == Product::Pinta && self.shape == ShapeKind::Line {
                    self.curve = Some(CurveEdit {
                        points: vec![start, end],
                        bends: 0,
                    });
                    return Ok(());
                }
                let shape = Shape {
                    kind: self.shape,
                    // Box shapes run through the centres of the corner pixels, so an
                    // outline sits on the dragged rectangle's own edge pixels.
                    points: if self.shape.open() {
                        vec![start, end]
                    } else {
                        vec![
                            centre(rect.x, rect.y),
                            centre(rect.right() - 1, rect.bottom() - 1),
                        ]
                    },
                    outline: (self.outline || self.shape.open()).then_some(self.primary),
                    fill: self.fill.then_some(self.secondary),
                    width: self.size.max(1),
                    antialias: self.antialias,
                };
                if self.doc()?.shape(&shape).is_some() {
                    self.modified = true;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// How far from a point (sub16) a press still takes it: six view pixels.
    fn grab_radius(&self, vw: u32, vh: u32) -> i64 {
        6 * 16 * 100 / i64::from(self.effective_zoom(vw, vh).max(1))
    }

    /// Set the clone source; the next stroke measures its offset from it.
    fn set_source(&mut self, x: i32, y: i32) {
        self.retouch.source = Some((x, y));
        self.retouch.offset = None;
        self.status = Some(format!("Clone source set at {x}, {y}"));
    }

    /// Press with the clone, heal or repair brush: a modifier-click sets the source,
    /// a plain press starts a stroke.
    fn retouch_down(&mut self, p: P16) -> Result<(), String> {
        let brush = self.brush();
        let merged = self.merged;
        let (px, py) = pixel_of(p);
        if self.tool == Tool::Repair {
            self.doc()?
                .begin_stroke_with(brush, p, StrokeMode::Repair, false);
            self.gesture = Some(Gesture::Paint);
            return Ok(());
        }
        let (bit, key) = self.product.source_modifier();
        if self.modifiers & bit != 0 {
            if self.doc()?.bounds().contains(px, py) {
                self.set_source(px, py);
            }
            return Ok(());
        }
        let Some((sx, sy)) = self.retouch.source else {
            self.status = Some(format!("{key}-click to set a clone source first"));
            return Ok(());
        };
        let offset = match (self.retouch.aligned, self.retouch.offset) {
            (true, Some(o)) => o,
            _ => (sx - px, sy - py),
        };
        if self.retouch.aligned {
            self.retouch.offset = Some(offset);
        }
        self.retouch.active = Some(offset);
        let mode = if self.tool == Tool::Heal {
            StrokeMode::Heal {
                dx: offset.0,
                dy: offset.1,
            }
        } else {
            StrokeMode::Clone {
                dx: offset.0,
                dy: offset.1,
            }
        };
        self.doc()?.begin_stroke_with(brush, p, mode, merged);
        self.gesture = Some(Gesture::Paint);
        Ok(())
    }

    /// Press with GIMP's Paths tool (Design mode): take an anchor or handle under the
    /// pointer, Ctrl-click the first anchor to close the path, or add an anchor.
    fn path_down(&mut self, p: P16, vw: u32, vh: u32) -> Result<(), String> {
        let radius = self.grab_radius(vw, vh);
        let ctrl = self.modifiers & MOD_CTRL != 0;
        let path = self.path_edit.get_or_insert_with(Path::default);
        if let Some((index, which)) = path.hit(p, radius) {
            if ctrl && index == 0 && which == 0 && !path.closed && path.anchors.len() >= 2 {
                path.closed = true;
                return Ok(());
            }
            self.gesture = Some(Gesture::Anchor {
                index,
                which,
                fresh: false,
            });
            return Ok(());
        }
        if path.closed {
            self.status = Some("The path is closed; drag its anchors, or delete it".into());
            return Ok(());
        }
        path.anchors.push(Anchor::corner(p));
        self.gesture = Some(Gesture::Anchor {
            index: path.anchors.len() - 1,
            which: 0,
            fresh: true,
        });
        Ok(())
    }

    /// The Paths dock and menu commands: stroke, fill, select, close, delete.
    fn path_command(&mut self, op: &str) -> Result<Vec<AppEffect>, String> {
        if !self.product.tools().contains(&Tool::Paths) {
            return Err(format!("{} has no paths", self.product.name()));
        }
        let path = self
            .path_edit
            .clone()
            .filter(|p| p.anchors.len() >= 2)
            .ok_or("draw a path with the Paths tool first")?;
        match op {
            "stroke" => {
                let values = dialog_params("stroke-path")
                    .iter()
                    .map(|(k, _, _, d)| ((*k).to_owned(), *d))
                    .collect();
                self.show_dialog("stroke-path", values)?;
            }
            "fill" => {
                let shape = path.fill(self.primary, self.antialias);
                self.edit(|d| {
                    d.edit_layer("Fill Path", |layer, sel| shape.draw(layer, sel));
                    Ok(())
                })?;
            }
            "select" => {
                let points = path.flatten();
                self.edit(|d| {
                    d.select_polygon(&points, SelectMode::Replace);
                    Ok(())
                })?;
            }
            "close" => {
                if let Some(p) = &mut self.path_edit {
                    p.closed = true;
                }
            }
            "delete" => self.path_edit = None,
            _ => return Err(format!("unknown path command {op}")),
        }
        Ok(vec![])
    }

    /// Press while a curve is being shaped. Paint: place the next bend. Pinta: take a
    /// control point, add one on the curve, or finish the curve and start a new line.
    fn curve_down(&mut self, p: P16, vw: u32, vh: u32) {
        let radius = self.grab_radius(vw, vh);
        let Some(curve) = &mut self.curve else {
            return;
        };
        if self.product == Product::Paint {
            let index = if curve.bends == 0 { 1 } else { 2 };
            curve.points[index] = p;
            if curve.bends == 0 {
                curve.points[2] = p;
            }
            self.gesture = Some(Gesture::CurvePoint { index });
            return;
        }
        let near = |q: P16| (q.0 - p.0).abs().max((q.1 - p.1).abs()) <= radius;
        if let Some(index) = curve.points.iter().position(|q| near(*q)) {
            self.gesture = Some(Gesture::CurvePoint { index });
            return;
        }
        // On the curve: a new control point between the two it runs between.
        let flat = Path::through(&curve.points, false).flatten();
        let on_curve = flat
            .windows(2)
            .any(|w| cw_raster::mask::near_segment(w[0], w[1], radius, p.0, p.1));
        if on_curve {
            let nearest = |q: &P16| (q.0 - p.0).pow(2) + (q.1 - p.1).pow(2);
            let (at, _) = flat
                .iter()
                .enumerate()
                .min_by_key(|(_, q)| nearest(q))
                .unwrap_or((0, &p));
            // Control points appear in the flattened curve in order; the new point
            // goes after the last one at or before the nearest sample.
            let index = curve
                .points
                .iter()
                .filter(|c| flat.iter().position(|q| q == *c).is_some_and(|i| i <= at))
                .count()
                .clamp(1, curve.points.len());
            curve.points.insert(index, p);
            self.gesture = Some(Gesture::CurvePoint { index });
            return;
        }
        self.commit_curve();
        self.gesture = Some(Gesture::Span {
            tool: Tool::Shape,
            start: p,
            end: p,
        });
    }

    /// Draw the curve being shaped, if there is one.
    fn commit_curve(&mut self) {
        let Some(curve) = self.curve.take() else {
            return;
        };
        let points = if self.product == Product::Paint {
            curve.points
        } else {
            Path::through(&curve.points, false).flatten()
        };
        let shape = Shape {
            kind: if self.product == Product::Paint {
                ShapeKind::Curve
            } else {
                ShapeKind::Polyline
            },
            points,
            outline: Some(self.primary),
            fill: None,
            width: self.size.max(1),
            antialias: self.antialias,
        };
        if let Some(doc) = self.doc.as_mut() {
            if doc.shape(&shape).is_some() {
                self.modified = true;
            }
        }
    }

    /// Whether `target` (a full target) wants the pointer while no button is down:
    /// the canvas, for the pointer position readout and the brush outline.
    pub fn hovers(&self, target: &str) -> bool {
        self.doc.is_some()
            && target
                .strip_prefix(self.prefix())
                .and_then(|t| t.strip_prefix(':'))
                .is_some_and(|c| c.starts_with("canvas:"))
    }
    /// The pointer passed over the canvas at `(x, y)` (relative to it). Returns whether
    /// what is shown changed.
    pub fn hover(&mut self, target: &str, x: i32, y: i32) -> bool {
        let Some(rest) = target
            .strip_prefix(self.prefix())
            .and_then(|t| t.strip_prefix(":canvas:"))
        else {
            return false;
        };
        let mut parts = rest
            .split(':')
            .map(|v| v.parse::<u32>().unwrap_or(1).max(1));
        let (vw, vh) = (parts.next().unwrap_or(1), parts.next().unwrap_or(1));
        if self.doc.is_none() {
            return false;
        }
        let p = self.view_centre(vw, vh, x, y);
        let changed = self.hover != Some(p);
        self.hover = Some(p);
        changed
    }
    /// The image pixel under the pointer, when the pointer (`pointer`, in the frame's
    /// coordinates) is over the canvas painted at `area`.
    pub fn pointer_pixel(
        &self,
        area: cw_scene::Rect,
        pointer: Option<(i32, i32)>,
    ) -> Option<(i32, i32)> {
        let (x, y) = pointer?;
        if !area.contains(x, y) {
            return None;
        }
        self.hover.map(pixel_of)
    }

    pub fn key(&mut self, window: u64, key: &str) -> Result<Vec<AppEffect>, String> {
        // A curve being shaped: Enter draws it, Escape drops it.
        if self.curve.is_some() {
            match key {
                "Enter" => return self.command(window, "curve-edit:commit"),
                "Escape" => return self.command(window, "curve-edit:cancel"),
                _ => {}
            }
        }
        // Text being typed takes its own keys first.
        if let Some(entry) = self.text.as_mut().filter(|t| !t.pending) {
            match key {
                "Backspace" => {
                    entry.text.pop();
                    return Ok(vec![]);
                }
                "Enter" => return Ok(self.commit_text(window)),
                "Escape" => {
                    self.text = None;
                    return Ok(vec![]);
                }
                _ => {}
            }
        }
        if let Some(Panel::Save { name, .. }) = &mut self.panel {
            match key {
                "Backspace" => {
                    name.pop();
                    return Ok(vec![]);
                }
                "Enter" => return self.command(window, "save-confirm"),
                _ => {}
            }
        }
        let key = key.replace("Meta+", "Ctrl+");
        let command = match key.as_str() {
            "Ctrl+z" => "undo",
            "Ctrl+y" | "Ctrl+Shift+z" | "Ctrl+Shift+Z" => "redo",
            "Ctrl+s" => "save",
            "Ctrl+Shift+s" | "Ctrl+Shift+S" => "save-as",
            "Ctrl+Shift+e" | "Ctrl+Shift+E" => "export",
            "Ctrl+o" => "open",
            "Ctrl+n" => "new",
            "Ctrl+a" => "select-all",
            "Ctrl+Shift+a" | "Ctrl+Shift+A" | "Ctrl+d" => "select-none",
            "Ctrl+i" => "select-invert",
            "Ctrl+c" => "copy",
            "Ctrl+x" => "cut",
            "Ctrl+v" => "paste",
            "Delete" => "delete",
            "Ctrl+=" | "Ctrl++" => "zoom:in",
            "Ctrl+-" => "zoom:out",
            "Ctrl+0" => "zoom:fit",
            "Enter" if self.crop.is_some() => "crop-apply",
            "Enter" if matches!(self.panel, Some(Panel::Dialog { .. })) => "apply",
            "Escape" => "close-panel",
            other => return Err(format!("{} does not use {other}", self.product.name())),
        };
        self.command(window, command)
    }

    /// Semantic projection shared by every product.
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: String| cw_protocol::PageAction {
            method: "APP".into(),
            url,
            fields: Default::default(),
        };
        page.elements.push(E::Heading {
            id: format!("{}-document", self.prefix()),
            text: self.title(),
            level: 2,
        });
        let mut facts = vec![];
        if let Some(doc) = &self.doc {
            facts.push(format!("{} x {} pixels", doc.width(), doc.height()));
            facts.push(format!(
                "{} layer(s), active: {}",
                doc.layers().len(),
                doc.active_layer().name
            ));
            if let Some(r) = doc.selection().and_then(Mask::bounds) {
                facts.push(format!("selection {}x{} at {},{}", r.w, r.h, r.x, r.y));
            }
        }
        facts.push(format!("tool: {}", self.tool.id()));
        if let Some((x, y)) = self.hover.map(pixel_of) {
            facts.push(format!("pointer at {x}, {y}"));
        }
        if let (Some((x, y)), true) = (self.retouch.source, self.tool.retouches()) {
            facts.push(format!("clone source {x}, {y}"));
        }
        facts.push(format!("color: #{}", cw_raster::hex(self.primary)));
        if let Some(status) = &self.status {
            facts.push(status.clone());
        }
        page.elements.push(E::Text {
            id: format!("{}-state", self.prefix()),
            text: facts.join("; "),
        });
        for tool in self.product.tools() {
            let target = self.target(&format!("tool:{}", tool.id()));
            page.elements.push(E::Button {
                id: target.clone(),
                text: tool.id().into(),
                action: act(target),
            });
        }
        for command in ["undo", "redo", "save", "save-as", "open"] {
            let target = self.target(command);
            page.elements.push(E::Button {
                id: target.clone(),
                text: command.into(),
                action: act(target),
            });
        }
    }

    /// Rendering is each product's own.
    pub fn render(&self, p: &mut crate::desktop_scene::Painter, env: &crate::AppEnv<'_>) {
        match self.product {
            Product::Paint => paint::render(self, p, env),
            Product::Preview => preview::render(self, p, env),
            Product::Pixelmator => pixelmator::render(self, p, env),
            Product::Gimp => gimp::render(self, p, env),
            Product::Pinta => pinta::render(self, p, env),
            Product::Sketchbook => sketchbook::render(self, p, env),
            Product::IosPhotos | Product::GooglePhotos => render_photo_editor(self, p, env),
        }
    }
}

/// A desktop or phone application whose whole content is one [`Studio`].
macro_rules! editor_app {
    ($name:ident, $kind:literal, $product:expr) => {
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Box<Studio>);
        impl std::ops::Deref for $name {
            type Target = Studio;
            fn deref(&self) -> &Studio {
                &self.0
            }
        }
        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Studio {
                &mut self.0
            }
        }
        impl $name {
            pub const KIND: &'static str = $kind;
            pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
                let (studio, effects) = Studio::launch($product, argument, window);
                (Self(Box::new(studio)), effects)
            }
            pub fn kind(&self) -> &'static str {
                Self::KIND
            }
            pub fn title(&self, _theme: DesktopTheme) -> String {
                self.0.title()
            }
            pub fn document(&self) -> String {
                self.0.path.clone()
            }
            pub fn caption(&self) -> String {
                self.0.caption()
            }
            pub fn modified(&self) -> bool {
                self.0.modified
            }
            pub fn text(&mut self, text: &str) -> Result<(), String> {
                self.0.text(text)
            }
            pub fn key(
                &mut self,
                window: u64,
                key: &str,
                _clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                self.0.key(window, key)
            }
            pub fn click(
                &mut self,
                window: u64,
                target: &str,
                _clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                let command = target
                    .strip_prefix(self.0.product.prefix())
                    .and_then(|t| t.strip_prefix(':'))
                    .ok_or_else(|| format!("interaction does not belong to {}", Self::KIND))?
                    .to_owned();
                self.0.command(window, &command)
            }
            pub fn http(
                &mut self,
                _window: u64,
                _tag: &str,
                _status: u16,
                _body: &str,
            ) -> Result<Vec<AppEffect>, String> {
                Err(format!("{} makes no network requests", Self::KIND))
            }
            pub fn offline(&mut self, _tag: &str, reason: &str) {
                self.0.listing_failed(reason);
            }
            pub fn page(&self, page: &mut cw_protocol::Page) {
                self.0.page(page)
            }
            pub fn render(&self, p: &mut crate::desktop_scene::Painter, env: &crate::AppEnv<'_>) {
                self.0.render(p, env)
            }
        }
    };
}

editor_app!(Paint, "paint", Product::Paint);
editor_app!(Preview, "preview", Product::Preview);
editor_app!(Pixelmator, "pixelmator", Product::Pixelmator);
editor_app!(Gimp, "gimp", Product::Gimp);
editor_app!(Pinta, "pinta", Product::Pinta);
editor_app!(Sketchbook, "sketchbook", Product::Sketchbook);

#[cfg(test)]
mod tests;
