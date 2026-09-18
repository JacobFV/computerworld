//! The document an editor holds: a stack of layers, a selection, a stroke in progress and
//! the undo history. Every edit goes through here so it is recorded exactly once, with
//! just enough of the old state to take it back — the pixels it touched, not a copy of
//! the whole image.
use crate::adjust::{self, Adjustment};
use crate::blend::{self, BlendMode};
use crate::draw::{self, Brush, BrushKind, Shape};
use crate::filter::{self, Filter};
use crate::mask::{Mask, SelectMode, P16};
use crate::transform::{self, Resample};
use crate::{check_size, Canvas, IRect, Rgba, TRANSPARENT};
use serde::{Deserialize, Serialize};

/// Most layers a document may hold.
pub const LAYER_LIMIT: usize = 32;
/// Undo depth, and the raw pixel bytes the history may retain across all its steps.
pub const HISTORY_LIMIT: usize = 50;
pub const HISTORY_BYTES: usize = 48 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// Percent, 0..=100.
    pub opacity: u8,
    pub blend: BlendMode,
    pub canvas: Canvas,
}
impl Layer {
    pub fn new(name: impl Into<String>, canvas: Canvas) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity: 100,
            blend: BlendMode::Normal,
            canvas,
        }
    }
    fn meta(&self) -> LayerMeta {
        LayerMeta {
            name: self.name.clone(),
            visible: self.visible,
            opacity: self.opacity,
            blend: self.blend,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LayerMeta {
    name: String,
    visible: bool,
    opacity: u8,
    blend: BlendMode,
}

/// Everything structural, for edits that change the size or the stack.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    selection: Option<Mask>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Change {
    /// The pixels a local edit overwrote, at their place in one layer.
    Pixels {
        layer: usize,
        x: i32,
        y: i32,
        pixels: Canvas,
    },
    /// A layer's name, visibility, opacity or blend mode.
    Meta {
        layer: usize,
        meta: LayerMeta,
    },
    Selection {
        selection: Option<Mask>,
    },
    State {
        state: Box<State>,
    },
}
impl Change {
    fn bytes(&self) -> usize {
        match self {
            Self::Pixels { pixels, .. } => pixels.pixels().len(),
            Self::Meta { .. } => 64,
            Self::Selection { selection } => selection.as_ref().map_or(0, |m| m.data().len()),
            Self::State { state } => {
                state
                    .layers
                    .iter()
                    .map(|l| l.canvas.pixels().len())
                    .sum::<usize>()
                    + (state.width * state.height) as usize
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct History {
    undo: Vec<(String, Change)>,
    redo: Vec<(String, Change)>,
}

/// A brush stroke between pointer down and pointer up. The layer as it was when the
/// stroke began is kept, and the stroke's own coverage accumulates in `mask`, so
/// overlapping dabs never build past the brush's opacity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Stroke {
    brush: Brush,
    layer: usize,
    before: Canvas,
    mask: Mask,
    last: P16,
    carry: i64,
    touched: Option<IRect>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    selection: Option<Mask>,
    #[serde(default)]
    history: History,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stroke: Option<Stroke>,
}

/// Transparent pixels are shown over this checkerboard, 8 view pixels to a square.
pub fn checker(x: i32, y: i32) -> Rgba {
    if ((x >> 3) + (y >> 3)) & 1 == 0 {
        [255, 255, 255, 255]
    } else {
        [204, 204, 204, 255]
    }
}

impl Document {
    /// A new image with one layer, filled with `background` (transparent if `None`).
    pub fn new(width: u32, height: u32, background: Option<Rgba>) -> Result<Self, String> {
        check_size(width, height)?;
        let canvas = Canvas::filled(width, height, background.unwrap_or(TRANSPARENT));
        Ok(Self::from_layer(Layer::new("Background", canvas)))
    }
    /// An opened picture, as a single layer called `name`.
    pub fn from_canvas(canvas: Canvas, name: &str) -> Self {
        Self::from_layer(Layer::new(name, canvas))
    }
    fn from_layer(layer: Layer) -> Self {
        Self {
            width: layer.canvas.width(),
            height: layer.canvas.height(),
            layers: vec![layer],
            active: 0,
            selection: None,
            history: History::default(),
            stroke: None,
        }
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
    /// Bottom first.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }
    pub fn active(&self) -> usize {
        self.active
    }
    pub fn active_layer(&self) -> &Layer {
        &self.layers[self.active]
    }
    pub fn selection(&self) -> Option<&Mask> {
        self.selection.as_ref()
    }
    pub fn stroking(&self) -> bool {
        self.stroke.is_some()
    }
    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }
    /// Names of the steps that can be undone, oldest first (GIMP's Undo History).
    pub fn undo_steps(&self) -> Vec<&str> {
        self.history.undo.iter().map(|(l, _)| l.as_str()).collect()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo.last().map(|(l, _)| l.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo.last().map(|(l, _)| l.as_str())
    }

    // ----- reading -----------------------------------------------------------------

    /// The visible layers composited at one pixel.
    pub fn pixel(&self, x: i32, y: i32) -> Rgba {
        let mut out = TRANSPARENT;
        for layer in self.layers.iter().filter(|l| l.visible) {
            out = blend::composite(
                out,
                layer.canvas.get(x, y),
                crate::fmath::percent255(layer.opacity),
                layer.blend,
            );
        }
        out
    }
    /// The flattened image: what is exported and what a merged sample reads.
    pub fn composite(&self) -> Canvas {
        let mut out = Canvas::new(self.width, self.height);
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                out.set(x, y, self.pixel(x, y));
            }
        }
        out
    }
    /// A view of the image: `width` x `height` view pixels, view pixel `(vx, vy)` showing
    /// the image at the centre of its span, sub16 `origin + (v + 1/2) * num / den` (nearest
    /// sample; the ratio is exact, so a view at 67% does not drift). Transparency shows
    /// the checkerboard, and anything outside the image is `outside`.
    pub fn view(
        &self,
        origin: P16,
        (num, den): (i64, i64),
        width: u32,
        height: u32,
        outside: Rgba,
    ) -> Canvas {
        let mut out = Canvas::new(width.max(1), height.max(1));
        let (num, den) = (num.max(1), den.max(1));
        let at = |o: i64, v: i64| (o + (2 * v + 1) * num / (2 * den)).div_euclid(16);
        for vy in 0..height as i64 {
            let iy = at(origin.1, vy);
            for vx in 0..width as i64 {
                let ix = at(origin.0, vx);
                let p = if ix < 0
                    || iy < 0
                    || ix >= i64::from(self.width)
                    || iy >= i64::from(self.height)
                {
                    outside
                } else {
                    let px = self.pixel(ix as i32, iy as i32);
                    if px[3] == 255 {
                        px
                    } else {
                        blend::composite(checker(vx as i32, vy as i32), px, 255, BlendMode::Normal)
                    }
                };
                out.set(vx as i32, vy as i32, p);
            }
        }
        out
    }

    // ----- history -----------------------------------------------------------------

    fn record(&mut self, label: &str, change: Change) {
        self.history.redo.clear();
        self.history.undo.push((label.into(), change));
        let mut total: usize = self.history.undo.iter().map(|(_, c)| c.bytes()).sum();
        while self.history.undo.len() > HISTORY_LIMIT
            || (total > HISTORY_BYTES && self.history.undo.len() > 1)
        {
            total -= self.history.undo[0].1.bytes();
            self.history.undo.remove(0);
        }
    }
    fn state(&self) -> State {
        State {
            width: self.width,
            height: self.height,
            layers: self.layers.clone(),
            active: self.active,
            selection: self.selection.clone(),
        }
    }
    fn restore(&mut self, state: State) {
        self.width = state.width;
        self.height = state.height;
        self.layers = state.layers;
        self.active = state.active.min(self.layers.len() - 1);
        self.selection = state.selection;
    }
    /// Swap `change` with the document, returning the change that swaps it back.
    fn swap(&mut self, change: Change) -> Change {
        match change {
            Change::Pixels {
                layer,
                x,
                y,
                pixels,
            } => {
                let canvas = &mut self.layers[layer].canvas;
                let current = canvas
                    .region(IRect::new(x, y, pixels.width(), pixels.height()))
                    .unwrap_or_else(|| pixels.clone());
                canvas.put(x, y, &pixels);
                Change::Pixels {
                    layer,
                    x,
                    y,
                    pixels: current,
                }
            }
            Change::Meta { layer, meta } => {
                let l = &mut self.layers[layer];
                let current = l.meta();
                l.name = meta.name;
                l.visible = meta.visible;
                l.opacity = meta.opacity;
                l.blend = meta.blend;
                Change::Meta {
                    layer,
                    meta: current,
                }
            }
            Change::Selection { selection } => Change::Selection {
                selection: std::mem::replace(&mut self.selection, selection),
            },
            Change::State { state } => {
                let current = self.state();
                self.restore(*state);
                Change::State {
                    state: Box::new(current),
                }
            }
        }
    }
    pub fn undo(&mut self) -> Result<String, String> {
        self.end_stroke();
        let (label, change) = self.history.undo.pop().ok_or("nothing to undo")?;
        let back = self.swap(change);
        self.history.redo.push((label.clone(), back));
        Ok(label)
    }
    pub fn redo(&mut self) -> Result<String, String> {
        let (label, change) = self.history.redo.pop().ok_or("nothing to redo")?;
        let back = self.swap(change);
        self.history.undo.push((label.clone(), back));
        Ok(label)
    }
    /// Run a local edit on the active layer and record the pixels it touched.
    fn edit_active(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Canvas, Option<&Mask>) -> Option<IRect>,
    ) -> Option<IRect> {
        self.end_stroke();
        let index = self.active;
        let before = self.layers[index].canvas.clone();
        let touched = f(&mut self.layers[index].canvas, self.selection.as_ref())?;
        let pixels = before.region(touched)?;
        if self.layers[index].canvas.region(touched).as_ref() == Some(&pixels) {
            return None;
        }
        self.record(
            label,
            Change::Pixels {
                layer: index,
                x: touched.x,
                y: touched.y,
                pixels,
            },
        );
        Some(touched)
    }
    fn structural(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        self.end_stroke();
        let before = self.state();
        f(self)?;
        if self.state() != before {
            self.record(
                label,
                Change::State {
                    state: Box::new(before),
                },
            );
        }
        Ok(())
    }

    // ----- painting ----------------------------------------------------------------

    /// Pointer down with a brush, at sub16 `p`.
    pub fn begin_stroke(&mut self, brush: Brush, p: P16) {
        self.end_stroke();
        let layer = self.active;
        let before = self.layers[layer].canvas.clone();
        self.stroke = Some(Stroke {
            brush,
            layer,
            before,
            mask: Mask::empty(self.width, self.height),
            last: p,
            carry: 0,
            touched: None,
        });
        self.dab_at(&[p]);
    }
    /// Pointer moved while stroking.
    pub fn stroke_to(&mut self, p: P16) {
        let Some(stroke) = &mut self.stroke else {
            return;
        };
        let (points, carry) =
            draw::dabs_along(stroke.last, p, stroke.brush.spacing16(), stroke.carry);
        stroke.last = p;
        stroke.carry = carry;
        self.dab_at(&points);
    }
    fn dab_at(&mut self, points: &[P16]) {
        let Some(stroke) = &mut self.stroke else {
            return;
        };
        let mut area: Option<IRect> = None;
        for p in points {
            if let Some(r) = stroke.brush.dab(&mut stroke.mask, p.0, p.1) {
                area = Some(area.map_or(r, |a| a.union(&r)));
            }
        }
        let Some(area) = area else {
            return;
        };
        stroke.touched = Some(stroke.touched.map_or(area, |t| t.union(&area)));
        let brush = stroke.brush;
        let (mask, before) = (&stroke.mask, &stroke.before);
        draw::apply_coverage(
            &mut self.layers[stroke.layer].canvas,
            Some(before),
            area,
            self.selection.as_ref(),
            brush.kind,
            brush.color,
            brush.opacity,
            brush.blend,
            |x, y| mask.get(x, y),
        );
    }
    /// Pointer up: the stroke becomes one undoable step.
    pub fn end_stroke(&mut self) -> Option<IRect> {
        let stroke = self.stroke.take()?;
        let touched = stroke.touched?;
        let pixels = stroke.before.region(touched)?;
        let label = match stroke.brush.kind {
            BrushKind::Erase => "Eraser",
            BrushKind::Paint if !stroke.brush.antialias => "Pencil",
            BrushKind::Paint => "Brush",
        };
        self.record(
            label,
            Change::Pixels {
                layer: stroke.layer,
                x: touched.x,
                y: touched.y,
                pixels,
            },
        );
        Some(touched)
    }
    pub fn fill(
        &mut self,
        x: i32,
        y: i32,
        color: Rgba,
        tolerance: u8,
        merged: bool,
    ) -> Option<IRect> {
        let sample = if merged {
            self.composite()
        } else {
            self.layers[self.active].canvas.clone()
        };
        self.edit_active("Fill", |layer, sel| {
            draw::bucket_fill(layer, &sample, x, y, color, tolerance, sel)
        })
    }
    pub fn shape(&mut self, shape: &Shape) -> Option<IRect> {
        self.edit_active("Shape", |layer, sel| shape.draw(layer, sel))
    }
    /// Stamp a coverage mask (rasterised text) in `color` at `(x, y)`.
    pub fn stamp(&mut self, x: i32, y: i32, alpha: &Mask, color: Rgba) -> Option<IRect> {
        self.edit_active("Text", |layer, sel| {
            draw::stamp(layer, x, y, alpha, color, sel)
        })
    }
    pub fn adjust(&mut self, label: &str, adj: &Adjustment) -> Option<IRect> {
        self.edit_active(label, |layer, sel| adjust::apply(layer, adj, sel))
    }
    pub fn filter(&mut self, label: &str, f: &Filter) -> Option<IRect> {
        self.edit_active(label, |layer, sel| filter::apply(layer, f, sel))
    }
    /// Clear what is selected on the active layer to transparency.
    pub fn delete_selection(&mut self) -> Option<IRect> {
        let sel = self.selection.clone()?;
        self.edit_active("Delete", |layer, _| {
            let area = sel.bounds()?;
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    let k = u32::from(sel.get(x, y));
                    layer.set(x, y, blend::erase(layer.get(x, y), k));
                }
            }
            Some(area)
        })
    }
    /// Move the active layer's pixels by `(dx, dy)`; what leaves the canvas is lost.
    pub fn translate(&mut self, dx: i32, dy: i32) -> Option<IRect> {
        if dx == 0 && dy == 0 {
            return None;
        }
        self.edit_active("Move", |layer, _| {
            let src = layer.clone();
            let mut out = Canvas::new(src.width(), src.height());
            out.put(dx, dy, &src);
            *layer = out;
            Some(layer.bounds())
        })
    }
    /// The composited colour under `(x, y)`, or the active layer's own with `merged` off.
    pub fn pick(&self, x: i32, y: i32, merged: bool) -> Option<Rgba> {
        if !self.bounds().contains(x, y) {
            return None;
        }
        Some(if merged {
            self.pixel(x, y)
        } else {
            self.layers[self.active].canvas.get(x, y)
        })
    }

    // ----- selection ---------------------------------------------------------------

    pub fn select(&mut self, mask: Mask, mode: SelectMode) {
        self.end_stroke();
        let before = self.selection.clone();
        let mut next = match (&self.selection, mode) {
            (_, SelectMode::Replace) | (None, SelectMode::Add) => mask,
            (None, SelectMode::Subtract) => return,
            (None, SelectMode::Intersect) => Mask::empty(self.width, self.height),
            (Some(current), mode) => {
                let mut m = current.clone();
                m.combine(&mask, mode);
                m
            }
        };
        let next = if next.is_empty() {
            None
        } else {
            next.combine(&Mask::full(self.width, self.height), SelectMode::Intersect);
            Some(next)
        };
        if next != before {
            self.selection = next;
            self.record("Selection", Change::Selection { selection: before });
        }
    }
    pub fn select_all(&mut self) {
        self.select(Mask::full(self.width, self.height), SelectMode::Replace);
    }
    pub fn select_none(&mut self) {
        if self.selection.is_some() {
            let before = self.selection.take();
            self.record("Select None", Change::Selection { selection: before });
        }
    }
    pub fn invert_selection(&mut self) {
        let mut m = self
            .selection
            .clone()
            .unwrap_or_else(|| Mask::empty(self.width, self.height));
        m.invert();
        self.select(m, SelectMode::Replace);
    }
    /// Magic wand on the active layer (or the merged image).
    pub fn select_similar(
        &mut self,
        x: i32,
        y: i32,
        tolerance: u8,
        merged: bool,
        mode: SelectMode,
    ) {
        let sample = if merged {
            self.composite()
        } else {
            self.layers[self.active].canvas.clone()
        };
        self.select(Mask::flood(&sample, x, y, tolerance), mode);
    }

    // ----- clipboard ---------------------------------------------------------------

    /// The selected pixels of the active layer, cropped to the selection (the whole
    /// layer when nothing is selected).
    pub fn copy(&self) -> Canvas {
        let layer = &self.layers[self.active].canvas;
        match &self.selection {
            None => layer.clone(),
            Some(sel) => {
                let r = sel.bounds().unwrap_or(self.bounds());
                let mut out = layer.region(r).unwrap_or_else(|| Canvas::new(1, 1));
                for y in 0..out.height() as i32 {
                    for x in 0..out.width() as i32 {
                        let p = out.get(x, y);
                        let k = u32::from(sel.get(r.x + x, r.y + y));
                        out.set(x, y, blend::erase(p, 255 - k));
                    }
                }
                out
            }
        }
    }
    /// Paste as a new layer above the active one, its top-left at `(x, y)`.
    pub fn paste(&mut self, pixels: &Canvas, x: i32, y: i32) -> Result<(), String> {
        let (w, h) = (self.width, self.height);
        self.structural("Paste", |d| {
            if d.layers.len() >= LAYER_LIMIT {
                return Err(format!("at most {LAYER_LIMIT} layers"));
            }
            let mut canvas = Canvas::new(w, h);
            canvas.put(x, y, pixels);
            d.layers
                .insert(d.active + 1, Layer::new("Pasted Layer", canvas));
            d.active += 1;
            let r = IRect::new(x, y, pixels.width(), pixels.height());
            d.selection = r.clip(w, h).map(|r| Mask::rect(w, h, r));
            Ok(())
        })
    }

    // ----- image -------------------------------------------------------------------

    pub fn crop(&mut self, r: IRect) -> Result<(), String> {
        let r = r
            .clip(self.width, self.height)
            .ok_or("nothing to crop to")?;
        self.structural("Crop Image", |d| {
            for layer in &mut d.layers {
                layer.canvas = transform::crop(&layer.canvas, r);
            }
            d.width = r.w;
            d.height = r.h;
            d.selection = None;
            Ok(())
        })
    }
    pub fn crop_to_selection(&mut self) -> Result<(), String> {
        let r = self
            .selection
            .as_ref()
            .and_then(Mask::bounds)
            .ok_or("nothing is selected")?;
        self.crop(r)
    }
    pub fn resize(&mut self, width: u32, height: u32, filter: Resample) -> Result<(), String> {
        check_size(width, height)?;
        self.structural("Scale Image", |d| {
            for layer in &mut d.layers {
                layer.canvas = transform::resize(&layer.canvas, width, height, filter);
            }
            d.width = width;
            d.height = height;
            d.selection = None;
            Ok(())
        })
    }
    /// Change the canvas without scaling: the image keeps its place at the top left.
    pub fn resize_canvas(&mut self, width: u32, height: u32, fill: Rgba) -> Result<(), String> {
        check_size(width, height)?;
        self.structural("Canvas Size", |d| {
            for (i, layer) in d.layers.iter_mut().enumerate() {
                let mut c = Canvas::filled(width, height, if i == 0 { fill } else { TRANSPARENT });
                c.put(0, 0, &layer.canvas);
                layer.canvas = c;
            }
            d.width = width;
            d.height = height;
            d.selection = None;
            Ok(())
        })
    }
    /// Clockwise quarter turns of the whole image.
    pub fn rotate_quarter(&mut self, turns: u32) -> Result<(), String> {
        self.structural("Rotate", |d| {
            for layer in &mut d.layers {
                layer.canvas = transform::rotate_quarter(&layer.canvas, turns);
            }
            if turns % 2 == 1 {
                std::mem::swap(&mut d.width, &mut d.height);
            }
            d.selection = None;
            Ok(())
        })
    }
    pub fn flip(&mut self, horizontal: bool) -> Result<(), String> {
        self.structural(
            if horizontal {
                "Flip Horizontally"
            } else {
                "Flip Vertically"
            },
            |d| {
                for layer in &mut d.layers {
                    layer.canvas = transform::flip(&layer.canvas, horizontal);
                }
                d.selection = None;
                Ok(())
            },
        )
    }
    /// Arbitrary rotation, growing the canvas to fit when `expand`.
    pub fn rotate(&mut self, centidegrees: i32, expand: bool) -> Result<(), String> {
        let first = transform::rotate(&self.layers[0].canvas, centidegrees, expand);
        check_size(first.width(), first.height())?;
        self.structural("Rotate", |d| {
            for layer in &mut d.layers {
                layer.canvas = transform::rotate(&layer.canvas, centidegrees, expand);
            }
            d.width = first.width();
            d.height = first.height();
            d.selection = None;
            Ok(())
        })
    }
    /// Rotate and zoom so no corner is empty, keeping the size (a phone's Straighten).
    pub fn straighten(&mut self, centidegrees: i32) -> Result<(), String> {
        self.structural("Straighten", |d| {
            for layer in &mut d.layers {
                layer.canvas = transform::straighten(&layer.canvas, centidegrees);
            }
            d.selection = None;
            Ok(())
        })
    }
    /// Replace the whole image with `canvas` as one layer, recorded as a single step.
    pub fn replace(&mut self, label: &str, canvas: Canvas) -> Result<(), String> {
        check_size(canvas.width(), canvas.height())?;
        self.structural(label, |d| {
            let name = d.layers[0].name.clone();
            d.width = canvas.width();
            d.height = canvas.height();
            d.layers = vec![Layer::new(name, canvas)];
            d.active = 0;
            d.selection = None;
            Ok(())
        })
    }
    pub fn flatten(&mut self) -> Result<(), String> {
        let flat = self.composite();
        self.structural("Flatten Image", |d| {
            let name = d.layers[0].name.clone();
            d.layers = vec![Layer::new(name, flat)];
            d.active = 0;
            Ok(())
        })
    }

    // ----- layers ------------------------------------------------------------------

    pub fn add_layer(&mut self, name: &str) -> Result<(), String> {
        let (w, h) = (self.width, self.height);
        self.structural("New Layer", |d| {
            if d.layers.len() >= LAYER_LIMIT {
                return Err(format!("at most {LAYER_LIMIT} layers"));
            }
            d.layers
                .insert(d.active + 1, Layer::new(name, Canvas::new(w, h)));
            d.active += 1;
            Ok(())
        })
    }
    pub fn duplicate_layer(&mut self) -> Result<(), String> {
        self.structural("Duplicate Layer", |d| {
            if d.layers.len() >= LAYER_LIMIT {
                return Err(format!("at most {LAYER_LIMIT} layers"));
            }
            let mut copy = d.layers[d.active].clone();
            copy.name = format!("{} copy", copy.name);
            d.layers.insert(d.active + 1, copy);
            d.active += 1;
            Ok(())
        })
    }
    pub fn delete_layer(&mut self) -> Result<(), String> {
        self.structural("Delete Layer", |d| {
            if d.layers.len() <= 1 {
                return Err("an image keeps at least one layer".into());
            }
            d.layers.remove(d.active);
            d.active = d.active.saturating_sub(1).min(d.layers.len() - 1);
            Ok(())
        })
    }
    /// Move the active layer one place up (`true`) or down the stack.
    pub fn move_layer(&mut self, up: bool) -> Result<(), String> {
        self.structural(if up { "Raise Layer" } else { "Lower Layer" }, |d| {
            let to = if up {
                d.active + 1
            } else {
                d.active
                    .checked_sub(1)
                    .ok_or("the layer is already at the bottom")?
            };
            if to >= d.layers.len() {
                return Err("the layer is already at the top".into());
            }
            d.layers.swap(d.active, to);
            d.active = to;
            Ok(())
        })
    }
    pub fn merge_down(&mut self) -> Result<(), String> {
        self.structural("Merge Down", |d| {
            let below = d
                .active
                .checked_sub(1)
                .ok_or("there is no layer below to merge into")?;
            let top = d.layers.remove(d.active);
            let target = &mut d.layers[below];
            let mut merged = target.canvas.clone();
            if top.visible {
                merged.draw(
                    0,
                    0,
                    &top.canvas,
                    top.blend,
                    crate::fmath::percent255(top.opacity),
                );
            }
            target.canvas = merged;
            d.active = below;
            Ok(())
        })
    }
    pub fn select_layer(&mut self, index: usize) -> Result<(), String> {
        if index >= self.layers.len() {
            return Err("no such layer".into());
        }
        self.end_stroke();
        self.active = index;
        Ok(())
    }
    fn set_meta(
        &mut self,
        index: usize,
        label: &str,
        f: impl FnOnce(&mut Layer),
    ) -> Result<(), String> {
        self.end_stroke();
        let layer = self.layers.get_mut(index).ok_or("no such layer")?;
        let before = layer.meta();
        f(layer);
        if layer.meta() != before {
            self.record(
                label,
                Change::Meta {
                    layer: index,
                    meta: before,
                },
            );
        }
        Ok(())
    }
    pub fn set_visible(&mut self, index: usize, visible: bool) -> Result<(), String> {
        self.set_meta(index, "Layer Visibility", |l| l.visible = visible)
    }
    pub fn set_opacity(&mut self, index: usize, percent: u8) -> Result<(), String> {
        self.set_meta(index, "Layer Opacity", |l| l.opacity = percent.min(100))
    }
    pub fn set_blend(&mut self, index: usize, mode: BlendMode) -> Result<(), String> {
        self.set_meta(index, "Layer Mode", |l| l.blend = mode)
    }
    pub fn rename_layer(&mut self, index: usize, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a layer needs a name".into());
        }
        self.set_meta(index, "Rename Layer", |l| {
            l.name = name.chars().take(64).collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ShapeKind, BLACK, WHITE};
    const RED: Rgba = [255, 0, 0, 255];
    fn at(x: i64, y: i64) -> P16 {
        (x * 16 + 8, y * 16 + 8)
    }
    fn pencil(color: Rgba) -> Brush {
        Brush {
            size: 1,
            antialias: false,
            color,
            ..Brush::default()
        }
    }
    #[test]
    fn a_stroke_draws_along_the_drag_and_undoes_as_one_step() {
        let mut d = Document::new(10, 5, Some(WHITE)).unwrap();
        d.begin_stroke(pencil(BLACK), at(1, 2));
        d.stroke_to(at(5, 2));
        d.stroke_to(at(8, 2));
        assert!(d.stroking());
        let touched = d.end_stroke().unwrap();
        assert_eq!(touched, IRect::new(1, 2, 8, 1));
        for x in 1..=8 {
            assert_eq!(d.pixel(x, 2), BLACK, "gap at {x}");
        }
        assert_eq!(d.pixel(0, 2), WHITE);
        assert_eq!(d.pixel(9, 2), WHITE);
        assert_eq!(d.undo_label(), Some("Pencil"));
        d.undo().unwrap();
        assert_eq!(d.composite(), Canvas::filled(10, 5, WHITE));
        d.redo().unwrap();
        assert_eq!(d.pixel(4, 2), BLACK);
        assert!(!d.can_redo());
    }
    #[test]
    fn stroke_opacity_is_a_ceiling_not_a_buildup() {
        let mut d = Document::new(20, 5, Some(WHITE)).unwrap();
        let brush = Brush {
            size: 6,
            opacity: 128,
            color: BLACK,
            ..Brush::default()
        };
        d.begin_stroke(brush, at(3, 2));
        // Back and forth over the same pixels many times.
        for _ in 0..5 {
            d.stroke_to(at(15, 2));
            d.stroke_to(at(3, 2));
        }
        d.end_stroke();
        assert_eq!(d.pixel(9, 2), [127, 127, 127, 255]);
    }
    #[test]
    fn eraser_removes_alpha_through_the_selection() {
        let mut d = Document::new(10, 1, Some(RED)).unwrap();
        d.select(
            Mask::rect(10, 1, IRect::new(0, 0, 5, 1)),
            SelectMode::Replace,
        );
        let eraser = Brush {
            kind: BrushKind::Erase,
            size: 1,
            antialias: false,
            ..Brush::default()
        };
        d.begin_stroke(eraser, at(0, 0));
        d.stroke_to(at(9, 0));
        d.end_stroke();
        assert_eq!(d.pixel(4, 0), TRANSPARENT);
        assert_eq!(d.pixel(5, 0), RED, "outside the selection");
        d.undo().unwrap();
        assert_eq!(d.pixel(4, 0), RED);
        d.undo().unwrap();
        assert!(
            d.selection().is_none(),
            "the selection was a step of its own"
        );
    }
    #[test]
    fn layers_blend_reorder_merge_and_undo() {
        let mut d = Document::new(4, 4, Some([100, 100, 100, 255])).unwrap();
        d.add_layer("Layer 1").unwrap();
        d.fill(0, 0, [200, 0, 0, 255], 0, false).unwrap();
        assert_eq!(d.pixel(0, 0), [200, 0, 0, 255]);
        d.set_blend(1, BlendMode::Multiply).unwrap();
        assert_eq!(d.pixel(0, 0), [78, 0, 0, 255]);
        d.set_opacity(1, 50).unwrap();
        // Half of the multiply over grey: (78 + 100) / 2 = 89.
        assert_eq!(d.pixel(0, 0), [89, 50, 50, 255]);
        d.set_visible(1, false).unwrap();
        assert_eq!(d.pixel(0, 0), [100, 100, 100, 255]);
        d.undo().unwrap();
        d.move_layer(false).unwrap();
        assert_eq!(d.active(), 0);
        assert_eq!(d.layers()[1].name, "Background");
        assert_eq!(d.pixel(0, 0), [100, 100, 100, 255], "grey now covers red");
        d.undo().unwrap();
        d.merge_down().unwrap();
        assert_eq!(d.layers().len(), 1);
        assert_eq!(d.pixel(0, 0), [89, 50, 50, 255], "merging keeps the look");
        d.undo().unwrap();
        assert_eq!(d.layers().len(), 2);
        d.duplicate_layer().unwrap();
        assert_eq!(d.layers()[2].name, "Layer 1 copy");
        d.delete_layer().unwrap();
        d.delete_layer().unwrap();
        assert!(d.delete_layer().is_err());
        d.rename_layer(0, "Paper").unwrap();
        assert_eq!(d.undo().unwrap(), "Rename Layer");
    }
    #[test]
    fn image_operations_resize_the_document_and_undo_whole() {
        let mut d = Document::new(6, 4, Some(WHITE)).unwrap();
        d.shape(&Shape {
            kind: ShapeKind::Rectangle,
            points: vec![(0, 0), (16, 16)],
            outline: None,
            fill: Some(RED),
            width: 1,
            antialias: false,
        })
        .unwrap();
        assert_eq!(d.pixel(0, 0), RED);
        d.rotate_quarter(1).unwrap();
        assert_eq!((d.width(), d.height()), (4, 6));
        assert_eq!(d.pixel(3, 0), RED);
        d.flip(true).unwrap();
        assert_eq!(d.pixel(0, 0), RED);
        d.resize(8, 12, Resample::Nearest).unwrap();
        assert_eq!(d.pixel(1, 1), RED);
        d.select(
            Mask::rect(8, 12, IRect::new(0, 0, 2, 2)),
            SelectMode::Replace,
        );
        d.crop_to_selection().unwrap();
        assert_eq!((d.width(), d.height()), (2, 2));
        assert!(d.selection().is_none());
        for _ in 0..5 {
            d.undo().unwrap();
        }
        assert_eq!((d.width(), d.height()), (6, 4));
        assert_eq!(d.pixel(0, 0), RED);
        d.resize_canvas(8, 5, BLACK).unwrap();
        assert_eq!(d.pixel(7, 4), BLACK);
        d.rotate(4500, true).unwrap();
        assert!(d.width() > 8);
    }
    #[test]
    fn selections_copy_paste_and_delete() {
        let mut d = Document::new(6, 6, Some(RED)).unwrap();
        d.select(
            Mask::rect(6, 6, IRect::new(1, 1, 2, 3)),
            SelectMode::Replace,
        );
        let copied = d.copy();
        assert_eq!((copied.width(), copied.height()), (2, 3));
        d.delete_selection().unwrap();
        assert_eq!(d.pixel(1, 1), TRANSPARENT);
        assert_eq!(d.pixel(0, 0), RED);
        d.paste(&copied, 4, 0).unwrap();
        assert_eq!(d.layers().len(), 2);
        assert_eq!(d.layers()[1].name, "Pasted Layer");
        assert_eq!(d.pixel(4, 0), RED);
        assert_eq!(
            d.selection().and_then(Mask::bounds),
            Some(IRect::new(4, 0, 2, 3))
        );
        d.invert_selection();
        assert!(d.selection().unwrap().get(0, 0) == 255);
        d.select_none();
        d.select_all();
        assert_eq!(d.selection().and_then(Mask::bounds), Some(d.bounds()));
        d.select_similar(1, 1, 0, true, SelectMode::Replace);
        // The hole left by Delete shows the transparent pixels only.
        assert_eq!(
            d.selection().and_then(Mask::bounds),
            Some(IRect::new(1, 1, 2, 3))
        );
        assert_eq!(d.pick(0, 0, true), Some(RED));
        assert_eq!(d.pick(9, 9, true), None);
    }
    #[test]
    fn adjustments_filters_and_text_record_their_area() {
        let mut d = Document::new(4, 4, Some([10, 20, 30, 255])).unwrap();
        d.adjust("Invert", &Adjustment::Invert).unwrap();
        assert_eq!(d.pixel(0, 0), [245, 235, 225, 255]);
        d.filter("Blur", &Filter::BoxBlur { radius: 1 });
        let mut alpha = Mask::empty(2, 1);
        alpha.set(0, 0, 255);
        d.stamp(1, 1, &alpha, BLACK).unwrap();
        assert_eq!(d.pixel(1, 1), BLACK);
        assert_eq!(d.undo_steps(), vec!["Invert", "Text"]);
        d.undo().unwrap();
        d.undo().unwrap();
        assert_eq!(d.pixel(0, 0), [10, 20, 30, 255]);
        // An edit that changes nothing is not a step.
        assert!(d
            .adjust("Nothing", &Adjustment::Posterize { levels: 255 })
            .is_none());
        assert!(!d.can_undo());
    }
    #[test]
    fn documents_round_trip_and_hash_deterministically() {
        let mut d = Document::new(32, 24, Some(WHITE)).unwrap();
        d.begin_stroke(
            Brush {
                size: 7,
                hardness: 40,
                color: [30, 90, 200, 255],
                ..Brush::default()
            },
            at(3, 3),
        );
        d.stroke_to(at(28, 20));
        d.end_stroke();
        d.filter("Blur", &Filter::GaussianBlur { radius: 2 });
        let json = serde_json::to_string(&d).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
        // Pinned: any change to rasterisation, blur or rounding moves this hash.
        assert_eq!(format!("{:016x}", d.composite().hash()), "4abb365a4021b651");
        let view = d.view((0, 0), (8, 1), 64, 48, [0, 0, 0, 255]);
        // Two view pixels per image pixel: nearest sampling repeats each one.
        assert_eq!(view.get(0, 0), d.pixel(0, 0));
        assert_eq!(view.get(7, 5), d.pixel(3, 2));
        assert_eq!(view.get(63, 47), d.pixel(31, 23));
        let transparent = Document::new(2, 2, None).unwrap();
        // Zoomed to 1600%: one image pixel fills 16x16 view pixels of checkerboard.
        let v = transparent.view((0, 0), (1, 1), 16, 16, BLACK);
        assert_eq!(v.get(0, 0), [255, 255, 255, 255]);
        assert_eq!(v.get(8, 0), [204, 204, 204, 255]);
        assert_eq!(v.get(15, 15), [255, 255, 255, 255]);
        let outside = transparent.view((-32, 0), (16, 1), 3, 1, BLACK);
        assert_eq!(outside.get(0, 0), BLACK);
    }
}
