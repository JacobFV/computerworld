//! The pieces every editor interface is built from: the canvas view (the image at its
//! zoom, the selection's marching ants, the drag in progress), and a small kit of
//! controls each product styles with its own [`Skin`]. Every control here dispatches a
//! command the [`Studio`] really handles, or is painted disabled with its reason.
use super::{dialog_params, look, preview_adjustments, Gesture, Panel, Product, Studio};
use crate::desktop_scene::{shared::Align, Painter};
use cw_raster::document::checker;
use cw_raster::mask::Mask;
use cw_raster::{blend, BlendMode, Canvas, Rgba};
use cw_scene::{Color, Primitive, Rect};

/// A product's palette and metrics.
#[derive(Clone, Copy, Debug)]
pub struct Skin {
    /// Window background.
    pub bg: Color,
    /// Toolbars and title bars.
    pub bar: Color,
    /// Docks, sidebars and popovers.
    pub panel: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub on_accent: Color,
    /// Fill behind a selected tool or row.
    pub selected: Color,
    pub line: Color,
    /// Around the image.
    pub backdrop: Color,
    pub radius: u32,
    /// Size of body text.
    pub font: u16,
}

pub fn rgba(c: Rgba) -> Color {
    Color(c[0], c[1], c[2], c[3])
}

/// Title a product gives a dialog.
pub fn dialog_title(product: Product, id: &str) -> String {
    let gimp = product == Product::Gimp;
    match id {
        "brightness-contrast" if product == Product::Pinta => "Brightness / Contrast",
        "brightness-contrast" if gimp => "Brightness-Contrast",
        "brightness-contrast" => "Brightness & Contrast",
        "exposure" => "Exposure",
        "levels" => "Levels",
        "curves" => "Curves",
        "hue-saturation" if gimp => "Hue-Saturation",
        "hue-saturation" if product == Product::Pinta => "Hue / Saturation",
        "hue-saturation" => "Hue & Saturation",
        "saturation" => "Saturation",
        "color-balance" => "Color Balance",
        "temperature" if gimp => "Color Temperature",
        "temperature" => "White Balance",
        "shadows-highlights" if gimp => "Shadows-Highlights",
        "shadows-highlights" => "Shadows & Highlights",
        "threshold" => "Threshold",
        "posterize" => "Posterize",
        "gaussian-blur" => "Gaussian Blur",
        "box-blur" => "Box Blur",
        "sharpen" => "Sharpen",
        "unsharp-mask" if gimp => "Sharpen (Unsharp Mask)",
        "unsharp-mask" => "Unsharp Mask",
        "median" if gimp => "Median Blur",
        "median" => "Median",
        "noise-reduction" => "Noise Reduction",
        "pixelate" if gimp => "Pixelize",
        "pixelate" => "Pixelate",
        "vignette" => "Vignette",
        "resize" if product == Product::Paint => "Resize and Skew",
        "resize" if product == Product::Preview => "Image Dimensions",
        "resize" if gimp => "Scale Image",
        "resize" => "Resize Image",
        "rotate" if gimp => "Arbitrary Rotation",
        "rotate" => "Rotate",
        "new-image" if gimp => "Create a New Image",
        "new-image" => "New Image",
        "color" if product == Product::Paint => "Edit colors",
        "color" if gimp => "Change Foreground Color",
        "color" => "Colors",
        "adjust-color" => "Adjust Color",
        other => other,
    }
    .to_owned()
}

/// Label of a dialog parameter.
pub fn param_label(id: &str) -> &str {
    match id {
        "brightness" => "Brightness",
        "contrast" => "Contrast",
        "exposure" => "Exposure",
        "black" => "Input black",
        "white" => "Input white",
        "gamma" => "Gamma (x100)",
        "hue" => "Hue",
        "saturation" => "Saturation",
        "lightness" => "Lightness",
        "cyan-red" => "Cyan – Red",
        "magenta-green" => "Magenta – Green",
        "yellow-blue" => "Yellow – Blue",
        "temperature" => "Temperature",
        "tint" => "Tint",
        "shadows" => "Shadows",
        "highlights" => "Highlights",
        "low" => "Low",
        "high" => "High",
        "levels" => "Levels",
        "radius" => "Radius",
        "amount" => "Amount",
        "threshold" => "Threshold",
        "size" => "Cell size",
        "width" => "Width",
        "height" => "Height",
        "ratio" => "Maintain aspect ratio",
        "angle" => "Angle",
        "expand" => "Enlarge canvas to fit",
        "background" => "White background",
        "sepia" => "Sepia",
        "sharpness" => "Sharpness",
        "r" => "Red",
        "g" => "Green",
        "b" => "Blue",
        other => other,
    }
}

// ----- canvas --------------------------------------------------------------------

/// The image as the view shows it, with anything it overlays. `outside` fills view
/// pixels beyond the image edge (there are none unless the image is scrolled).
fn view_bitmap(studio: &Studio, vw: u32, vh: u32) -> Option<(Rect, Canvas)> {
    let doc = studio.doc.as_ref()?;
    let z = i64::from(studio.effective_zoom(vw, vh).max(1));
    let (ox, oy) = studio.origin(vw, vh);
    let (dw, dh) = (
        i64::from(doc.width()) * z / 100,
        i64::from(doc.height()) * z / 100,
    );
    let x0 = ox.max(0);
    let y0 = oy.max(0);
    let x1 = (i64::from(ox) + dw).min(i64::from(vw)) as i32;
    let y1 = (i64::from(oy) + dh).min(i64::from(vh)) as i32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let map = |v: i32, o: i32| -> i32 { ((2 * i64::from(v - o) + 1) * 100 / (2 * z)) as i32 };
    let cols: Vec<i32> = (x0..x1).map(|vx| map(vx, ox)).collect();
    let rows: Vec<i32> = (y0..y1).map(|vy| map(vy, oy)).collect();
    // Colour first, without the checkerboard, so adjustments preview on the image.
    let mut img = Canvas::new(w, h);
    for (j, iy) in rows.iter().enumerate() {
        for (i, ix) in cols.iter().enumerate() {
            img.set(i as i32, j as i32, doc.pixel(*ix, *iy));
        }
    }
    for adj in preview_adjustments(studio.panel.as_ref()) {
        cw_raster::adjust::apply(&mut img, &adj, None);
    }
    if matches!(studio.product, Product::IosPhotos | Product::GooglePhotos)
        && !look::is_identity(studio)
    {
        img = look::apply(studio, &img);
    }
    let sel = doc.selection();
    let edge = |m: &Mask, x: i32, y: i32| {
        m.get(x, y) >= 128
            && [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                .iter()
                .any(|(a, b)| m.get(*a, *b) < 128)
    };
    for (j, iy) in rows.iter().enumerate() {
        for (i, ix) in cols.iter().enumerate() {
            let (vx, vy) = (x0 + i as i32, y0 + j as i32);
            let mut px = img.get(i as i32, j as i32);
            if px[3] < 255 {
                px = blend::composite(checker(vx, vy), px, 255, BlendMode::Normal);
            }
            if let Some(crop) = studio.crop {
                if !crop.contains(*ix, *iy) {
                    px = blend::mix(px, [0, 0, 0, 255], 140);
                }
            }
            if sel.is_some_and(|m| edge(m, *ix, *iy)) {
                // Marching ants: black and white dashes along the edge.
                px = if ((vx + vy) / 4) % 2 == 0 {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
            }
            img.set(i as i32, j as i32, px);
        }
    }
    Some((Rect::new(x0, y0, w, h), img))
}

/// View position of an image point (sub16).
fn to_view(studio: &Studio, vw: u32, vh: u32, p: (i64, i64)) -> (i32, i32) {
    let z = i64::from(studio.effective_zoom(vw, vh).max(1));
    let (ox, oy) = studio.origin(vw, vh);
    (ox + (p.0 * z / 1600) as i32, oy + (p.1 * z / 1600) as i32)
}

/// The editing surface: the image, what overlays it, and the drag target that turns
/// pointer drags into strokes, selections and shapes. `page` draws the paper shadow
/// Paint and Pinta put under the image.
pub fn canvas(p: &mut Painter, s: &Skin, r: Rect, studio: &Studio, page: bool) {
    canvas_with(p, s, r, studio, page, true);
}
/// The canvas; `interactive` is false where the image is only shown (a phone editor's
/// Adjust and Filters tabs), so no drag surface is painted over it.
pub fn canvas_with(
    p: &mut Painter,
    s: &Skin,
    r: Rect,
    studio: &Studio,
    page: bool,
    interactive: bool,
) {
    p.box_(r, s.backdrop, 0);
    let (vw, vh) = (r.width.max(1), r.height.max(1));
    let target = studio.target(&format!("canvas:{vw}:{vh}"));
    if let Some((at, img)) = view_bitmap(studio, vw, vh) {
        let at = Rect::new(r.x + at.x, r.y + at.y, at.width, at.height);
        if page {
            p.drop_shadow(at, 0, 6, 40, 1);
        }
        p.node(
            at,
            Primitive::Image {
                width: img.width(),
                height: img.height(),
                rgba: img.into_pixels(),
            },
            None,
        );
    }
    overlays(p, r, studio);
    if !interactive {
        return;
    }
    let label = match studio.tool {
        super::Tool::Text => "Image canvas (click to type)",
        _ => "Image canvas",
    };
    p.region(r, &target, label);
}

fn overlays(p: &mut Painter, r: Rect, studio: &Studio) {
    let (vw, vh) = (r.width.max(1), r.height.max(1));
    let at = |pt: (i64, i64)| {
        let (x, y) = to_view(studio, vw, vh, pt);
        (r.x + x, r.y + y)
    };
    let dash = Color(20, 20, 20, 220);
    match &studio.gesture {
        Some(Gesture::Span { tool, start, end }) => {
            let (a, b) = (at(*start), at(*end));
            let (x0, y0, x1, y1) = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
            let color = if *tool == super::Tool::Shape {
                rgba(studio.primary)
            } else {
                dash
            };
            if *tool == super::Tool::Shape && studio.shape.open() {
                p.line(vec![a, b], color, studio.size.clamp(1, 40) as u16);
            } else if *tool == super::Tool::EllipseSelect
                || (*tool == super::Tool::Shape
                    && studio.shape == cw_raster::draw::ShapeKind::Ellipse)
            {
                let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);
                let (rx, ry) = (((x1 - x0) / 2).max(1), ((y1 - y0) / 2).max(1));
                let pts: Vec<(i32, i32)> = (0..=48)
                    .map(|i| {
                        let a = i * 360 / 48;
                        (
                            cx + rx * crate::desktop_scene::shared::sin1024(a) / 1024,
                            cy - ry * crate::desktop_scene::shared::cos1024(a) / 1024,
                        )
                    })
                    .collect();
                p.line(pts, color, 1);
            } else {
                p.line(
                    vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)],
                    color,
                    1,
                );
            }
        }
        Some(Gesture::Lasso { points }) => {
            let pts: Vec<(i32, i32)> = points.iter().map(|q| at(*q)).collect();
            if pts.len() > 1 {
                p.line(pts, dash, 1);
            }
        }
        _ => {}
    }
    if let Some(crop) = studio.crop {
        let a = at(((crop.x as i64) * 16, (crop.y as i64) * 16));
        let b = at(((crop.right() as i64) * 16, (crop.bottom() as i64) * 16));
        p.line(
            vec![(a.0, a.1), (b.0, a.1), (b.0, b.1), (a.0, b.1), (a.0, a.1)],
            Color::WHITE,
            2,
        );
    }
    if let Some(t) = &studio.text {
        let z = studio.effective_zoom(vw, vh);
        let size = (u32::from(studio.font_size) * z / 100).clamp(6, 200) as u16;
        let (x, y) = at(((t.x as i64) * 16, (t.y as i64) * 16));
        let width = p.measure(&t.text, size, studio.bold).max(u32::from(size));
        let h = u32::from(size) + u32::from(size) / 2 + 4;
        p.line(
            vec![
                (x - 2, y - 2),
                (x + width as i32 + 4, y - 2),
                (x + width as i32 + 4, y + h as i32),
                (x - 2, y + h as i32),
                (x - 2, y - 2),
            ],
            dash,
            1,
        );
        if !t.text.is_empty() {
            p.node(
                Rect::new(x, y, width + 2, h),
                Primitive::ui_text(
                    t.text.clone(),
                    rgba(studio.primary),
                    size,
                    studio.bold.into(),
                ),
                None,
            );
        }
        if !t.pending {
            let caret = p.measure(&t.text, size, studio.bold) as i32;
            p.box_(
                Rect::new(x + caret + 1, y, 1, h - 2),
                rgba(studio.primary),
                0,
            );
        }
    }
}

/// Scroll bars for an image larger than its view, each a drag surface.
pub fn scrollbars(p: &mut Painter, s: &Skin, r: Rect, studio: &Studio) {
    let Some(doc) = &studio.doc else {
        return;
    };
    let (vw, vh) = (r.width.max(1), r.height.max(1));
    let z = studio.effective_zoom(vw, vh).max(1);
    let (dw, dh) = (doc.width() * z / 100, doc.height() * z / 100);
    if dw > vw {
        let track = Rect::new(r.x, r.y + r.height as i32 - 10, r.width, 10);
        p.box_(track, Color(0, 0, 0, 20), 0);
        let len = (u64::from(vw) * u64::from(vw) / u64::from(dw)).max(24) as u32;
        let pos = (i64::from(studio.scroll.0) * i64::from(z) / 100 * i64::from(vw) / i64::from(dw))
            as i32;
        p.box_(
            Rect::new(track.x + pos, track.y + 2, len, 6),
            Color(s.muted.0, s.muted.1, s.muted.2, 180),
            3,
        );
        p.region(
            track,
            &studio.target(&format!("hscroll:{}:{vw}:{vh}", track.width)),
            "Horizontal scroll bar",
        );
    }
    if dh > vh {
        let track = Rect::new(r.x + r.width as i32 - 10, r.y, 10, r.height);
        p.box_(track, Color(0, 0, 0, 20), 0);
        let len = (u64::from(vh) * u64::from(vh) / u64::from(dh)).max(24) as u32;
        let pos = (i64::from(studio.scroll.1) * i64::from(z) / 100 * i64::from(vh) / i64::from(dh))
            as i32;
        p.box_(
            Rect::new(track.x + 2, track.y + pos, 6, len),
            Color(s.muted.0, s.muted.1, s.muted.2, 180),
            3,
        );
        p.region(
            track,
            &studio.target(&format!("vscroll:{}:{vw}:{vh}", track.height)),
            "Vertical scroll bar",
        );
    }
}

// ----- controls ------------------------------------------------------------------

/// Square tool button with a glyph. `active` marks the current tool.
#[allow(clippy::too_many_arguments)]
pub fn tool(
    p: &mut Painter,
    s: &Skin,
    r: Rect,
    symbol: &str,
    label: &str,
    target: &str,
    active: bool,
) {
    p.button(
        r,
        if active {
            s.selected
        } else {
            Color::TRANSPARENT
        },
        s.radius,
        target,
        label,
    );
    let size = (r.width.min(r.height) * 3 / 5).clamp(10, 28);
    p.symbol(
        symbol,
        r.x + (r.width as i32 - size as i32) / 2,
        r.y + (r.height as i32 - size as i32) / 2,
        size,
        if active { s.accent } else { s.text },
    );
}
/// A tool button with a caption under its glyph (Paint's big buttons).
pub fn captioned(
    p: &mut Painter,
    s: &Skin,
    r: Rect,
    symbol: &str,
    label: &str,
    target: &str,
    active: bool,
) {
    p.button(
        r,
        if active {
            s.selected
        } else {
            Color::TRANSPARENT
        },
        s.radius,
        target,
        label,
    );
    p.symbol(symbol, r.x + (r.width as i32 - 20) / 2, r.y + 6, 20, s.text);
    p.label(
        r.x,
        r.y + r.height as i32 - 18,
        r.width,
        label,
        11,
        s.text,
        false,
        Align::Center,
    );
}
/// Text button. `primary` fills it with the accent.
pub fn button(p: &mut Painter, s: &Skin, r: Rect, label: &str, target: &str, primary: bool) {
    p.button(
        r,
        if primary { s.accent } else { s.panel },
        s.radius,
        target,
        label,
    );
    if !primary {
        p.border(r, Color::TRANSPARENT, s.radius, s.line);
    }
    p.label(
        r.x,
        r.y + (r.height as i32 - s.font as i32 - 5) / 2,
        r.width,
        label,
        s.font,
        if primary { s.on_accent } else { s.text },
        primary,
        Align::Center,
    );
}
/// A control this state cannot honour, announced disabled with the reason.
pub fn inert(p: &mut Painter, s: &Skin, r: Rect, label: &str, why: &str) {
    p.border(r, Color::TRANSPARENT, s.radius, s.line);
    p.disabled(why);
    p.label(
        r.x,
        r.y + (r.height as i32 - s.font as i32 - 5) / 2,
        r.width,
        label,
        s.font,
        s.muted,
        false,
        Align::Center,
    );
}
/// An icon button that cannot act now.
pub fn inert_tool(p: &mut Painter, s: &Skin, r: Rect, symbol: &str, why: &str) {
    p.box_(r, Color::TRANSPARENT, s.radius);
    p.disabled(why);
    let size = (r.width.min(r.height) * 3 / 5).clamp(10, 28);
    p.symbol(
        symbol,
        r.x + (r.width as i32 - size as i32) / 2,
        r.y + (r.height as i32 - size as i32) / 2,
        size,
        Color(s.muted.0, s.muted.1, s.muted.2, 110),
    );
}
/// Toggle chip.
pub fn chip(p: &mut Painter, s: &Skin, r: Rect, label: &str, target: &str, on: bool) {
    p.button(
        r,
        if on { s.selected } else { Color::TRANSPARENT },
        s.radius,
        target,
        label,
    );
    if !on {
        p.border(r, Color::TRANSPARENT, s.radius, s.line);
    }
    p.label(
        r.x,
        r.y + (r.height as i32 - s.font as i32 - 5) / 2,
        r.width,
        label,
        s.font.saturating_sub(1).max(9),
        if on { s.accent } else { s.text },
        on,
        Align::Center,
    );
}
/// A colour well. `round` for the platforms that draw circles.
pub fn swatch(
    p: &mut Painter,
    s: &Skin,
    r: Rect,
    color: Rgba,
    target: &str,
    selected: bool,
    round: bool,
) {
    let radius = if round { r.width.min(r.height) / 2 } else { 2 };
    if selected {
        let ring = Rect::new(r.x - 2, r.y - 2, r.width + 4, r.height + 4);
        p.border(ring, Color::TRANSPARENT, radius + 2, s.accent);
    }
    p.button(
        r,
        rgba(color),
        radius,
        target,
        &format!("#{}", cw_raster::hex(color)),
    );
    p.border(r, Color::TRANSPARENT, radius, s.line);
}
/// Labelled slider bound to a parameter. The track is a drag surface: pressing or
/// dragging anywhere on it sets the value there.
#[allow(clippy::too_many_arguments)]
pub fn slider(
    p: &mut Painter,
    s: &Skin,
    r: Rect,
    studio: &Studio,
    label: &str,
    param: &str,
    value_text: bool,
) {
    let Some((lo, hi)) = studio.param_range(param) else {
        return;
    };
    // Zoom "fit" is stored as 0; the slider shows the zoom really on screen.
    let value = match (param, studio.param(param)) {
        ("zoom", 0) => 100,
        (_, v) => v,
    }
    .clamp(lo, hi);
    let label_h = if label.is_empty() { 0 } else { 16 };
    if !label.is_empty() {
        p.label(
            r.x,
            r.y,
            r.width.saturating_sub(48),
            label,
            s.font.saturating_sub(1).max(9),
            s.text,
            false,
            Align::Left,
        );
        if value_text {
            p.label(
                r.x + r.width as i32 - 48,
                r.y,
                48,
                &value.to_string(),
                s.font.saturating_sub(1).max(9),
                s.muted,
                false,
                Align::Right,
            );
        }
    }
    let track = Rect::new(
        r.x,
        r.y + label_h,
        r.width,
        r.height.saturating_sub(label_h as u32).max(12),
    );
    let mid = track.y + track.height as i32 / 2;
    p.box_(
        Rect::new(track.x, mid - 2, track.width, 4),
        Color(s.line.0, s.line.1, s.line.2, 90),
        2,
    );
    let span = (hi - lo).max(1);
    let fill = ((i64::from(value - lo) * i64::from(track.width)) / i64::from(span)) as u32;
    p.box_(
        Rect::new(track.x, mid - 2, fill.min(track.width), 4),
        s.accent,
        2,
    );
    let knob = track.x + fill.min(track.width) as i32;
    p.circle(knob, mid, 7, Color::WHITE);
    p.ring(knob, mid, 7, 1, s.line);
    p.region(
        track,
        &studio.target(&format!("slider:{param}:{}", track.width)),
        &format!("{label} slider"),
    );
}

/// One entry of a menu.
pub struct Item<'a> {
    pub label: &'a str,
    /// Command it runs, or `None` for a separator.
    pub command: Option<String>,
    /// Why it is disabled; empty when it is live.
    pub why: &'a str,
    pub shortcut: &'a str,
    pub checked: bool,
}
impl<'a> Item<'a> {
    pub fn new(label: &'a str, command: &str) -> Self {
        Self {
            label,
            command: Some(command.to_owned()),
            why: "",
            shortcut: "",
            checked: false,
        }
    }
    pub fn key(mut self, shortcut: &'a str) -> Self {
        self.shortcut = shortcut;
        self
    }
    pub fn when(mut self, live: bool, why: &'a str) -> Self {
        if !live {
            self.why = why;
        }
        self
    }
    pub fn checked(mut self, on: bool) -> Self {
        self.checked = on;
        self
    }
    pub fn separator() -> Self {
        Self {
            label: "",
            command: None,
            why: "",
            shortcut: "",
            checked: false,
        }
    }
}
/// A drop-down menu at `(x, y)`, above everything else.
pub fn menu(
    p: &mut Painter,
    s: &Skin,
    studio: &Studio,
    x: i32,
    y: i32,
    width: u32,
    items: &[Item<'_>],
) {
    let row = 26;
    let height: u32 = items
        .iter()
        .map(|i| if i.command.is_none() { 9 } else { row })
        .sum::<u32>()
        + 8;
    let z = p.z;
    p.z += 500;
    let r = Rect::new(x, y, width, height);
    p.drop_shadow(r, s.radius, 10, 60, 3);
    p.border(r, s.panel, s.radius, s.line);
    // The menu's own body absorbs clicks between rows.
    p.region(r, &studio.target("noop"), "Menu");
    let mut cy = y + 4;
    for item in items {
        let Some(command) = &item.command else {
            p.hline(x + 8, cy + 4, width - 16, s.line);
            cy += 9;
            continue;
        };
        let rr = Rect::new(x + 4, cy, width - 8, row);
        let live = item.why.is_empty();
        if live {
            p.button(
                rr,
                Color::TRANSPARENT,
                s.radius.min(4),
                &studio.target(command),
                item.label,
            );
        } else {
            p.box_(rr, Color::TRANSPARENT, 0);
            p.disabled(item.why);
        }
        let color = if live { s.text } else { s.muted };
        if item.checked {
            p.symbol("check", rr.x + 4, rr.y + 6, 14, color);
        }
        p.label(
            rr.x + 24,
            rr.y + 5,
            width - 120,
            item.label,
            s.font,
            color,
            false,
            Align::Left,
        );
        if !item.shortcut.is_empty() {
            p.label(
                rr.x + width as i32 - 104,
                rr.y + 5,
                90,
                item.shortcut,
                s.font.saturating_sub(1),
                s.muted,
                false,
                Align::Right,
            );
        }
        cy += row as i32;
    }
    p.z = z;
}

/// The open dialog, if one is: a panel of the dialog's parameters with Reset, Cancel
/// and OK. Adjustment dialogs preview on the canvas while they are open.
pub fn dialog(p: &mut Painter, s: &Skin, studio: &Studio, width: u32, height: u32) {
    let Some(Panel::Dialog { id, values }) = &studio.panel else {
        return;
    };
    let params = dialog_params(id);
    let row = 44;
    let extra = if id == "curves" { 170 } else { 0 };
    let w = 380.min(width.saturating_sub(24)).max(240);
    let h = (56 + params.len() as u32 * row + extra + 56).min(height.saturating_sub(16));
    let r = Rect::new(
        (width as i32 - w as i32) / 2,
        (height as i32 - h as i32) / 2,
        w,
        h,
    );
    let z = p.z;
    p.z += 400;
    // Dim what is behind, and take its clicks: a dialog is modal.
    p.box_(Rect::new(0, 0, width, height), Color(0, 0, 0, 40), 0);
    p.region(
        Rect::new(0, 0, width, height),
        &studio.target("noop"),
        "Dialog backdrop",
    );
    p.drop_shadow(r, s.radius, 16, 70, 4);
    p.border(r, s.panel, s.radius + 2, s.line);
    p.strong(
        r.x + 16,
        r.y + 14,
        w - 32,
        &dialog_title(studio.product, id),
        s.font + 1,
        s.text,
    );
    let mut y = r.y + 44;
    if id == "curves" {
        let g = Rect::new(r.x + 16, y, w - 32, 160);
        p.box_(g, Color(0, 0, 0, 16), 2);
        for i in 1..4 {
            p.hline(g.x, g.y + (g.height * i / 4) as i32, g.width, s.line);
            p.vline(g.x + (g.width * i / 4) as i32, g.y, g.height, s.line);
        }
        let pts: Vec<(u8, u8)> = (0..5)
            .map(|i| {
                (
                    (i * 64).min(255) as u8,
                    values
                        .get(&format!("c{i}"))
                        .copied()
                        .unwrap_or(i * 64)
                        .clamp(0, 255) as u8,
                )
            })
            .collect();
        let table = cw_raster::adjust::curve_table(&pts);
        let line: Vec<(i32, i32)> = (0..=32)
            .map(|k| {
                let xin = (k * 255 / 32) as usize;
                (
                    g.x + (xin as u32 * (g.width - 1) / 255) as i32,
                    g.y + ((255 - u32::from(table[xin])) * (g.height - 1) / 255) as i32,
                )
            })
            .collect();
        p.line(line, s.text, 2);
        for (x, v) in &pts {
            let cx = g.x + (u32::from(*x) * (g.width - 1) / 255) as i32;
            let cy = g.y + ((255 - u32::from(*v)) * (g.height - 1) / 255) as i32;
            p.circle(cx, cy, 4, s.accent);
        }
        p.region(
            g,
            &studio.target(&format!("curve:{}:{}", g.width, g.height)),
            "Curve",
        );
        y += 170;
    }
    for (param, lo, hi, _) in params {
        if id == "curves" || *param == "slot" {
            continue;
        }
        let value = values.get(*param).copied().unwrap_or(0);
        let rr = Rect::new(r.x + 16, y, w - 32, 36);
        match (*param, *lo, *hi) {
            ("range", _, _) => {
                p.label(
                    rr.x,
                    rr.y,
                    80,
                    "Range",
                    s.font - 1,
                    s.text,
                    false,
                    Align::Left,
                );
                for (i, name) in ["Shadows", "Midtones", "Highlights"].iter().enumerate() {
                    chip(
                        p,
                        s,
                        Rect::new(rr.x + 70 + i as i32 * 92, rr.y + 12, 86, 22),
                        name,
                        &studio.target(&format!("set:range:{i}")),
                        value == i as i32,
                    );
                }
            }
            ("resample", _, _) => {
                p.label(
                    rr.x,
                    rr.y,
                    80,
                    "Resample",
                    s.font - 1,
                    s.text,
                    false,
                    Align::Left,
                );
                for (i, name) in ["Nearest", "Bilinear", "Bicubic"].iter().enumerate() {
                    chip(
                        p,
                        s,
                        Rect::new(rr.x + 76 + i as i32 * 88, rr.y + 12, 82, 22),
                        name,
                        &studio.target(&format!("set:resample:{i}")),
                        value == i as i32,
                    );
                }
            }
            (_, 0, 1) => {
                let target = studio.target(&format!("set:{param}:{}", 1 - value));
                p.button(
                    Rect::new(rr.x, rr.y + 8, 18, 18),
                    if value == 1 { s.accent } else { s.panel },
                    3,
                    &target,
                    param_label(param),
                );
                p.border(
                    Rect::new(rr.x, rr.y + 8, 18, 18),
                    Color::TRANSPARENT,
                    3,
                    s.line,
                );
                if value == 1 {
                    p.symbol("check", rr.x + 2, rr.y + 10, 14, s.on_accent);
                }
                p.label(
                    rr.x + 26,
                    rr.y + 9,
                    rr.width - 26,
                    param_label(param),
                    s.font,
                    s.text,
                    false,
                    Align::Left,
                );
            }
            ("width" | "height", _, _) => {
                p.label(
                    rr.x,
                    rr.y + 8,
                    70,
                    param_label(param),
                    s.font,
                    s.text,
                    false,
                    Align::Left,
                );
                let field = Rect::new(rr.x + 70, rr.y + 4, 64, 26);
                p.border(field, s.bg, 3, s.line);
                p.label(
                    field.x + 6,
                    field.y + 5,
                    54,
                    &value.to_string(),
                    s.font,
                    s.text,
                    false,
                    Align::Left,
                );
                let mut bx = field.x + 70;
                for (label, next) in [
                    ("−10", value - 10),
                    ("−1", value - 1),
                    ("+1", value + 1),
                    ("+10", value + 10),
                    ("½", value / 2),
                    ("2×", value * 2),
                ] {
                    let next = next.clamp(*lo, *hi);
                    button(
                        p,
                        s,
                        Rect::new(bx, rr.y + 4, 30, 26),
                        label,
                        &studio.target(&format!("set:{param}:{next}")),
                        false,
                    );
                    bx += 32;
                }
            }
            _ => {
                slider(p, s, rr, studio, param_label(param), param, true);
            }
        }
        y += row as i32;
    }
    if id == "color" {
        let c = [
            values.get("r").copied().unwrap_or(0) as u8,
            values.get("g").copied().unwrap_or(0) as u8,
            values.get("b").copied().unwrap_or(0) as u8,
            255,
        ];
        p.box_(Rect::new(r.x + 16, r.y + h as i32 - 44, 40, 28), rgba(c), 4);
        p.label(
            r.x + 62,
            r.y + h as i32 - 38,
            90,
            &format!("#{}", cw_raster::hex(c)),
            s.font,
            s.text,
            false,
            Align::Left,
        );
    }
    let by = r.y + h as i32 - 44;
    button(
        p,
        s,
        Rect::new(r.x + w as i32 - 180, by, 78, 28),
        "Cancel",
        &studio.target("cancel"),
        false,
    );
    button(
        p,
        s,
        Rect::new(r.x + w as i32 - 94, by, 78, 28),
        "OK",
        &studio.target("apply"),
        true,
    );
    if id != "color" && id != "new-image" {
        button(
            p,
            s,
            Rect::new(r.x + 16, by, 70, 28),
            "Reset",
            &studio.target("reset"),
            false,
        );
    }
    p.z = z;
}

/// The Open or Save sheet over the editor.
pub fn chooser(p: &mut Painter, s: &Skin, studio: &Studio, width: u32, height: u32) {
    let (title, folder, entries, name, loading) = match &studio.panel {
        Some(Panel::Open {
            folder,
            entries,
            loading,
        }) => ("Open", folder, entries, None, *loading),
        Some(Panel::Save {
            folder,
            entries,
            name,
        }) => ("Save As", folder, entries, Some(name), false),
        _ => return,
    };
    let w = 520.min(width.saturating_sub(24)).max(260);
    let h = 420.min(height.saturating_sub(24)).max(200);
    let r = Rect::new(
        (width as i32 - w as i32) / 2,
        (height as i32 - h as i32) / 2,
        w,
        h,
    );
    let z = p.z;
    p.z += 400;
    p.box_(Rect::new(0, 0, width, height), Color(0, 0, 0, 40), 0);
    p.region(
        Rect::new(0, 0, width, height),
        &studio.target("noop"),
        "Sheet backdrop",
    );
    p.drop_shadow(r, s.radius, 16, 70, 4);
    p.border(r, s.panel, s.radius + 2, s.line);
    p.strong(r.x + 16, r.y + 12, w - 32, title, s.font + 1, s.text);
    let up = Rect::new(r.x + 16, r.y + 40, 28, 26);
    if super::folder_of(folder.trim_end_matches('/')).is_empty() || folder == "/" {
        inert_tool(p, s, up, "arrow-up", "There is no folder above this one");
    } else {
        tool(
            p,
            s,
            up,
            "arrow-up",
            "Enclosing folder",
            &studio.target("folder-up"),
            false,
        );
    }
    p.label(
        r.x + 52,
        r.y + 45,
        w - 68,
        folder,
        s.font,
        s.muted,
        false,
        Align::Left,
    );
    let list = Rect::new(
        r.x + 16,
        r.y + 74,
        w - 32,
        h - 74 - if name.is_some() { 96 } else { 52 },
    );
    p.border(list, s.bg, 4, s.line);
    let mut y = list.y + 4;
    if loading {
        p.label(
            list.x,
            y + 8,
            list.width,
            "Loading…",
            s.font,
            s.muted,
            false,
            Align::Center,
        );
    } else if entries.is_empty() {
        p.label(
            list.x,
            y + 8,
            list.width,
            "No images in this folder",
            s.font,
            s.muted,
            false,
            Align::Center,
        );
    }
    for entry in entries {
        if y + 26 > list.y + list.height as i32 {
            break;
        }
        let row = Rect::new(list.x + 4, y, list.width - 8, 26);
        let folder_entry = entry.ends_with('/');
        let clean = entry.trim_end_matches('/');
        let command = if folder_entry {
            Some(format!("folder:{clean}"))
        } else if name.is_none() {
            Some(format!("open:{clean}"))
        } else {
            None
        };
        if let Some(command) = &command {
            p.button(row, Color::TRANSPARENT, 3, &studio.target(command), clean);
        }
        p.symbol(
            if folder_entry { "folder" } else { "image" },
            row.x + 6,
            row.y + 5,
            16,
            if command.is_some() { s.accent } else { s.muted },
        );
        p.label(
            row.x + 28,
            row.y + 5,
            row.width - 32,
            clean,
            s.font,
            if command.is_some() { s.text } else { s.muted },
            false,
            Align::Left,
        );
        y += 26;
    }
    if let Some(name) = name {
        p.label(
            r.x + 16,
            r.y + h as i32 - 86,
            80,
            "Save as:",
            s.font,
            s.text,
            false,
            Align::Left,
        );
        let field = Rect::new(r.x + 90, r.y + h as i32 - 92, w - 106, 28);
        p.border(field, s.bg, 4, s.accent);
        p.label(
            field.x + 8,
            field.y + 6,
            field.width - 16,
            name,
            s.font,
            s.text,
            false,
            Align::Left,
        );
        let caret = p.measure(name, s.font, false).min(field.width - 16) as i32;
        p.box_(
            Rect::new(field.x + 9 + caret, field.y + 6, 1, 16),
            s.text,
            0,
        );
        p.label(
            r.x + 16,
            r.y + h as i32 - 56,
            200,
            "Format: PNG",
            s.font - 1,
            s.muted,
            false,
            Align::Left,
        );
    }
    let by = r.y + h as i32 - 42;
    button(
        p,
        s,
        Rect::new(r.x + w as i32 - 188, by, 82, 28),
        "Cancel",
        &studio.target("cancel"),
        false,
    );
    if name.is_some() {
        button(
            p,
            s,
            Rect::new(r.x + w as i32 - 98, by, 82, 28),
            "Save",
            &studio.target("save-confirm"),
            true,
        );
    } else {
        inert(
            p,
            s,
            Rect::new(r.x + w as i32 - 98, by, 82, 28),
            "Open",
            "choose a file in the list",
        );
    }
    p.z = z;
}

/// A small nearest-sampled thumbnail of a canvas.
pub fn thumbnail(p: &mut Painter, r: Rect, c: &Canvas) {
    let (w, h) = (c.width(), c.height());
    let k = w.max(h).div_ceil(r.width.max(r.height).max(1)).max(1);
    let (tw, th) = ((w / k).max(1), (h / k).max(1));
    let mut small = Canvas::new(tw, th);
    for y in 0..th {
        for x in 0..tw {
            let px = c.get((x * k) as i32, (y * k) as i32);
            let px = blend::composite(
                checker(x as i32 * 2, y as i32 * 2),
                px,
                255,
                BlendMode::Normal,
            );
            small.set(x as i32, y as i32, px);
        }
    }
    p.node(
        Rect::new(
            r.x + (r.width - tw.min(r.width)) as i32 / 2,
            r.y + (r.height - th.min(r.height)) as i32 / 2,
            tw.min(r.width),
            th.min(r.height),
        ),
        Primitive::Image {
            width: tw,
            height: th,
            rgba: small.into_pixels(),
        },
        None,
    );
}

/// A layer list, top layer first: visibility, thumbnail, name; the active one marked.
pub fn layer_rows(p: &mut Painter, s: &Skin, r: Rect, studio: &Studio, row: u32) {
    let Some(doc) = &studio.doc else {
        return;
    };
    let mut y = r.y;
    for (i, layer) in doc.layers().iter().enumerate().rev() {
        if y + row as i32 > r.y + r.height as i32 {
            break;
        }
        let rr = Rect::new(r.x, y, r.width, row - 2);
        let active = i == doc.active();
        p.button(
            rr,
            if active {
                s.selected
            } else {
                Color::TRANSPARENT
            },
            s.radius,
            &studio.target(&format!("layer:select:{i}")),
            &layer.name,
        );
        let eye = Rect::new(rr.x + 4, rr.y + (rr.height as i32 - 22) / 2, 22, 22);
        p.region_above(
            eye,
            &studio.target(&format!("layer:toggle:{i}")),
            if layer.visible {
                "Hide layer"
            } else {
                "Show layer"
            },
        );
        p.symbol(
            if layer.visible { "eye" } else { "eye-off" },
            eye.x + 3,
            eye.y + 3,
            16,
            s.muted,
        );
        let th = Rect::new(
            rr.x + 30,
            rr.y + 3,
            rr.height.saturating_sub(6).max(8),
            rr.height.saturating_sub(6).max(8),
        );
        p.border(th, Color::WHITE, 2, s.line);
        thumbnail(p, th, &layer.canvas);
        p.label(
            th.x + th.width as i32 + 8,
            rr.y + (rr.height as i32 - 16) / 2,
            rr.width.saturating_sub(th.width + 50),
            &layer.name,
            s.font,
            if active { s.accent } else { s.text },
            active,
            Align::Left,
        );
        y += row as i32;
    }
}

/// A status message, when there is one.
pub fn status(p: &mut Painter, s: &Skin, x: i32, y: i32, w: u32, studio: &Studio) {
    if let Some(msg) = &studio.status {
        p.label(
            x,
            y,
            w,
            msg,
            s.font.saturating_sub(1).max(9),
            s.muted,
            false,
            Align::Left,
        );
    }
}

/// Where a view of the canvas's point `(vx, vy)` lands in image pixels, for a status
/// bar readout of the selection or image size.
pub fn size_text(studio: &Studio) -> String {
    studio
        .doc
        .as_ref()
        .map(|d| format!("{} × {}px", d.width(), d.height()))
        .unwrap_or_default()
}
/// Selection size, when there is one.
pub fn selection_text(studio: &Studio) -> Option<String> {
    let r = studio.doc.as_ref()?.selection().and_then(Mask::bounds)?;
    Some(format!("{} × {}px", r.w, r.h))
}
