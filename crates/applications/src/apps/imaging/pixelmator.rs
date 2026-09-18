//! Pixelmator Pro: a dark window with the Layers sidebar on the left, the Tools sidebar
//! on the right — a grid of round tools over the chosen tool's options, or the Adjust
//! Colors and Effects browsers — and the image between them.
use super::view::{self, Item, Skin};
use super::{Panel, Studio, Tool};
use crate::desktop_scene::{shared::Align, Painter};
use cw_raster::draw::ShapeKind;
use cw_raster::mask::SelectMode;
use cw_scene::{Color, Rect};

fn skin() -> Skin {
    Skin {
        bg: Color::rgb(30, 30, 30),
        bar: Color::rgb(42, 42, 42),
        panel: Color::rgb(35, 35, 35),
        text: Color::rgb(229, 229, 229),
        muted: Color::rgb(150, 150, 150),
        accent: Color::rgb(10, 132, 255),
        on_accent: Color::WHITE,
        selected: Color(10, 132, 255, 90),
        line: Color(255, 255, 255, 22),
        backdrop: Color::rgb(26, 26, 26),
        radius: 6,
        font: 12,
    }
}

/// The tools grid: label, symbol, and what choosing it does.
const GRID: [(&str, &str, &str); 15] = [
    ("Arrange", "move", "tool:move"),
    ("Adjust Colors", "sliders", "tab:adjust"),
    ("Effects", "filters", "tab:effects"),
    ("Select", "select-rect", "tool:select-rect"),
    ("Crop", "crop", "tool:crop"),
    ("Repair", "magic", "tool:repair"),
    ("Clone", "stamp", "tool:clone"),
    ("Paint", "brush", "tool:brush"),
    ("Erase", "eraser", "tool:eraser"),
    ("Fill", "bucket", "tool:fill"),
    ("Gradient", "contrast", "tool:gradient"),
    ("Shapes", "shapes", "tool:shape"),
    ("Type", "text-tool", "tool:text"),
    ("Color Picker", "eyedropper", "tool:picker"),
    ("Zoom", "zoom-in", "tool:zoom"),
];

/// Adjust Colors browser: label, and the dialog or one-shot action it opens.
const ADJUSTMENTS: [(&str, &str); 13] = [
    ("White Balance", "dialog:temperature"),
    ("Hue & Saturation", "dialog:hue-saturation"),
    ("Exposure", "dialog:exposure"),
    ("Brightness & Contrast", "dialog:brightness-contrast"),
    ("Shadows & Highlights", "dialog:shadows-highlights"),
    ("Color Balance", "dialog:color-balance"),
    ("Levels", "dialog:levels"),
    ("Curves", "dialog:curves"),
    ("Black & White", "action:grayscale"),
    ("Invert", "action:invert"),
    ("Posterize", "dialog:posterize"),
    ("Threshold", "dialog:threshold"),
    ("Auto Enhance", "action:auto-levels"),
];

/// Effects browser, grouped as Pixelmator groups them.
const EFFECTS: [(&str, &str, &str); 8] = [
    ("Blur", "Gaussian", "dialog:gaussian-blur"),
    ("Blur", "Box", "dialog:box-blur"),
    ("Sharpen", "Sharpen", "dialog:sharpen"),
    ("Stylize", "Vignette", "dialog:vignette"),
    ("Stylize", "Pixelate", "dialog:pixelate"),
    ("Stylize", "Emboss", "action:emboss"),
    ("Other", "Edges", "action:edge-detect"),
    ("Other", "Noise Reduction", "dialog:median"),
];

pub fn render(st: &Studio, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let s = skin();
    let (w, h) = (env.width, env.height);
    p.scene.background = s.bg;
    let t = |c: &str| st.target(c);
    let doc = st.doc.as_ref();
    let open = doc.is_some();

    let left = 220u32.min(w / 4);
    let right = 260u32.min(w / 3);
    let top = 44;
    let cw = w.saturating_sub(left + right);
    let area = Rect::new(left as i32, top, cw, h.saturating_sub(top as u32));

    // Toolbar.
    p.box_(Rect::new(0, 0, w, 44), s.bar, 0);
    p.hline(0, 44, w, Color(0, 0, 0, 120));
    if open {
        let z = st.effective_zoom(area.width, area.height);
        view::chip(
            p,
            &s,
            Rect::new(12, 10, 78, 24),
            &format!("{z}% ▾"),
            &t("menu:zoom"),
            false,
        );
    }
    p.strong_center(0, 13, w, &st.document_name(), 13, s.text);
    for (i, (symbol, label, command, live)) in [
        ("share", "Export", "save-as", open),
        ("undo", "Undo", "undo", doc.is_some_and(|d| d.can_undo())),
    ]
    .into_iter()
    .enumerate()
    {
        let r = Rect::new(w as i32 - 44 - i as i32 * 40, 8, 32, 28);
        if live {
            view::tool(p, &s, r, symbol, label, &t(command), false);
        } else {
            view::inert_tool(
                p,
                &s,
                r,
                symbol,
                if command == "undo" {
                    "Nothing to undo"
                } else {
                    "No image is open"
                },
            );
        }
    }

    // Layers sidebar.
    p.box_(Rect::new(0, top + 1, left, h), s.panel, 0);
    p.vline(left as i32, top, h, Color(0, 0, 0, 120));
    p.strong(14, top + 12, left - 28, "Layers", 12, s.muted);
    if let Some(d) = doc {
        view::layer_rows(
            p,
            &s,
            Rect::new(6, top + 36, left - 12, h.saturating_sub(top as u32 + 160)),
            st,
            46,
        );
        let by = h as i32 - 116;
        view::slider(
            p,
            &s,
            Rect::new(12, by, left - 24, 34),
            st,
            "Opacity",
            "layer-opacity",
            true,
        );
        view::chip(
            p,
            &s,
            Rect::new(12, by + 40, left - 24, 24),
            &format!("{} ▾", d.active_layer().blend.label()),
            &t("menu:blend"),
            false,
        );
        let count = d.layers().len();
        let active = d.active();
        let mut bx = 10;
        for (symbol, command, label, live, why) in [
            ("plus", "layer:new", "Add Layer", true, ""),
            (
                "minus",
                "layer:delete",
                "Delete Layer",
                count > 1,
                "An image keeps at least one layer",
            ),
            ("copy", "layer:duplicate", "Duplicate", true, ""),
            (
                "chevron-up",
                "layer:up",
                "Bring Forward",
                active + 1 < count,
                "The layer is already at the top",
            ),
            (
                "chevron-down",
                "layer:down",
                "Send Backward",
                active > 0,
                "The layer is already at the bottom",
            ),
            (
                "layers",
                "layer:merge",
                "Merge Down",
                active > 0,
                "There is no layer below",
            ),
        ] {
            let r = Rect::new(bx, h as i32 - 44, 30, 30);
            if live {
                view::tool(p, &s, r, symbol, label, &t(command), false);
            } else {
                view::inert_tool(p, &s, r, symbol, why);
            }
            bx += 34;
        }
    }

    // The image.
    if open {
        view::canvas(p, &s, area, st, false, env.pointer);
        view::scrollbars(p, &s, area, st);
    } else {
        p.box_(area, s.backdrop, 0);
        let cx = area.x + area.width as i32 / 2;
        p.strong_center(
            area.x,
            area.y + 80,
            area.width,
            "Pixelmator Pro",
            20,
            s.text,
        );
        view::button(
            p,
            &s,
            Rect::new(cx - 130, area.y + 124, 124, 32),
            "New Image",
            &t("dialog:new-image"),
            false,
        );
        view::button(
            p,
            &s,
            Rect::new(cx + 6, area.y + 124, 124, 32),
            "Open…",
            &t("open"),
            true,
        );
        view::status(p, &s, area.x + 20, area.y + 172, area.width - 40, st);
    }

    // Tools sidebar.
    let rx = w as i32 - right as i32;
    p.box_(Rect::new(rx, top + 1, right, h), s.panel, 0);
    p.vline(rx, top, h, Color(0, 0, 0, 120));
    let cols = 4;
    let cell = ((right - 16) / cols).min(60);
    for (i, (label, symbol, command)) in GRID.iter().enumerate() {
        let cx = rx + 8 + (i as u32 % cols * cell) as i32;
        let cy = top + 10 + (i as u32 / cols * (cell + 4)) as i32;
        let on = match *command {
            "tab:adjust" => st.tab == "adjust",
            "tab:effects" => st.tab == "effects",
            c => {
                st.tab.is_empty()
                    && Tool::parse(c.trim_start_matches("tool:")).is_some_and(|tool| {
                        tool == st.tool
                            || (tool == Tool::RectSelect
                                && matches!(
                                    st.tool,
                                    Tool::EllipseSelect | Tool::Lasso | Tool::MagicWand
                                ))
                            || (tool == Tool::Brush && st.tool == Tool::Pencil)
                    })
            }
        };
        let r = Rect::new(cx + (cell as i32 - 38) / 2, cy, 38, 38);
        if open {
            p.button(
                r,
                if on {
                    s.accent
                } else {
                    Color(255, 255, 255, 18)
                },
                19,
                &t(command),
                label,
            );
            p.symbol(
                symbol,
                r.x + 10,
                r.y + 10,
                18,
                if on { Color::WHITE } else { s.text },
            );
        } else {
            p.box_(r, Color(255, 255, 255, 10), 19);
            p.disabled("Open or create an image first");
            p.symbol(symbol, r.x + 10, r.y + 10, 18, s.muted);
        }
        p.label(cx, cy + 40, cell, label, 9, s.muted, false, Align::Center);
    }
    let rows = (GRID.len() as u32).div_ceil(cols);
    let oy = top + 14 + (rows * (cell + 4)) as i32;
    p.hline(rx, oy, right, s.line);
    let ow = right - 24;
    let ox = rx + 12;
    let mut y = oy + 10;
    if open {
        match st.tab.as_str() {
            "adjust" => {
                p.strong(ox, y, ow, "Adjust Colors", 13, s.text);
                y += 26;
                for (label, command) in ADJUSTMENTS {
                    let r = Rect::new(ox, y, ow, 26);
                    p.button(r, Color(255, 255, 255, 12), 5, &t(command), label);
                    p.label(
                        r.x + 10,
                        r.y + 5,
                        r.width - 20,
                        label,
                        12,
                        s.text,
                        false,
                        Align::Left,
                    );
                    p.symbol(
                        "chevron-right",
                        r.x + r.width as i32 - 20,
                        r.y + 7,
                        12,
                        s.muted,
                    );
                    y += 29;
                }
            }
            "effects" => {
                p.strong(ox, y, ow, "Effects", 13, s.text);
                y += 26;
                let mut group = "";
                for (g, label, command) in EFFECTS {
                    if g != group {
                        p.label(ox, y + 2, ow, g, 11, s.muted, true, Align::Left);
                        y += 20;
                        group = g;
                    }
                    let r = Rect::new(ox, y, ow, 26);
                    p.button(r, Color(255, 255, 255, 12), 5, &t(command), label);
                    p.label(
                        r.x + 10,
                        r.y + 5,
                        r.width - 20,
                        label,
                        12,
                        s.text,
                        false,
                        Align::Left,
                    );
                    y += 29;
                }
            }
            _ => options(st, p, &s, ox, y, ow),
        }
    }

    menus(st, p, &s, left);
    view::dialog(p, &s, st, w, h);
    view::chooser(p, &s, st, w, h);
}

fn options(st: &Studio, p: &mut Painter, s: &Skin, ox: i32, mut y: i32, ow: u32) {
    let t = |c: &str| st.target(c);
    let title = match st.tool {
        Tool::Move => "Arrange",
        Tool::RectSelect | Tool::EllipseSelect | Tool::Lasso | Tool::MagicWand => "Select",
        Tool::Crop => "Crop",
        Tool::Brush | Tool::Pencil => "Paint",
        Tool::Eraser => "Erase",
        Tool::Fill => "Fill",
        Tool::Shape => "Shapes",
        Tool::Text => "Type",
        Tool::Picker => "Color Picker",
        Tool::Gradient => "Gradient",
        Tool::Clone => "Clone",
        Tool::Repair => "Repair",
        _ => "Zoom",
    };
    p.strong(ox, y, ow, title, 13, s.text);
    y += 28;
    let color_well = |p: &mut Painter, y: i32| {
        p.label(ox, y + 5, 60, "Color", 12, s.text, false, Align::Left);
        view::swatch(
            p,
            s,
            Rect::new(ox + ow as i32 - 40, y, 40, 22),
            st.primary,
            &t("dialog:color"),
            false,
            false,
        );
    };
    match st.tool {
        Tool::Move => {
            p.paragraph(
                ox,
                y,
                ow,
                "Drag the image to move the selected layer.",
                11,
                s.muted,
            );
            y += 40;
            for (i, (label, command)) in [
                ("Flip Horizontal", "flip:h"),
                ("Flip Vertical", "flip:v"),
                ("Rotate Left", "rotate:ccw"),
                ("Rotate Right", "rotate:cw"),
            ]
            .iter()
            .enumerate()
            {
                view::button(
                    p,
                    s,
                    Rect::new(
                        ox + (i as i32 % 2) * (ow as i32 / 2 + 2),
                        y + (i as i32 / 2) * 32,
                        ow / 2 - 2,
                        28,
                    ),
                    label,
                    &t(command),
                    false,
                );
            }
        }
        Tool::RectSelect | Tool::EllipseSelect | Tool::Lasso | Tool::MagicWand => {
            for (i, (label, tool)) in [
                ("Rectangular", Tool::RectSelect),
                ("Elliptical", Tool::EllipseSelect),
                ("Freeform", Tool::Lasso),
                ("Select Color", Tool::MagicWand),
            ]
            .iter()
            .enumerate()
            {
                view::chip(
                    p,
                    s,
                    Rect::new(
                        ox + (i as i32 % 2) * (ow as i32 / 2 + 2),
                        y + (i as i32 / 2) * 28,
                        ow / 2 - 2,
                        24,
                    ),
                    label,
                    &t(&format!("tool:{}", tool.id())),
                    st.tool == *tool,
                );
            }
            y += 62;
            for (i, (label, mode)) in [
                ("New", SelectMode::Replace),
                ("Add", SelectMode::Add),
                ("Subtract", SelectMode::Subtract),
                ("Intersect", SelectMode::Intersect),
            ]
            .iter()
            .enumerate()
            {
                view::chip(
                    p,
                    s,
                    Rect::new(ox + i as i32 * (ow as i32 / 4), y, ow / 4 - 3, 24),
                    label,
                    &t(&format!("mode:{}", mode.id())),
                    st.select_mode == *mode,
                );
            }
            y += 34;
            if st.tool == Tool::MagicWand {
                view::slider(
                    p,
                    s,
                    Rect::new(ox, y, ow, 34),
                    st,
                    "Intensity",
                    "tolerance",
                    true,
                );
            }
        }
        Tool::Crop => match st.crop {
            Some(r) => {
                p.label(
                    ox,
                    y,
                    ow,
                    &format!("{} × {} px", r.w, r.h),
                    12,
                    s.text,
                    false,
                    Align::Left,
                );
                view::button(
                    p,
                    s,
                    Rect::new(ox, y + 26, ow, 30),
                    "Apply",
                    &t("crop-apply"),
                    true,
                );
                view::button(
                    p,
                    s,
                    Rect::new(ox, y + 62, ow, 28),
                    "Cancel",
                    &t("cancel"),
                    false,
                );
            }
            None => {
                p.paragraph(
                    ox,
                    y,
                    ow,
                    "Drag over the image to set the crop.",
                    11,
                    s.muted,
                );
            }
        },
        Tool::Brush | Tool::Pencil => {
            view::chip(
                p,
                s,
                Rect::new(ox, y, ow / 2 - 2, 24),
                "Brush",
                &t("tool:brush"),
                st.tool == Tool::Brush,
            );
            view::chip(
                p,
                s,
                Rect::new(ox + ow as i32 / 2 + 2, y, ow / 2 - 2, 24),
                "Pencil",
                &t("tool:pencil"),
                st.tool == Tool::Pencil,
            );
            y += 34;
            color_well(p, y);
            y += 32;
            view::slider(p, s, Rect::new(ox, y, ow, 34), st, "Size", "size", true);
            y += 40;
            view::slider(
                p,
                s,
                Rect::new(ox, y, ow, 34),
                st,
                "Opacity",
                "opacity",
                true,
            );
            y += 40;
            if st.tool == Tool::Brush {
                view::slider(
                    p,
                    s,
                    Rect::new(ox, y, ow, 34),
                    st,
                    "Hardness",
                    "hardness",
                    true,
                );
            }
        }
        Tool::Eraser => {
            view::slider(p, s, Rect::new(ox, y, ow, 34), st, "Size", "size", true);
            y += 40;
            view::slider(
                p,
                s,
                Rect::new(ox, y, ow, 34),
                st,
                "Opacity",
                "opacity",
                true,
            );
        }
        Tool::Fill => {
            color_well(p, y);
            y += 32;
            view::slider(
                p,
                s,
                Rect::new(ox, y, ow, 34),
                st,
                "Tolerance",
                "tolerance",
                true,
            );
        }
        Tool::Shape => {
            let kinds = [
                ("Rectangle", ShapeKind::Rectangle),
                ("Rounded", ShapeKind::RoundedRectangle),
                ("Ellipse", ShapeKind::Ellipse),
                ("Polygon", ShapeKind::Hexagon),
                ("Star", ShapeKind::Star),
                ("Line", ShapeKind::Line),
                ("Arrow", ShapeKind::Arrow),
            ];
            for (i, (label, kind)) in kinds.iter().enumerate() {
                view::chip(
                    p,
                    s,
                    Rect::new(
                        ox + (i as i32 % 3) * (ow as i32 / 3),
                        y + (i as i32 / 3) * 28,
                        ow / 3 - 3,
                        24,
                    ),
                    label,
                    &t(&format!("shape:{}", kind.id())),
                    st.shape == *kind,
                );
            }
            y += 92;
            view::chip(
                p,
                s,
                Rect::new(ox, y, ow / 2 - 2, 24),
                "Fill",
                &t(if st.fill { "fill:none" } else { "fill:solid" }),
                st.fill,
            );
            view::chip(
                p,
                s,
                Rect::new(ox + ow as i32 / 2 + 2, y, ow / 2 - 2, 24),
                "Stroke",
                &t(if st.outline {
                    "outline:none"
                } else {
                    "outline:solid"
                }),
                st.outline,
            );
            y += 34;
            p.label(
                ox,
                y + 5,
                80,
                "Stroke color",
                12,
                s.text,
                false,
                Align::Left,
            );
            view::swatch(
                p,
                s,
                Rect::new(ox + ow as i32 - 40, y, 40, 22),
                st.primary,
                &t("dialog:color"),
                false,
                false,
            );
            y += 30;
            view::slider(
                p,
                s,
                Rect::new(ox, y, ow, 34),
                st,
                "Stroke width",
                "size",
                true,
            );
        }
        Tool::Text => {
            color_well(p, y);
            y += 32;
            view::slider(
                p,
                s,
                Rect::new(ox, y, ow, 34),
                st,
                "Size",
                "font-size",
                true,
            );
            y += 40;
            view::chip(p, s, Rect::new(ox, y, 60, 24), "Bold", &t("bold"), st.bold);
        }
        Tool::Picker => {
            color_well(p, y);
            y += 32;
            view::chip(
                p,
                s,
                Rect::new(ox, y, ow, 24),
                "Sample all layers",
                &t("merged"),
                st.merged,
            );
        }
        Tool::Gradient => {
            use cw_raster::gradient::GradientShape as G;
            for (i, (label, shape)) in [
                ("Linear", G::Linear),
                ("Radial", G::Radial),
                ("Angle", G::ConicalAsymmetric),
            ]
            .iter()
            .enumerate()
            {
                view::chip(
                    p,
                    s,
                    Rect::new(ox + i as i32 * (ow as i32 / 3), y, ow / 3 - 3, 24),
                    label,
                    &t(&format!("gradient-shape:{}", shape.id())),
                    st.gradient.shape == *shape,
                );
            }
            y += 32;
            for (i, (label, command, on)) in [
                (
                    "Color to Color",
                    "gradient-colors:fg-bg",
                    !st.gradient.transparent,
                ),
                (
                    "Color to Clear",
                    "gradient-colors:fg-transparent",
                    st.gradient.transparent,
                ),
            ]
            .iter()
            .enumerate()
            {
                view::chip(
                    p,
                    s,
                    Rect::new(ox + i as i32 * (ow as i32 / 2 + 2), y, ow / 2 - 2, 24),
                    label,
                    &t(command),
                    *on,
                );
            }
            y += 32;
            color_well(p, y);
            y += 30;
            view::chip(
                p,
                s,
                Rect::new(ox, y, ow, 24),
                "Reverse",
                &t("gradient-reverse"),
                st.gradient.reverse,
            );
            y += 32;
            view::slider(
                p,
                s,
                Rect::new(ox, y, ow, 34),
                st,
                "Opacity",
                "opacity",
                true,
            );
        }
        Tool::Clone | Tool::Repair => {
            view::slider(p, s, Rect::new(ox, y, ow, 34), st, "Size", "size", true);
            y += 40;
            if st.tool == Tool::Repair {
                p.paragraph(
                    ox,
                    y,
                    ow,
                    "Paint over what to remove; it is rebuilt from what surrounds it when you let go.",
                    11,
                    s.muted,
                );
                return;
            }
            view::slider(
                p,
                s,
                Rect::new(ox, y, ow, 34),
                st,
                "Opacity",
                "opacity",
                true,
            );
            y += 40;
            view::slider(
                p,
                s,
                Rect::new(ox, y, ow, 34),
                st,
                "Hardness",
                "hardness",
                true,
            );
            y += 40;
            view::chip(
                p,
                s,
                Rect::new(ox, y, ow, 24),
                "Sample all layers",
                &t("merged"),
                st.merged,
            );
            y += 30;
            view::chip(
                p,
                s,
                Rect::new(ox, y, ow, 24),
                "Fix source position",
                &t(if st.retouch.aligned {
                    "aligned:off"
                } else {
                    "aligned:on"
                }),
                !st.retouch.aligned,
            );
            y += 32;
            p.paragraph(
                ox,
                y,
                ow,
                match st.retouch.source {
                    Some(_) => "Option-click to choose another source.",
                    None => "Option-click the image to choose the source.",
                },
                11,
                s.muted,
            );
        }
        _ => {
            p.paragraph(ox, y, ow, "Click the image to zoom in.", 11, s.muted);
        }
    }
}

fn menus(st: &Studio, p: &mut Painter, s: &Skin, left: u32) {
    let Some(Panel::Menu { id }) = &st.panel else {
        return;
    };
    let doc = st.doc.as_ref();
    match id.as_str() {
        "zoom" => {
            let items = vec![
                Item::new("Zoom to Fit", "zoom:fit").checked(st.zoom == 0),
                Item::new("50%", "zoom:50").checked(st.zoom == 50),
                Item::new("100%", "zoom:100").checked(st.zoom == 100),
                Item::new("200%", "zoom:200").checked(st.zoom == 200),
                Item::new("400%", "zoom:400").checked(st.zoom == 400),
            ];
            view::menu(p, s, st, 12, 36, 160, &items);
        }
        "blend" => {
            let items: Vec<Item<'_>> = cw_raster::BlendMode::ALL
                .iter()
                .map(|m| {
                    let label = if *m == cw_raster::BlendMode::Add {
                        "Linear Dodge"
                    } else {
                        m.label()
                    };
                    Item::new(label, &format!("layer:blend:{}", m.id()))
                        .checked(doc.is_some_and(|d| d.active_layer().blend == *m))
                })
                .collect();
            view::menu(p, s, st, 12, p.scene.height as i32 - 300, left - 24, &items);
        }
        _ => {}
    }
}
