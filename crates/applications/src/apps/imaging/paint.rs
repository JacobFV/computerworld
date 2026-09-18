//! Windows 11 Paint: File / Edit / View with undo and redo, the toolbar of Selection,
//! Image, Tools, Brushes, Shapes, Size and Colors groups with their captions, the Layers
//! pane, the page on its blue-grey backdrop and the status bar with canvas size and zoom.
use super::view::{self, Item, Skin};
use super::{Studio, Tool};
use crate::desktop_scene::{shared::Align, Painter};
use cw_raster::draw::{Shape, ShapeKind};
use cw_scene::{Color, Primitive, Rect};

fn skin(dark: bool) -> Skin {
    if dark {
        Skin {
            bg: Color::rgb(32, 32, 32),
            bar: Color::rgb(43, 43, 43),
            panel: Color::rgb(44, 44, 44),
            text: Color::rgb(255, 255, 255),
            muted: Color::rgb(170, 170, 170),
            accent: Color::rgb(96, 205, 255),
            on_accent: Color::rgb(0, 0, 0),
            selected: Color::rgb(61, 61, 61),
            line: Color(255, 255, 255, 30),
            backdrop: Color::rgb(28, 28, 28),
            radius: 4,
            font: 12,
        }
    } else {
        Skin {
            bg: Color::rgb(255, 255, 255),
            bar: Color::rgb(249, 249, 249),
            panel: Color::rgb(252, 252, 252),
            text: Color::rgb(26, 26, 26),
            muted: Color::rgb(96, 96, 96),
            accent: Color::rgb(0, 95, 184),
            on_accent: Color::WHITE,
            selected: Color::rgb(229, 229, 229),
            line: Color(0, 0, 0, 24),
            backdrop: Color::rgb(233, 236, 241),
            radius: 4,
            font: 12,
        }
    }
}

/// Paint's palette: the two classic rows.
pub const PALETTE: [[u8; 3]; 20] = [
    [0, 0, 0],
    [127, 127, 127],
    [136, 0, 21],
    [237, 28, 36],
    [255, 127, 39],
    [255, 242, 0],
    [34, 177, 76],
    [0, 162, 232],
    [63, 72, 204],
    [163, 73, 164],
    [255, 255, 255],
    [195, 195, 195],
    [185, 122, 87],
    [255, 174, 201],
    [255, 201, 14],
    [239, 228, 176],
    [181, 230, 29],
    [153, 217, 234],
    [112, 146, 190],
    [200, 191, 231],
];
/// The shapes gallery, in Paint's order.
const SHAPES: [ShapeKind; 16] = [
    ShapeKind::Line,
    ShapeKind::Curve,
    ShapeKind::Ellipse,
    ShapeKind::Rectangle,
    ShapeKind::RoundedRectangle,
    ShapeKind::Polygon,
    ShapeKind::Triangle,
    ShapeKind::RightTriangle,
    ShapeKind::Diamond,
    ShapeKind::Pentagon,
    ShapeKind::Hexagon,
    ShapeKind::RightArrow,
    ShapeKind::LeftArrow,
    ShapeKind::UpArrow,
    ShapeKind::DownArrow,
    ShapeKind::Star,
];

/// A small outline drawing of a shape, for a gallery cell.
pub fn shape_glyph(p: &mut Painter, r: Rect, kind: ShapeKind, color: Color) {
    let (x0, y0) = (r.x + 3, r.y + 3);
    let (w, h) = (r.width as i32 - 6, r.height as i32 - 6);
    let pts: Vec<(i32, i32)> = match kind {
        ShapeKind::Line => vec![(x0, y0 + h), (x0 + w, y0)],
        ShapeKind::Arrow => vec![(x0, y0 + h), (x0 + w, y0)],
        // An S-bend, flattened in sub-pixel units as the tool flattens its curve.
        ShapeKind::Curve => {
            let (w, h) = (i64::from(w) * 16, i64::from(h) * 16);
            cw_raster::path::cubic((0, h), (w / 3, -h), (w * 2 / 3, 2 * h), (w, 0))
                .iter()
                .map(|(x, y)| (x0 + (*x / 16) as i32, y0 + (*y / 16) as i32))
                .collect()
        }
        ShapeKind::Ellipse => (0..=24)
            .map(|i| {
                let a = i * 15;
                (
                    x0 + w / 2 + w / 2 * crate::desktop_scene::shared::sin1024(a) / 1024,
                    y0 + h / 2 - h / 2 * crate::desktop_scene::shared::cos1024(a) / 1024,
                )
            })
            .collect(),
        ShapeKind::Rectangle | ShapeKind::RoundedRectangle => {
            vec![
                (x0, y0),
                (x0 + w, y0),
                (x0 + w, y0 + h),
                (x0, y0 + h),
                (x0, y0),
            ]
        }
        ShapeKind::Polygon => vec![
            (x0, y0 + h),
            (x0 + w / 3, y0),
            (x0 + w, y0 + h / 3),
            (x0 + w * 2 / 3, y0 + h),
            (x0, y0 + h),
        ],
        other => {
            let shape = Shape {
                kind: other,
                points: vec![(0, 0), (1000, 1000)],
                outline: None,
                fill: None,
                width: 1,
                antialias: true,
            };
            let mut pts: Vec<(i32, i32)> = shape
                .outline_points()
                .iter()
                .map(|(fx, fy)| {
                    (
                        x0 + (*fx * w as i64 / 1000) as i32,
                        y0 + (*fy * h as i64 / 1000) as i32,
                    )
                })
                .collect();
            if let Some(first) = pts.first().copied() {
                pts.push(first);
            }
            pts
        }
    };
    p.node(
        Rect::new(0, 0, p.scene.width, p.scene.height),
        Primitive::Path {
            points: pts,
            fill: None,
            stroke: Some(color),
            stroke_width: 1,
            closed: false,
        },
        None,
    );
}

fn caption(p: &mut Painter, s: &Skin, x: i32, y: i32, w: u32, text: &str) {
    p.label(x, y, w, text, 11, s.muted, false, Align::Center);
}
fn divider(p: &mut Painter, s: &Skin, x: i32, top: i32) {
    p.vline(x, top + 8, 72, s.line);
}

pub fn render(st: &Studio, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let s = skin(env.settings.dark_mode);
    let (w, h) = (env.width, env.height);
    p.scene.background = s.bg;
    let t = |c: &str| st.target(c);
    let doc = st.doc.as_ref();
    let has_sel = doc.and_then(|d| d.selection()).is_some();

    // Menu row: File, Edit, View, then undo and redo.
    p.box_(Rect::new(0, 0, w, 40), s.bar, 0);
    let mut x = 8;
    for (id, label) in [("file", "File"), ("edit", "Edit"), ("view", "View")] {
        let bw = p.measure(label, 13, false) + 20;
        let open = matches!(&st.panel, Some(super::Panel::Menu { id: m }) if m == id);
        p.button(
            Rect::new(x, 6, bw, 28),
            if open { s.selected } else { Color::TRANSPARENT },
            4,
            &t(&format!("menu:{id}")),
            label,
        );
        p.label(x, 11, bw, label, 13, s.text, false, Align::Center);
        x += bw as i32 + 2;
    }
    p.vline(x + 6, 12, 16, s.line);
    x += 14;
    for (symbol, command, live, why) in [
        (
            "undo",
            "undo",
            doc.is_some_and(|d| d.can_undo()),
            "Nothing to undo",
        ),
        (
            "redo",
            "redo",
            doc.is_some_and(|d| d.can_redo()),
            "Nothing to redo",
        ),
    ] {
        let r = Rect::new(x, 6, 30, 28);
        if live {
            view::tool(
                p,
                &s,
                r,
                symbol,
                if command == "undo" { "Undo" } else { "Redo" },
                &t(command),
                false,
            );
        } else {
            view::inert_tool(p, &s, r, symbol, why);
        }
        x += 32;
    }
    view::status(p, &s, x + 16, 13, w.saturating_sub(x as u32 + 24), st);
    p.hline(0, 40, w, s.line);

    // The toolbar.
    let top = 41;
    let bar_h = 96;
    p.box_(Rect::new(0, top, w, bar_h), s.bar, 0);
    p.hline(0, top + bar_h as i32, w, s.line);
    let cap_y = top + bar_h as i32 - 18;
    let mut x = 8;
    // Selection.
    view::tool(
        p,
        &s,
        Rect::new(x, top + 8, 44, 44),
        "select-rect",
        "Rectangle selection",
        &t("tool:select-rect"),
        st.tool == Tool::RectSelect,
    );
    view::tool(
        p,
        &s,
        Rect::new(x + 44, top + 8, 18, 44),
        "chevron-down",
        "Selection options",
        &t("menu:selection"),
        st.tool == Tool::Lasso,
    );
    caption(p, &s, x, cap_y, 62, "Selection");
    x += 70;
    divider(p, &s, x, top);
    x += 8;
    // Image.
    let crop = Rect::new(x, top + 8, 30, 26);
    if has_sel {
        view::tool(p, &s, crop, "crop", "Crop", &t("crop-selection"), false);
    } else {
        view::inert_tool(p, &s, crop, "crop", "Select an area to crop to");
    }
    view::tool(
        p,
        &s,
        Rect::new(x + 32, top + 8, 30, 26),
        "resize",
        "Resize and skew",
        &t("dialog:resize"),
        false,
    );
    view::tool(
        p,
        &s,
        Rect::new(x, top + 38, 30, 26),
        "rotate-cw",
        "Rotate",
        &t("menu:rotate"),
        false,
    );
    caption(p, &s, x, cap_y, 62, "Image");
    x += 70;
    divider(p, &s, x, top);
    x += 8;
    // Tools: pencil, fill, text / eraser, colour picker, magnifier.
    for (i, tool) in [
        Tool::Pencil,
        Tool::Fill,
        Tool::Text,
        Tool::Eraser,
        Tool::Picker,
        Tool::Zoom,
    ]
    .into_iter()
    .enumerate()
    {
        let r = Rect::new(
            x + (i as i32 % 3) * 30,
            top + 8 + (i as i32 / 3) * 30,
            28,
            28,
        );
        let label = match tool {
            Tool::Pencil => "Pencil",
            Tool::Fill => "Fill",
            Tool::Text => "Text",
            Tool::Eraser => "Eraser",
            Tool::Picker => "Color picker",
            _ => "Magnifier",
        };
        view::tool(
            p,
            &s,
            r,
            tool.symbol(),
            label,
            &t(&format!("tool:{}", tool.id())),
            st.tool == tool,
        );
    }
    caption(p, &s, x, cap_y, 90, "Tools");
    x += 96;
    divider(p, &s, x, top);
    x += 8;
    // Brushes.
    let brushing = matches!(st.tool, Tool::Brush | Tool::Airbrush);
    view::tool(
        p,
        &s,
        Rect::new(x, top + 8, 44, 44),
        "brush",
        "Brushes",
        &t("tool:brush"),
        brushing,
    );
    view::tool(
        p,
        &s,
        Rect::new(x + 44, top + 8, 18, 44),
        "chevron-down",
        "Brush types",
        &t("menu:brushes"),
        false,
    );
    caption(p, &s, x, cap_y, 62, "Brushes");
    x += 70;
    divider(p, &s, x, top);
    x += 8;
    // Shapes gallery with Outline and Fill.
    if w > 760 {
        for (i, kind) in SHAPES.iter().enumerate() {
            let r = Rect::new(
                x + (i as i32 % 6) * 24,
                top + 6 + (i as i32 / 6) * 23,
                22,
                22,
            );
            let on = st.tool == Tool::Shape && st.shape == *kind;
            p.button(
                r,
                if on { s.selected } else { Color::TRANSPARENT },
                3,
                &t(&format!("shape:{}", kind.id())),
                kind.id(),
            );
            shape_glyph(p, r, *kind, s.text);
        }
        let ox = x + 148;
        view::chip(
            p,
            &s,
            Rect::new(ox, top + 10, 70, 24),
            if st.outline {
                "Outline ▾"
            } else {
                "No outline ▾"
            },
            &t("menu:outline"),
            false,
        );
        view::chip(
            p,
            &s,
            Rect::new(ox, top + 40, 70, 24),
            if st.fill { "Fill ▾" } else { "No fill ▾" },
            &t("menu:fill"),
            false,
        );
        caption(p, &s, x, cap_y, 218, "Shapes");
        x += 226;
        divider(p, &s, x, top);
        x += 8;
    } else {
        view::tool(
            p,
            &s,
            Rect::new(x, top + 8, 44, 44),
            "shapes",
            "Shapes",
            &t("menu:shapes"),
            st.tool == Tool::Shape,
        );
        caption(p, &s, x, cap_y, 44, "Shapes");
        x += 52;
        divider(p, &s, x, top);
        x += 8;
    }
    // Size.
    view::tool(
        p,
        &s,
        Rect::new(x, top + 8, 40, 44),
        "line-tool",
        "Size",
        &t("menu:size"),
        false,
    );
    p.label(
        x,
        top + 54,
        40,
        &format!("{}px", st.size),
        10,
        s.muted,
        false,
        Align::Center,
    );
    caption(p, &s, x, cap_y, 40, "Size");
    x += 48;
    divider(p, &s, x, top);
    x += 8;
    // Colors: Color 1 over Color 2, the palette and Edit colors.
    let c1 = Rect::new(x + 2, top + 10, 30, 30);
    let c2 = Rect::new(x + 10, top + 44, 22, 22);
    view::swatch(p, &s, c1, st.primary, &t("slot:1"), st.slot == 0, true);
    view::swatch(p, &s, c2, st.secondary, &t("slot:2"), st.slot == 1, true);
    x += 44;
    let columns = if w > 1000 { 10 } else { 5 };
    for (i, c) in PALETTE.iter().enumerate() {
        if columns == 5 && i % 10 >= 5 {
            continue;
        }
        let col = (i % 10) as i32;
        let row = (i / 10) as i32;
        let r = Rect::new(x + col * 22, top + 12 + row * 24, 18, 18);
        let color = [c[0], c[1], c[2], 255];
        view::swatch(
            p,
            &s,
            r,
            color,
            &t(&format!("color:{}", cw_raster::hex(color))),
            false,
            true,
        );
    }
    let edit = Rect::new(x + columns * 22 + 4, top + 12, 38, 42);
    p.button(
        edit,
        Color::TRANSPARENT,
        4,
        &t("dialog:color"),
        "Edit colors",
    );
    p.circle(edit.x + 19, edit.y + 14, 11, Color::rgb(230, 120, 60));
    p.circle(edit.x + 19, edit.y + 14, 7, Color::rgb(80, 150, 230));
    p.symbol("plus", edit.x + 13, edit.y + 8, 12, Color::WHITE);
    caption(p, &s, x - 44, cap_y, columns as u32 * 22 + 90, "Colors");
    x += columns * 22 + 50;
    divider(p, &s, x, top);
    x += 8;
    // Layers.
    view::tool(
        p,
        &s,
        Rect::new(x, top + 8, 44, 44),
        "layers",
        "Layers",
        &t("layers"),
        st.layers_open,
    );
    caption(p, &s, x, cap_y, 44, "Layers");

    // Canvas, Layers pane and status bar.
    let status_h = 32;
    let body_top = top + bar_h as i32 + 1;
    let body_h = h.saturating_sub(body_top as u32 + status_h);
    let pane = if st.layers_open { 240.min(w / 3) } else { 0 };
    let area = Rect::new(0, body_top, w.saturating_sub(pane), body_h);
    if st.doc.is_some() {
        view::canvas(p, &s, area, st, true, env.pointer);
        view::scrollbars(p, &s, area, st);
    } else {
        p.box_(area, s.backdrop, 0);
        p.center(0, area.y + 40, area.width, "No image is open", 14, s.muted);
    }
    if pane > 0 {
        let px = w as i32 - pane as i32;
        p.box_(Rect::new(px, body_top, pane, body_h), s.bar, 0);
        p.vline(px, body_top, body_h, s.line);
        p.strong(px + 12, body_top + 12, 120, "Layers", 14, s.text);
        view::tool(
            p,
            &s,
            Rect::new(px + pane as i32 - 40, body_top + 8, 30, 28),
            "plus",
            "Add layer",
            &t("layer:new"),
            false,
        );
        view::layer_rows(
            p,
            &s,
            Rect::new(px + 8, body_top + 44, pane - 16, body_h.saturating_sub(180)),
            st,
            64,
        );
        let by = body_top + body_h as i32 - 128;
        view::slider(
            p,
            &s,
            Rect::new(px + 12, by, pane - 24, 36),
            st,
            "Opacity",
            "layer-opacity",
            true,
        );
        let mode = doc.map(|d| d.active_layer().blend).unwrap_or_default();
        view::chip(
            p,
            &s,
            Rect::new(px + 12, by + 42, pane - 24, 26),
            &format!("Blend mode: {} ▾", mode.label()),
            &t("menu:blend"),
            false,
        );
        let count = doc.map_or(0, |d| d.layers().len());
        let active = doc.map_or(0, |d| d.active());
        let mut bx = px + 12;
        for (symbol, command, label, live, why) in [
            ("copy", "layer:duplicate", "Duplicate layer", true, ""),
            (
                "chevron-up",
                "layer:up",
                "Move up",
                active + 1 < count,
                "The layer is already on top",
            ),
            (
                "chevron-down",
                "layer:down",
                "Move down",
                active > 0,
                "The layer is already at the bottom",
            ),
            (
                "layers",
                "layer:merge",
                "Merge down",
                active > 0,
                "There is no layer below",
            ),
            (
                "trash",
                "layer:delete",
                "Delete layer",
                count > 1,
                "An image keeps at least one layer",
            ),
        ] {
            let r = Rect::new(bx, by + 76, 34, 30);
            if live {
                view::tool(p, &s, r, symbol, label, &t(command), false);
            } else {
                view::inert_tool(p, &s, r, symbol, why);
            }
            bx += 40;
        }
    }
    let sy = h as i32 - status_h as i32;
    p.box_(Rect::new(0, sy, w, status_h), s.bar, 0);
    p.hline(0, sy, w, s.line);
    let mut sx = 12;
    // Where the pointer is on the picture, while it is over it.
    p.symbol("move", sx, sy + 8, 16, s.muted);
    if let Some((x, y)) = st.pointer_pixel(area, env.pointer) {
        p.left(sx + 22, sy + 8, 110, &format!("{x}, {y}px"), 12, s.text);
    }
    sx += 130;
    if let Some(sel) = view::selection_text(st) {
        p.symbol("select-rect", sx, sy + 8, 16, s.muted);
        p.left(sx + 22, sy + 8, 120, &sel, 12, s.text);
        sx += 150;
    }
    p.symbol("image", sx, sy + 8, 16, s.muted);
    p.left(sx + 22, sy + 8, 140, &view::size_text(st), 12, s.text);
    let zoom = st.effective_zoom(area.width, area.height);
    let zx = w as i32 - 250;
    p.label(
        zx,
        sy + 8,
        48,
        &format!("{zoom}%"),
        12,
        s.text,
        false,
        Align::Right,
    );
    view::tool(
        p,
        &s,
        Rect::new(zx + 54, sy + 4, 24, 24),
        "minus",
        "Zoom out",
        &t(&format!("zoom:out:{}:{}", area.width, area.height)),
        false,
    );
    view::slider(
        p,
        &s,
        Rect::new(zx + 82, sy + 6, 110, 20),
        st,
        "",
        "zoom",
        false,
    );
    view::tool(
        p,
        &s,
        Rect::new(zx + 196, sy + 4, 24, 24),
        "plus",
        "Zoom in",
        &t(&format!("zoom:in:{}:{}", area.width, area.height)),
        false,
    );
    view::tool(
        p,
        &s,
        Rect::new(zx + 222, sy + 4, 24, 24),
        "screenshot",
        "Fit to window",
        &t("zoom:fit"),
        st.zoom == 0,
    );

    menus(st, p, &s, has_sel);
    view::dialog(p, &s, st, w, h);
    view::chooser(p, &s, st, w, h);
}

fn menus(st: &Studio, p: &mut Painter, s: &Skin, has_sel: bool) {
    let Some(super::Panel::Menu { id }) = &st.panel else {
        return;
    };
    let doc = st.doc.as_ref();
    let can_undo = doc.is_some_and(|d| d.can_undo());
    let can_redo = doc.is_some_and(|d| d.can_redo());
    let open = doc.is_some();
    let (x, items): (i32, Vec<Item<'_>>) = match id.as_str() {
        "file" => (
            8,
            vec![
                Item::new("New", "new").key("Ctrl+N"),
                Item::new("Open", "open").key("Ctrl+O"),
                Item::separator(),
                Item::new("Save", "save")
                    .key("Ctrl+S")
                    .when(open, "No image is open"),
                Item::new("Save as", "save-as")
                    .key("Ctrl+Shift+S")
                    .when(open, "No image is open"),
                Item::separator(),
                Item::new("Save as PNG picture", "save-as:png").when(open, "No image is open"),
                Item::new("Save as JPEG picture", "save-as:jpg").when(open, "No image is open"),
                Item::new("Save as BMP picture", "save-as:bmp").when(open, "No image is open"),
            ],
        ),
        "edit" => (
            52,
            vec![
                Item::new("Undo", "undo")
                    .key("Ctrl+Z")
                    .when(can_undo, "Nothing to undo"),
                Item::new("Redo", "redo")
                    .key("Ctrl+Y")
                    .when(can_redo, "Nothing to redo"),
                Item::separator(),
                Item::new("Cut", "cut")
                    .key("Ctrl+X")
                    .when(has_sel, "Nothing is selected"),
                Item::new("Copy", "copy")
                    .key("Ctrl+C")
                    .when(open, "No image is open"),
                Item::new("Paste", "paste")
                    .key("Ctrl+V")
                    .when(open, "No image is open"),
                Item::separator(),
                Item::new("Select all", "select-all")
                    .key("Ctrl+A")
                    .when(open, "No image is open"),
                Item::new("Invert selection", "select-invert").when(open, "No image is open"),
                Item::new("Delete", "delete")
                    .key("Delete")
                    .when(has_sel, "Nothing is selected"),
            ],
        ),
        "view" => (
            96,
            vec![
                Item::new("Zoom in", "zoom:in").key("Ctrl++"),
                Item::new("Zoom out", "zoom:out").key("Ctrl+-"),
                Item::new("Actual size", "zoom:100"),
                Item::new("Fit to window", "zoom:fit")
                    .key("Ctrl+0")
                    .checked(st.zoom == 0),
                Item::separator(),
                Item::new("Layers", "layers").checked(st.layers_open),
            ],
        ),
        "selection" => (
            8,
            vec![
                Item::new("Rectangle", "tool:select-rect").checked(st.tool == Tool::RectSelect),
                Item::new("Free-form", "tool:lasso").checked(st.tool == Tool::Lasso),
                Item::separator(),
                Item::new("Select all", "select-all").key("Ctrl+A"),
                Item::new("Invert selection", "select-invert"),
                Item::new("Delete", "delete")
                    .key("Delete")
                    .when(has_sel, "Nothing is selected"),
            ],
        ),
        "rotate" => (
            78,
            vec![
                Item::new("Rotate right 90°", "rotate:cw"),
                Item::new("Rotate left 90°", "rotate:ccw"),
                Item::new("Rotate 180°", "rotate:180"),
                Item::separator(),
                Item::new("Flip vertical", "flip:v"),
                Item::new("Flip horizontal", "flip:h"),
            ],
        ),
        "brushes" => (
            250,
            vec![
                Item::new("Brush", "tool:brush").checked(st.tool == Tool::Brush),
                Item::new("Airbrush", "tool:airbrush").checked(st.tool == Tool::Airbrush),
            ],
        ),
        "outline" => (
            440,
            vec![
                Item::new("No outline", "outline:none").checked(!st.outline),
                Item::new("Solid color", "outline:solid").checked(st.outline),
            ],
        ),
        "fill" => (
            440,
            vec![
                Item::new("No fill", "fill:none").checked(!st.fill),
                Item::new("Solid color", "fill:solid").checked(st.fill),
            ],
        ),
        "shapes" => (
            320,
            SHAPES
                .iter()
                .map(|k| {
                    Item::new(k.id(), &format!("shape:{}", k.id()))
                        .checked(st.tool == Tool::Shape && st.shape == *k)
                })
                .collect(),
        ),
        "size" => (
            520,
            [1u32, 2, 3, 5, 8, 12, 20, 30, 40]
                .iter()
                .map(|n| Item::new(size_label(*n), &format!("set:size:{n}")).checked(st.size == *n))
                .collect(),
        ),
        "blend" => (
            (p.scene.width as i32 - 240).max(0),
            cw_raster::BlendMode::ALL
                .iter()
                .map(|m| {
                    Item::new(m.label(), &format!("layer:blend:{}", m.id()))
                        .checked(doc.is_some_and(|d| d.active_layer().blend == *m))
                })
                .collect(),
        ),
        _ => return,
    };
    let y = if matches!(id.as_str(), "file" | "edit" | "view") {
        38
    } else {
        132
    };
    view::menu(p, s, st, x.max(0), y, 220, &items);
}

fn size_label(n: u32) -> &'static str {
    match n {
        1 => "1px",
        2 => "2px",
        3 => "3px",
        5 => "5px",
        8 => "8px",
        12 => "12px",
        20 => "20px",
        30 => "30px",
        _ => "40px",
    }
}
