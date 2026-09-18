//! GIMP 2.10 in single-window mode with its default dark theme: the menu bar, the image
//! tab, the toolbox with FG/BG colours and Tool Options on the left, rulers around the
//! canvas, the Layers / Undo dock on the right and the status bar.
use super::view::{self, Item, Skin};
use super::{Panel, Studio, Tool};
use crate::desktop_scene::{shared::Align, Painter};
use cw_raster::mask::SelectMode;
use cw_scene::{Color, Rect};

fn skin() -> Skin {
    Skin {
        bg: Color::rgb(69, 69, 69),
        bar: Color::rgb(60, 60, 60),
        panel: Color::rgb(56, 56, 56),
        text: Color::rgb(221, 221, 221),
        muted: Color::rgb(160, 160, 160),
        accent: Color::rgb(116, 153, 204),
        on_accent: Color::WHITE,
        selected: Color::rgb(94, 94, 94),
        line: Color(0, 0, 0, 90),
        backdrop: Color::rgb(51, 51, 51),
        radius: 2,
        font: 12,
    }
}

const MENUS: [(&str, &str); 11] = [
    ("file", "File"),
    ("edit", "Edit"),
    ("select", "Select"),
    ("view", "View"),
    ("image", "Image"),
    ("layer", "Layer"),
    ("colors", "Colors"),
    ("tools", "Tools"),
    ("filters", "Filters"),
    ("windows", "Windows"),
    ("help", "Help"),
];

fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Move => "Move",
        Tool::RectSelect => "Rectangle Select",
        Tool::EllipseSelect => "Ellipse Select",
        Tool::Lasso => "Free Select",
        Tool::MagicWand => "Fuzzy Select",
        Tool::Crop => "Crop",
        Tool::Text => "Text",
        Tool::Fill => "Bucket Fill",
        Tool::Brush => "Paintbrush",
        Tool::Pencil => "Pencil",
        Tool::Airbrush => "Airbrush",
        Tool::Eraser => "Eraser",
        Tool::Picker => "Color Picker",
        Tool::Zoom => "Zoom",
        Tool::Pan => "Pan",
        _ => "Tool",
    }
}

pub fn render(st: &Studio, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let s = skin();
    let (w, h) = (env.width, env.height);
    p.scene.background = s.bg;
    let t = |c: &str| st.target(c);

    // Menu bar.
    p.box_(Rect::new(0, 0, w, 26), s.bar, 0);
    let mut x = 6;
    for (id, label) in MENUS {
        let bw = p.measure(label, 12, false) + 16;
        let r = Rect::new(x, 2, bw, 22);
        if matches!(id, "windows" | "help") {
            p.box_(r, Color::TRANSPARENT, 0);
            p.disabled(if id == "windows" {
                "This window's docks are fixed in single-window mode"
            } else {
                "The GIMP user manual is not installed"
            });
            p.label(x, 6, bw, label, 12, s.muted, false, Align::Center);
        } else {
            let open = matches!(&st.panel, Some(Panel::Menu { id: m }) if m == id);
            p.button(
                r,
                if open { s.selected } else { Color::TRANSPARENT },
                2,
                &t(&format!("menu:{id}")),
                label,
            );
            p.label(x, 6, bw, label, 12, s.text, false, Align::Center);
        }
        x += bw as i32;
    }
    p.hline(0, 26, w, s.line);

    let left = 206u32.min(w / 4);
    let right = 250u32.min(w / 4);
    let status_h = 24;
    let top = 27;
    let body_h = h.saturating_sub(top as u32 + status_h);

    // Toolbox.
    p.box_(Rect::new(0, top, left, body_h), s.panel, 0);
    p.vline(left as i32, top, body_h, s.line);
    let cols = ((left - 12) / 30).max(1) as i32;
    let tools = st.product.tools();
    for (i, tool) in tools.iter().enumerate() {
        let r = Rect::new(
            6 + (i as i32 % cols) * 30,
            top + 6 + (i as i32 / cols) * 30,
            28,
            28,
        );
        view::tool(
            p,
            &s,
            r,
            tool.symbol(),
            tool_name(*tool),
            &t(&format!("tool:{}", tool.id())),
            st.tool == *tool,
        );
    }
    let rows = (tools.len() as i32 + cols - 1) / cols;
    let sw_y = top + 12 + rows * 30;
    // FG/BG colour squares with swap and reset, as in the toolbox.
    let fg = Rect::new(10, sw_y, 30, 30);
    let bg = Rect::new(26, sw_y + 16, 30, 30);
    view::swatch(p, &s, bg, st.secondary, &t("dialog:color"), false, false);
    p.z += 1;
    view::swatch(p, &s, fg, st.primary, &t("dialog:color"), false, false);
    p.z -= 1;
    view::tool(
        p,
        &s,
        Rect::new(60, sw_y, 18, 18),
        "sort",
        "Swap colors",
        &t("swap-colors"),
        false,
    );
    view::tool(
        p,
        &s,
        Rect::new(60, sw_y + 26, 18, 18),
        "reload",
        "Default colors",
        &t("reset-colors"),
        false,
    );
    // Tool Options.
    let oy = sw_y + 56;
    p.hline(0, oy, left, s.line);
    p.strong(8, oy + 6, left - 16, "Tool Options", 12, s.text);
    p.label(
        8,
        oy + 26,
        left - 16,
        tool_name(st.tool),
        12,
        s.accent,
        true,
        Align::Left,
    );
    let ow = left - 16;
    let mut y = oy + 48;
    if st.tool.paints() {
        let mode = st.brush().blend;
        p.label(
            8,
            y,
            ow,
            &format!("Mode: {}", mode.label()),
            11,
            s.muted,
            false,
            Align::Left,
        );
        y += 20;
        view::slider(
            p,
            &s,
            Rect::new(8, y, ow, 34),
            st,
            "Opacity",
            "opacity",
            true,
        );
        y += 40;
        view::slider(p, &s, Rect::new(8, y, ow, 34), st, "Size", "size", true);
        y += 40;
        if st.tool != Tool::Pencil {
            view::slider(
                p,
                &s,
                Rect::new(8, y, ow, 34),
                st,
                "Hardness",
                "hardness",
                true,
            );
        }
    } else if matches!(
        st.tool,
        Tool::RectSelect | Tool::EllipseSelect | Tool::Lasso | Tool::MagicWand
    ) {
        p.label(8, y, ow, "Mode:", 11, s.muted, false, Align::Left);
        y += 18;
        for (i, mode) in SelectMode::ALL.iter().enumerate() {
            let label = match mode {
                SelectMode::Replace => "Replace",
                SelectMode::Add => "Add",
                SelectMode::Subtract => "Subtract",
                SelectMode::Intersect => "Intersect",
            };
            view::chip(
                p,
                &s,
                Rect::new(
                    8 + (i as i32 % 2) * (ow as i32 / 2),
                    y + (i as i32 / 2) * 26,
                    ow / 2 - 4,
                    22,
                ),
                label,
                &t(&format!("mode:{}", mode.id())),
                st.select_mode == *mode,
            );
        }
        y += 58;
        if st.tool == Tool::MagicWand {
            view::slider(
                p,
                &s,
                Rect::new(8, y, ow, 34),
                st,
                "Threshold",
                "tolerance",
                true,
            );
            y += 40;
            view::chip(
                p,
                &s,
                Rect::new(8, y, ow, 22),
                "Sample merged",
                &t("merged"),
                st.merged,
            );
        }
    } else if st.tool == Tool::Fill {
        view::slider(
            p,
            &s,
            Rect::new(8, y, ow, 34),
            st,
            "Threshold",
            "tolerance",
            true,
        );
        y += 40;
        view::chip(
            p,
            &s,
            Rect::new(8, y, ow, 22),
            "Sample merged",
            &t("merged"),
            st.merged,
        );
    } else if st.tool == Tool::Text {
        view::slider(
            p,
            &s,
            Rect::new(8, y, ow, 34),
            st,
            "Size",
            "font-size",
            true,
        );
        y += 40;
        view::chip(p, &s, Rect::new(8, y, ow, 22), "Bold", &t("bold"), st.bold);
        y += 28;
        p.paragraph(
            8,
            y,
            ow,
            "Click the image and type; Enter commits.",
            11,
            s.muted,
        );
    } else if st.tool == Tool::Crop {
        match st.crop {
            Some(r) => {
                p.label(
                    8,
                    y,
                    ow,
                    &format!("{} × {} at {}, {}", r.w, r.h, r.x, r.y),
                    11,
                    s.text,
                    false,
                    Align::Left,
                );
                view::button(
                    p,
                    &s,
                    Rect::new(8, y + 22, ow, 26),
                    "Crop",
                    &t("crop-apply"),
                    true,
                );
            }
            None => {
                p.paragraph(8, y, ow, "Drag a rectangle over the image.", 11, s.muted);
            }
        }
    } else if st.tool == Tool::Picker {
        view::chip(
            p,
            &s,
            Rect::new(8, y, ow, 22),
            "Sample merged",
            &t("merged"),
            st.merged,
        );
    }

    // Image tab, rulers, canvas.
    let cx = left as i32 + 1;
    let cw = w.saturating_sub(left + right + 2);
    let tab_h: i32 = if st.doc.is_some() { 30 } else { 0 };
    if st.doc.is_some() {
        p.box_(Rect::new(cx, top, cw, tab_h as u32), s.bar, 0);
        let tab = Rect::new(cx + 4, top + 3, 150.min(cw.saturating_sub(8)), 26);
        p.box_(tab, s.bg, 2);
        p.label(
            tab.x + 8,
            tab.y + 6,
            tab.width - 16,
            &st.document_name(),
            12,
            s.text,
            false,
            Align::Left,
        );
    }
    let ruler: i32 = 18;
    let area = Rect::new(
        cx + ruler,
        top + tab_h + ruler,
        cw.saturating_sub(ruler as u32),
        body_h.saturating_sub((tab_h + ruler) as u32),
    );
    if st.doc.is_some() {
        p.box_(Rect::new(cx, top + tab_h, cw, ruler as u32), s.panel, 0);
        p.box_(
            Rect::new(
                cx,
                top + tab_h,
                ruler as u32,
                body_h.saturating_sub(tab_h as u32),
            ),
            s.panel,
            0,
        );
        // Ruler ticks every 100 image pixels.
        let z = st.effective_zoom(area.width, area.height).max(1) as i32;
        let (ox, oy) = st.origin(area.width, area.height);
        let step = (100 * z / 100).max(8);
        let mut k = 0;
        while ox + k * step < area.width as i32 {
            let tx = area.x + ox + k * step;
            if tx >= area.x {
                p.vline(tx, top + tab_h + 8, 10, s.muted);
                p.left(
                    tx + 2,
                    top + tab_h + 1,
                    40,
                    &(k * 100).to_string(),
                    9,
                    s.muted,
                );
            }
            k += 1;
        }
        let mut k = 0;
        while oy + k * step < area.height as i32 {
            let ty = area.y + oy + k * step;
            if ty >= area.y {
                p.hline(cx + 8, ty, 10, s.muted);
            }
            k += 1;
        }
        view::canvas(p, &s, area, st, false);
        view::scrollbars(p, &s, area, st);
    } else {
        let empty = Rect::new(cx, top, cw, body_h);
        p.box_(empty, s.backdrop, 0);
        p.symbol(
            "image",
            cx + cw as i32 / 2 - 40,
            top + body_h as i32 / 2 - 70,
            80,
            Color(255, 255, 255, 40),
        );
        p.center(
            cx,
            top + body_h as i32 / 2 + 20,
            cw,
            "No image open",
            14,
            s.muted,
        );
        view::button(
            p,
            &s,
            Rect::new(
                cx + cw as i32 / 2 - 110,
                top + body_h as i32 / 2 + 48,
                104,
                28,
            ),
            "New…",
            &t("dialog:new-image"),
            false,
        );
        view::button(
            p,
            &s,
            Rect::new(
                cx + cw as i32 / 2 + 6,
                top + body_h as i32 / 2 + 48,
                104,
                28,
            ),
            "Open…",
            &t("open"),
            true,
        );
    }

    // Right dock: Layers or Undo History.
    let rx = w as i32 - right as i32;
    p.box_(Rect::new(rx, top, right, body_h), s.panel, 0);
    p.vline(rx, top, body_h, s.line);
    let undo_tab = st.tab == "undo";
    let mut tx = rx + 4;
    for (id, label, live) in [
        ("layers", "Layers", true),
        ("channels", "Channels", false),
        ("paths", "Paths", false),
        ("undo", "Undo", true),
    ] {
        let bw = p.measure(label, 11, false) + 14;
        let r = Rect::new(tx, top + 4, bw, 22);
        let on = if id == "undo" {
            undo_tab
        } else {
            id == "layers" && !undo_tab
        };
        if live {
            view::chip(p, &s, r, label, &t(&format!("tab:{id}")), on);
        } else {
            p.box_(r, Color::TRANSPARENT, 2);
            p.disabled("Channels and paths are not edited in this GIMP");
            p.label(
                r.x,
                r.y + 4,
                r.width,
                label,
                11,
                s.muted,
                false,
                Align::Center,
            );
        }
        tx += bw as i32 + 2;
    }
    let dy = top + 32;
    if let Some(doc) = &st.doc {
        if undo_tab {
            let mut y = dy + 4;
            for (i, step) in std::iter::once("[ Base Image ]")
                .chain(doc.undo_steps())
                .enumerate()
            {
                if y + 22 > top + body_h as i32 {
                    break;
                }
                let current = i == doc.undo_steps().len();
                if current {
                    p.box_(Rect::new(rx + 4, y, right - 8, 22), s.selected, 2);
                }
                p.label(
                    rx + 12,
                    y + 4,
                    right - 24,
                    step,
                    12,
                    s.text,
                    current,
                    Align::Left,
                );
                y += 22;
            }
        } else {
            let mode = doc.active_layer().blend;
            p.label(rx + 8, dy + 6, 44, "Mode", 11, s.muted, false, Align::Left);
            view::chip(
                p,
                &s,
                Rect::new(rx + 50, dy + 2, right - 58, 22),
                &format!("{} ▾", gimp_mode(mode)),
                &t("menu:blend"),
                false,
            );
            view::slider(
                p,
                &s,
                Rect::new(rx + 8, dy + 30, right - 16, 34),
                st,
                "Opacity",
                "layer-opacity",
                true,
            );
            view::layer_rows(
                p,
                &s,
                Rect::new(rx + 4, dy + 72, right - 8, body_h.saturating_sub(140)),
                st,
                40,
            );
            let by = top + body_h as i32 - 34;
            let count = doc.layers().len();
            let active = doc.active();
            let mut bx = rx + 6;
            for (symbol, command, label, live, why) in [
                ("new-tab", "layer:new", "Create a new layer", true, ""),
                (
                    "chevron-up",
                    "layer:up",
                    "Raise this layer",
                    active + 1 < count,
                    "The layer is already at the top",
                ),
                (
                    "chevron-down",
                    "layer:down",
                    "Lower this layer",
                    active > 0,
                    "The layer is already at the bottom",
                ),
                ("copy", "layer:duplicate", "Duplicate this layer", true, ""),
                (
                    "layers",
                    "layer:merge",
                    "Merge this layer with the one below",
                    active > 0,
                    "There is no layer below",
                ),
                (
                    "trash",
                    "layer:delete",
                    "Delete this layer",
                    count > 1,
                    "An image keeps at least one layer",
                ),
            ] {
                let r = Rect::new(bx, by, 30, 28);
                if live {
                    view::tool(p, &s, r, symbol, label, &t(command), false);
                } else {
                    view::inert_tool(p, &s, r, symbol, why);
                }
                bx += 34;
            }
        }
    }

    // Status bar.
    let sy = h as i32 - status_h as i32;
    p.box_(Rect::new(0, sy, w, status_h), s.bar, 0);
    p.hline(0, sy, w, s.line);
    if st.doc.is_some() {
        p.left(8, sy + 5, 30, "px", 11, s.text);
        let z = st.effective_zoom(area.width, area.height);
        view::chip(
            p,
            &s,
            Rect::new(40, sy + 2, 64, 20),
            &format!("{z}% ▾"),
            &t("menu:view"),
            false,
        );
        let msg = st
            .status
            .clone()
            .unwrap_or_else(|| format!("{} ({})", st.document_name(), view::size_text(st)));
        p.left(114, sy + 5, w.saturating_sub(124), &msg, 11, s.text);
    } else {
        view::status(p, &s, 8, sy + 5, w - 16, st);
    }

    menus(st, p, &s);
    view::dialog(p, &s, st, w, h);
    view::chooser(p, &s, st, w, h);
}

fn gimp_mode(mode: cw_raster::BlendMode) -> &'static str {
    match mode {
        cw_raster::BlendMode::Add => "Addition",
        cw_raster::BlendMode::Darken => "Darken only",
        cw_raster::BlendMode::Lighten => "Lighten only",
        other => other.label(),
    }
}

fn menus(st: &Studio, p: &mut Painter, s: &Skin) {
    let Some(Panel::Menu { id }) = &st.panel else {
        return;
    };
    let doc = st.doc.as_ref();
    let open = doc.is_some();
    let has_sel = doc.and_then(|d| d.selection()).is_some();
    let undo = doc
        .and_then(|d| d.undo_label())
        .map(|l| format!("Undo {l}"));
    let redo = doc
        .and_then(|d| d.redo_label())
        .map(|l| format!("Redo {l}"));
    let overwrite = st
        .save_target()
        .map(|p| format!("Overwrite {}", p.rsplit('/').next().unwrap_or("")));
    let no = "No image is open";
    let adj =
        |label: &'static str, id: &str| Item::new(label, &format!("dialog:{id}")).when(open, no);
    let act =
        |label: &'static str, id: &str| Item::new(label, &format!("action:{id}")).when(open, no);
    let items: Vec<Item<'_>> = match id.as_str() {
        "file" => vec![
            Item::new("New…", "dialog:new-image").key("Ctrl+N"),
            Item::new("Open…", "open").key("Ctrl+O"),
            Item::separator(),
            Item {
                label: "Save",
                command: Some("noop".into()),
                why: "GIMP's XCF format is not available here; use Export As",
                shortcut: "Ctrl+S",
                checked: false,
            },
            match &overwrite {
                Some(label) => Item::new(label, "save"),
                None => Item::new("Overwrite", "save")
                    .when(false, "The image did not come from a PNG file"),
            },
            Item::new("Export As…", "save-as")
                .key("Shift+Ctrl+E")
                .when(open, no),
        ],
        "edit" => vec![
            match &undo {
                Some(label) => Item::new(label, "undo").key("Ctrl+Z"),
                None => Item::new("Undo", "undo").when(false, "Nothing to undo"),
            },
            match &redo {
                Some(label) => Item::new(label, "redo").key("Ctrl+Y"),
                None => Item::new("Redo", "redo").when(false, "Nothing to redo"),
            },
            Item::separator(),
            Item::new("Cut", "cut")
                .key("Ctrl+X")
                .when(has_sel, "Nothing is selected"),
            Item::new("Copy", "copy").key("Ctrl+C").when(open, no),
            Item::new("Paste as New Layer", "paste")
                .key("Ctrl+V")
                .when(open, no),
            Item::new("Clear", "delete")
                .key("Delete")
                .when(has_sel, "Nothing is selected"),
        ],
        "select" => vec![
            Item::new("All", "select-all").key("Ctrl+A").when(open, no),
            Item::new("None", "select-none")
                .key("Shift+Ctrl+A")
                .when(has_sel, "Nothing is selected"),
            Item::new("Invert", "select-invert")
                .key("Ctrl+I")
                .when(open, no),
        ],
        "view" => vec![
            Item::new("Zoom In", "zoom:in").key("+").when(open, no),
            Item::new("Zoom Out", "zoom:out").key("-").when(open, no),
            Item::new("Fit Image in Window", "zoom:fit")
                .key("Shift+Ctrl+J")
                .checked(st.zoom == 0),
            Item::new("1:1 (100%)", "zoom:100")
                .key("1")
                .checked(st.zoom == 100),
            Item::new("2:1 (200%)", "zoom:200").checked(st.zoom == 200),
        ],
        "image" => vec![
            Item::new("Flip Horizontally", "flip:h").when(open, no),
            Item::new("Flip Vertically", "flip:v").when(open, no),
            Item::new("Rotate 90° clockwise", "rotate:cw").when(open, no),
            Item::new("Rotate 90° counter-clockwise", "rotate:ccw").when(open, no),
            Item::new("Rotate 180°", "rotate:180").when(open, no),
            adj("Arbitrary Rotation…", "rotate"),
            Item::separator(),
            Item::new("Crop to Selection", "crop-selection").when(has_sel, "Nothing is selected"),
            adj("Scale Image…", "resize"),
            Item::new("Flatten Image", "flatten").when(
                doc.is_some_and(|d| d.layers().len() > 1),
                "The image has one layer",
            ),
        ],
        "layer" => {
            let count = doc.map_or(0, |d| d.layers().len());
            let active = doc.map_or(0, |d| d.active());
            vec![
                Item::new("New Layer…", "layer:new")
                    .key("Shift+Ctrl+N")
                    .when(open, no),
                Item::new("Duplicate Layer", "layer:duplicate").when(open, no),
                Item::new("Merge Down", "layer:merge").when(active > 0, "There is no layer below"),
                Item::new("Delete Layer", "layer:delete")
                    .when(count > 1, "An image keeps at least one layer"),
                Item::separator(),
                Item::new("Raise Layer", "layer:up")
                    .when(active + 1 < count, "The layer is already at the top"),
                Item::new("Lower Layer", "layer:down")
                    .when(active > 0, "The layer is already at the bottom"),
            ]
        }
        "colors" => vec![
            adj("Color Balance…", "color-balance"),
            adj("Color Temperature…", "temperature"),
            adj("Hue-Saturation…", "hue-saturation"),
            adj("Saturation…", "saturation"),
            adj("Exposure…", "exposure"),
            adj("Shadows-Highlights…", "shadows-highlights"),
            adj("Brightness-Contrast…", "brightness-contrast"),
            adj("Levels…", "levels"),
            adj("Curves…", "curves"),
            Item::separator(),
            act("Invert", "invert"),
            act("Desaturate", "grayscale"),
            act("Auto ▸ Stretch Contrast", "auto-levels"),
            Item::separator(),
            adj("Threshold…", "threshold"),
            adj("Posterize…", "posterize"),
        ],
        "tools" => st
            .product
            .tools()
            .iter()
            .map(|tool| {
                Item::new(tool_name(*tool), &format!("tool:{}", tool.id()))
                    .checked(st.tool == *tool)
            })
            .collect(),
        "filters" => vec![
            adj("Blur ▸ Gaussian Blur…", "gaussian-blur"),
            adj("Blur ▸ Median Blur…", "median"),
            adj("Blur ▸ Pixelize…", "pixelate"),
            adj("Enhance ▸ Noise Reduction…", "noise-reduction"),
            adj("Enhance ▸ Sharpen (Unsharp Mask)…", "unsharp-mask"),
            act("Enhance ▸ Sharpen", "sharpen"),
            act("Edge-Detect ▸ Edge…", "edge-detect"),
            act("Distorts ▸ Emboss…", "emboss"),
            adj("Light and Shadow ▸ Vignette…", "vignette"),
        ],
        "blend" => cw_raster::BlendMode::ALL
            .iter()
            .map(|m| {
                Item::new(gimp_mode(*m), &format!("layer:blend:{}", m.id()))
                    .checked(doc.is_some_and(|d| d.active_layer().blend == *m))
            })
            .collect(),
        _ => return,
    };
    let x = match id.as_str() {
        "blend" => p.scene.width as i32 - 240,
        other => {
            let mut x = 6;
            for (mid, label) in MENUS {
                if mid == other {
                    break;
                }
                x += p.measure(label, 12, false) as i32 + 16;
            }
            x
        }
    };
    let y = if id == "blend" { 90 } else { 25 };
    view::menu(p, s, st, x.max(0), y, 260, &items);
}
