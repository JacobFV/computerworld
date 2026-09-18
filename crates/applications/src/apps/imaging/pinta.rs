//! Pinta 2 on GNOME: a libadwaita header bar with file, history, clipboard and crop
//! buttons and the Adjustments / Effects / Image / View menu buttons, the tool options
//! toolbar, the two-column tool palette, image tabs, the Layers and History pads and
//! the palette in the status bar.
use super::view::{self, Item, Skin};
use super::{Panel, Studio, Tool};
use crate::desktop_scene::{shared::Align, Painter};
use cw_raster::draw::ShapeKind;
use cw_raster::mask::SelectMode;
use cw_scene::{Color, Rect};

/// A header-bar button: symbol, command, label, live, and why not.
type Button<'a> = (&'a str, &'a str, &'a str, bool, &'a str);

fn skin(dark: bool) -> Skin {
    if dark {
        Skin {
            bg: Color::rgb(36, 36, 36),
            bar: Color::rgb(48, 48, 48),
            panel: Color::rgb(46, 46, 46),
            text: Color::rgb(255, 255, 255),
            muted: Color::rgb(170, 170, 170),
            accent: Color::rgb(53, 132, 228),
            on_accent: Color::WHITE,
            selected: Color(53, 132, 228, 64),
            line: Color(255, 255, 255, 26),
            backdrop: Color::rgb(30, 30, 30),
            radius: 6,
            font: 12,
        }
    } else {
        Skin {
            bg: Color::rgb(250, 250, 250),
            bar: Color::rgb(235, 235, 235),
            panel: Color::rgb(240, 240, 240),
            text: Color::rgb(46, 52, 54),
            muted: Color::rgb(120, 120, 120),
            accent: Color::rgb(53, 132, 228),
            on_accent: Color::WHITE,
            selected: Color(53, 132, 228, 56),
            line: Color(0, 0, 0, 30),
            backdrop: Color::rgb(222, 222, 222),
            radius: 6,
            font: 12,
        }
    }
}

/// Pinta's default palette, two rows.
const PALETTE: [[u8; 3]; 24] = [
    [255, 255, 255],
    [128, 128, 128],
    [127, 0, 0],
    [127, 51, 0],
    [127, 106, 0],
    [91, 127, 0],
    [38, 127, 0],
    [0, 127, 70],
    [0, 127, 127],
    [0, 74, 127],
    [0, 19, 127],
    [72, 0, 127],
    [0, 0, 0],
    [64, 64, 64],
    [255, 0, 0],
    [255, 106, 0],
    [255, 216, 0],
    [182, 255, 0],
    [76, 255, 0],
    [0, 255, 144],
    [0, 255, 255],
    [0, 148, 255],
    [0, 38, 255],
    [178, 0, 255],
];

/// The palette in tool order: (tool, shape, name, symbol).
fn palette_tools() -> Vec<(Tool, Option<ShapeKind>, &'static str, &'static str)> {
    vec![
        (Tool::Move, None, "Move Selected", "move"),
        (Tool::Zoom, None, "Zoom", "zoom-in"),
        (Tool::Pan, None, "Pan", "hand"),
        (Tool::RectSelect, None, "Rectangle Select", "select-rect"),
        (
            Tool::EllipseSelect,
            None,
            "Ellipse Select",
            "select-ellipse",
        ),
        (Tool::Lasso, None, "Lasso Select", "lasso"),
        (Tool::MagicWand, None, "Magic Wand", "wand"),
        (Tool::Brush, None, "Paintbrush", "brush"),
        (Tool::Pencil, None, "Pencil", "pencil"),
        (Tool::Eraser, None, "Eraser", "eraser"),
        (Tool::Fill, None, "Paint Bucket", "bucket"),
        (Tool::Gradient, None, "Gradient", "contrast"),
        (Tool::Picker, None, "Color Picker", "eyedropper"),
        (Tool::Text, None, "Text", "text-tool"),
        (
            Tool::Shape,
            Some(ShapeKind::Line),
            "Line/Curve",
            "line-tool",
        ),
        (
            Tool::Shape,
            Some(ShapeKind::Rectangle),
            "Rectangle",
            "select-rect",
        ),
        (
            Tool::Shape,
            Some(ShapeKind::RoundedRectangle),
            "Rounded Rectangle",
            "shapes",
        ),
        (
            Tool::Shape,
            Some(ShapeKind::Ellipse),
            "Ellipse",
            "select-ellipse",
        ),
        (
            Tool::Shape,
            Some(ShapeKind::Freeform),
            "Freeform Shape",
            "edit",
        ),
        (Tool::Clone, None, "Clone Stamp", "stamp"),
    ]
}

pub fn render(st: &Studio, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let s = skin(env.settings.dark_mode);
    let (w, h) = (env.width, env.height);
    p.scene.background = s.bg;
    let t = |c: &str| st.target(c);
    let doc = st.doc.as_ref();
    let open = doc.is_some();
    let has_sel = doc.and_then(|d| d.selection()).is_some();

    // Header bar.
    p.box_(Rect::new(0, 0, w, 46), s.bar, 0);
    p.hline(0, 46, w, s.line);
    let mut x = 8;
    let groups: [&[Button<'_>]; 4] = [
        &[
            ("new-tab", "new", "New Image", true, ""),
            ("folder", "open", "Open", true, ""),
            ("download", "save", "Save", open, "No image is open"),
        ],
        &[
            (
                "undo",
                "undo",
                "Undo",
                doc.is_some_and(|d| d.can_undo()),
                "Nothing to undo",
            ),
            (
                "redo",
                "redo",
                "Redo",
                doc.is_some_and(|d| d.can_redo()),
                "Nothing to redo",
            ),
        ],
        &[
            ("scissors", "cut", "Cut", has_sel, "Nothing is selected"),
            ("copy", "copy", "Copy", open, "No image is open"),
            ("paste", "paste", "Paste", open, "No image is open"),
        ],
        &[
            (
                "crop",
                "crop-selection",
                "Crop to Selection",
                has_sel,
                "Nothing is selected",
            ),
            (
                "close",
                "select-none",
                "Deselect All",
                has_sel,
                "Nothing is selected",
            ),
        ],
    ];
    for group in groups {
        for (symbol, command, label, live, why) in group {
            let r = Rect::new(x, 8, 32, 30);
            if *live {
                view::tool(p, &s, r, symbol, label, &t(command), false);
            } else {
                view::inert_tool(p, &s, r, symbol, why);
            }
            x += 34;
        }
        x += 10;
    }
    let mut rx = w as i32 - 8;
    for (symbol, id, label) in [
        ("menu", "main", "Main Menu"),
        ("filters", "effects", "Effects"),
        ("sliders", "adjustments", "Adjustments"),
        ("image", "image", "Image"),
        ("search", "view", "View"),
    ] {
        rx -= 38;
        let on = matches!(&st.panel, Some(Panel::Menu { id: m }) if m == id);
        view::tool(
            p,
            &s,
            Rect::new(rx, 8, 34, 30),
            symbol,
            label,
            &t(&format!("menu:{id}")),
            on,
        );
    }

    // Tool options toolbar.
    let ty = 47;
    p.box_(Rect::new(0, ty, w, 40), s.bg, 0);
    p.hline(0, ty + 40, w, s.line);
    let mut ox = 10;
    let name = palette_tools()
        .into_iter()
        .find(|(tool, shape, _, _)| *tool == st.tool && shape.is_none_or(|k| k == st.shape))
        .map(|(_, _, n, _)| n)
        .unwrap_or("");
    p.strong(ox, ty + 12, 130, name, 12, s.text);
    ox += 136;
    if st.tool == Tool::Gradient {
        use cw_raster::gradient::GradientShape as G;
        for (label, shape) in [
            ("Linear", G::Linear),
            ("Linear Reflected", G::BiLinear),
            ("Linear Diamond", G::Diamond),
            ("Radial", G::Radial),
            ("Conical", G::ConicalAsymmetric),
        ] {
            let bw = p.measure(label, 11, false) + 18;
            view::chip(
                p,
                &s,
                Rect::new(ox, ty + 8, bw, 24),
                label,
                &t(&format!("gradient-shape:{}", shape.id())),
                st.gradient.shape == shape,
            );
            ox += bw as i32 + 4;
        }
    } else if st.tool.paints() || st.tool == Tool::Shape || st.tool == Tool::Clone {
        p.label(
            ox,
            ty + 12,
            90,
            "Brush width:",
            12,
            s.text,
            false,
            Align::Left,
        );
        view::slider(
            p,
            &s,
            Rect::new(ox + 86, ty + 6, 140, 28),
            st,
            "",
            "size",
            false,
        );
        p.label(
            ox + 232,
            ty + 12,
            40,
            &st.size.to_string(),
            12,
            s.muted,
            false,
            Align::Left,
        );
        ox += 276;
        view::chip(
            p,
            &s,
            Rect::new(ox, ty + 8, 104, 24),
            "Antialiasing",
            &t("antialias"),
            st.antialias,
        );
        ox += 112;
        let hint = match (st.tool, &st.curve) {
            (Tool::Clone, _) if st.retouch.source.is_none() => {
                Some("Ctrl-click to set the origin, then paint")
            }
            (Tool::Clone, _) => Some("Ctrl-click to set a new origin"),
            (Tool::Shape, Some(_)) => {
                Some("Drag a point, click the line to add one; Enter to finalize")
            }
            _ => None,
        };
        if let Some(hint) = hint {
            p.label(ox, ty + 12, 360, hint, 11, s.muted, false, Align::Left);
        }
        if st.tool == Tool::Shape && st.curve.is_none() {
            for (label, outline, fill) in [
                ("Outline", true, false),
                ("Fill", false, true),
                ("Outline + Fill", true, true),
            ] {
                let bw = p.measure(label, 11, false) + 18;
                let on = st.outline == outline && st.fill == fill;
                // Pinta's fill style is one choice covering both halves.
                view::chip(
                    p,
                    &s,
                    Rect::new(ox, ty + 8, bw, 24),
                    label,
                    &t(&format!(
                        "fill-style:{}",
                        if outline && fill {
                            "both"
                        } else if fill {
                            "fill"
                        } else {
                            "outline"
                        }
                    )),
                    on,
                );
                ox += bw as i32 + 4;
            }
        }
    } else if matches!(
        st.tool,
        Tool::RectSelect | Tool::EllipseSelect | Tool::Lasso | Tool::MagicWand
    ) {
        p.label(
            ox,
            ty + 12,
            110,
            "Selection mode:",
            12,
            s.text,
            false,
            Align::Left,
        );
        ox += 110;
        for mode in SelectMode::ALL {
            let label = match mode {
                SelectMode::Replace => "Replace",
                SelectMode::Add => "Union",
                SelectMode::Subtract => "Exclude",
                SelectMode::Intersect => "Intersect",
            };
            let bw = p.measure(label, 11, false) + 18;
            view::chip(
                p,
                &s,
                Rect::new(ox, ty + 8, bw, 24),
                label,
                &t(&format!("mode:{}", mode.id())),
                st.select_mode == mode,
            );
            ox += bw as i32 + 4;
        }
        if st.tool == Tool::MagicWand {
            p.label(
                ox + 12,
                ty + 12,
                70,
                "Tolerance:",
                12,
                s.text,
                false,
                Align::Left,
            );
            view::slider(
                p,
                &s,
                Rect::new(ox + 84, ty + 6, 120, 28),
                st,
                "",
                "tolerance",
                false,
            );
        }
    } else if st.tool == Tool::Fill {
        p.label(
            ox,
            ty + 12,
            70,
            "Tolerance:",
            12,
            s.text,
            false,
            Align::Left,
        );
        view::slider(
            p,
            &s,
            Rect::new(ox + 72, ty + 6, 140, 28),
            st,
            "",
            "tolerance",
            false,
        );
        view::chip(
            p,
            &s,
            Rect::new(ox + 224, ty + 8, 110, 24),
            "Sample: Image",
            &t("merged"),
            st.merged,
        );
    } else if st.tool == Tool::Text {
        p.label(ox, ty + 12, 40, "Size:", 12, s.text, false, Align::Left);
        view::slider(
            p,
            &s,
            Rect::new(ox + 40, ty + 6, 140, 28),
            st,
            "",
            "font-size",
            false,
        );
        p.label(
            ox + 186,
            ty + 12,
            40,
            &st.font_size.to_string(),
            12,
            s.muted,
            false,
            Align::Left,
        );
        view::chip(
            p,
            &s,
            Rect::new(ox + 226, ty + 8, 60, 24),
            "Bold",
            &t("bold"),
            st.bold,
        );
    }

    // Tool palette.
    let top = ty + 41;
    let status_h = 58;
    let body_h = h.saturating_sub(top as u32 + status_h);
    p.box_(Rect::new(0, top, 76, body_h), s.panel, 0);
    p.vline(76, top, body_h, s.line);
    for (i, (tool, shape, label, symbol)) in palette_tools().into_iter().enumerate() {
        let r = Rect::new(
            6 + (i as i32 % 2) * 34,
            top + 6 + (i as i32 / 2) * 34,
            32,
            32,
        );
        let on = st.tool == tool && shape.is_none_or(|k| k == st.shape);
        let command = match shape {
            Some(k) => format!("shape:{}", k.id()),
            None => format!("tool:{}", tool.id()),
        };
        view::tool(p, &s, r, symbol, label, &t(&command), on);
    }

    // Image tabs and canvas.
    let dock = 220u32.min(w / 4);
    let cx = 77;
    let cw = w.saturating_sub(77 + dock);
    p.box_(Rect::new(cx, top, cw, 34), s.bar, 0);
    if open {
        let tab = Rect::new(cx + 6, top + 4, 180.min(cw.saturating_sub(12)), 26);
        p.box_(tab, s.bg, 6);
        p.label(
            tab.x + 10,
            tab.y + 5,
            tab.width - 20,
            &format!(
                "{}{}",
                st.document_name(),
                if st.modified { "*" } else { "" }
            ),
            12,
            s.text,
            false,
            Align::Left,
        );
    }
    let area = Rect::new(cx, top + 34, cw, body_h.saturating_sub(34));
    if open {
        view::canvas(p, &s, area, st, true, env.pointer);
        view::scrollbars(p, &s, area, st);
    } else {
        p.box_(area, s.backdrop, 0);
        p.center(cx, area.y + 60, cw, "No image is open", 14, s.muted);
    }

    // Layers and History pads.
    let dx = w as i32 - dock as i32;
    p.box_(Rect::new(dx, top, dock, body_h), s.panel, 0);
    p.vline(dx, top, body_h, s.line);
    p.strong(dx + 10, top + 8, dock - 20, "Layers", 13, s.text);
    let half = body_h / 2;
    view::layer_rows(
        p,
        &s,
        Rect::new(dx + 4, top + 32, dock - 8, half.saturating_sub(76)),
        st,
        40,
    );
    let by = top + half as i32 - 40;
    if let Some(doc) = doc {
        let count = doc.layers().len();
        let active = doc.active();
        let mut bx = dx + 6;
        for (symbol, command, label, live, why) in [
            ("plus", "layer:new", "Add New Layer", true, ""),
            (
                "trash",
                "layer:delete",
                "Delete Layer",
                count > 1,
                "An image keeps at least one layer",
            ),
            ("copy", "layer:duplicate", "Duplicate Layer", true, ""),
            (
                "chevron-up",
                "layer:up",
                "Move Layer Up",
                active + 1 < count,
                "The layer is already at the top",
            ),
            (
                "chevron-down",
                "layer:down",
                "Move Layer Down",
                active > 0,
                "The layer is already at the bottom",
            ),
            (
                "layers",
                "layer:merge",
                "Merge Layer Down",
                active > 0,
                "There is no layer below",
            ),
        ] {
            let r = Rect::new(bx, by, 32, 30);
            if live {
                view::tool(p, &s, r, symbol, label, &t(command), false);
            } else {
                view::inert_tool(p, &s, r, symbol, why);
            }
            bx += 34;
        }
    }
    p.hline(dx, top + half as i32, dock, s.line);
    p.strong(
        dx + 10,
        top + half as i32 + 8,
        dock - 20,
        "History",
        13,
        s.text,
    );
    if let Some(doc) = doc {
        let steps = doc.undo_steps();
        let mut y = top + half as i32 + 32;
        for (i, step) in std::iter::once("Open Image")
            .chain(steps.iter().copied())
            .enumerate()
        {
            if y + 22 > top + body_h as i32 {
                break;
            }
            let current = i == steps.len();
            if current {
                p.box_(Rect::new(dx + 4, y, dock - 8, 22), s.selected, 4);
            }
            p.label(
                dx + 12,
                y + 4,
                dock - 24,
                step,
                12,
                s.text,
                current,
                Align::Left,
            );
            y += 22;
        }
    }

    // Status bar: palette, selection size, zoom.
    let sy = h as i32 - status_h as i32;
    p.box_(Rect::new(0, sy, w, status_h), s.bar, 0);
    p.hline(0, sy, w, s.line);
    view::swatch(
        p,
        &s,
        Rect::new(22, sy + 20, 26, 26),
        st.secondary,
        &t("slot:2"),
        st.slot == 1,
        false,
    );
    p.z += 1;
    view::swatch(
        p,
        &s,
        Rect::new(10, sy + 8, 26, 26),
        st.primary,
        &t("slot:1"),
        st.slot == 0,
        false,
    );
    p.z -= 1;
    view::tool(
        p,
        &s,
        Rect::new(52, sy + 6, 18, 18),
        "sort",
        "Swap colors",
        &t("swap-colors"),
        false,
    );
    view::tool(
        p,
        &s,
        Rect::new(52, sy + 30, 18, 18),
        "reload",
        "Reset colors",
        &t("reset-colors"),
        false,
    );
    for (i, c) in PALETTE.iter().enumerate() {
        let r = Rect::new(
            80 + (i as i32 % 12) * 22,
            sy + 8 + (i as i32 / 12) * 22,
            20,
            20,
        );
        let color = [c[0], c[1], c[2], 255];
        view::swatch(
            p,
            &s,
            r,
            color,
            &t(&format!("color:{}", cw_raster::hex(color))),
            false,
            false,
        );
    }
    view::button(
        p,
        &s,
        Rect::new(80 + 12 * 22 + 6, sy + 14, 70, 28),
        "More…",
        &t("dialog:color"),
        false,
    );
    let mut sx = 80 + 12 * 22 + 90;
    // Cursor position, then selection size, as Pinta's status bar reads.
    p.symbol("move", sx, sy + 20, 16, s.muted);
    if let Some((x, y)) = st.pointer_pixel(area, env.pointer) {
        p.left(sx + 20, sy + 20, 100, &format!("{x}, {y}"), 12, s.text);
    }
    sx += 110;
    if let Some(sel) = view::selection_text(st) {
        p.symbol("select-rect", sx, sy + 20, 16, s.muted);
        p.left(sx + 20, sy + 20, 120, &sel, 12, s.text);
        sx += 140;
    }
    view::status(p, &s, sx, sy + 20, (w as i32 - sx - 140).max(0) as u32, st);
    let z = st.effective_zoom(area.width, area.height);
    view::chip(
        p,
        &s,
        Rect::new(w as i32 - 120, sy + 16, 110, 26),
        &format!("{z}% ▾"),
        &t("menu:view"),
        false,
    );

    menus(st, p, &s);
    view::dialog(p, &s, st, w, h);
    view::chooser(p, &s, st, w, h);
}

fn menus(st: &Studio, p: &mut Painter, s: &Skin) {
    let Some(Panel::Menu { id }) = &st.panel else {
        return;
    };
    let doc = st.doc.as_ref();
    let open = doc.is_some();
    let no = "No image is open";
    let has_sel = doc.and_then(|d| d.selection()).is_some();
    let adj =
        |label: &'static str, id: &str| Item::new(label, &format!("dialog:{id}")).when(open, no);
    let act =
        |label: &'static str, id: &str| Item::new(label, &format!("action:{id}")).when(open, no);
    let items: Vec<Item<'_>> = match id.as_str() {
        "adjustments" => vec![
            act("Auto Level", "auto-levels"),
            act("Black and White", "grayscale"),
            adj("Brightness / Contrast", "brightness-contrast"),
            adj("Curves", "curves"),
            adj("Hue / Saturation", "hue-saturation"),
            act("Invert Colors", "invert"),
            adj("Levels", "levels"),
            adj("Posterize", "posterize"),
            act("Sepia", "sepia"),
        ],
        "effects" => vec![
            adj("Blurs ▸ Gaussian Blur", "gaussian-blur"),
            adj("Distort ▸ Pixelate", "pixelate"),
            adj("Noise ▸ Median", "median"),
            adj("Photo ▸ Sharpen", "sharpen"),
            adj("Photo ▸ Vignette", "vignette"),
            act("Stylize ▸ Edge Detect", "edge-detect"),
            act("Stylize ▸ Emboss", "emboss"),
        ],
        "image" => vec![
            Item::new("Crop to Selection", "crop-selection").when(has_sel, "Nothing is selected"),
            adj("Resize Image…", "resize"),
            Item::separator(),
            Item::new("Flip Horizontal", "flip:h").when(open, no),
            Item::new("Flip Vertical", "flip:v").when(open, no),
            Item::new("Rotate 90° Clockwise", "rotate:cw").when(open, no),
            Item::new("Rotate 90° Counter-Clockwise", "rotate:ccw").when(open, no),
            Item::new("Rotate 180°", "rotate:180").when(open, no),
            adj("Rotate…", "rotate"),
            Item::separator(),
            Item::new("Flatten", "flatten").when(
                doc.is_some_and(|d| d.layers().len() > 1),
                "The image has one layer",
            ),
        ],
        "view" => vec![
            Item::new("Zoom In", "zoom:in").key("Ctrl++").when(open, no),
            Item::new("Zoom Out", "zoom:out")
                .key("Ctrl+-")
                .when(open, no),
            Item::new("Best Fit", "zoom:fit")
                .key("Ctrl+B")
                .checked(st.zoom == 0),
            Item::new("Actual Size", "zoom:100")
                .key("Ctrl+0")
                .checked(st.zoom == 100),
        ],
        "main" => vec![
            Item::new("New…", "new").key("Ctrl+N"),
            Item::new("Open…", "open").key("Ctrl+O"),
            Item::new("Save", "save").key("Ctrl+S").when(open, no),
            Item::new("Save As…", "save-as")
                .key("Ctrl+Shift+S")
                .when(open, no),
            Item::separator(),
            Item::new("Select All", "select-all")
                .key("Ctrl+A")
                .when(open, no),
            Item::new("Invert Selection", "select-invert")
                .key("Ctrl+I")
                .when(open, no),
            Item::new("Erase Selection", "delete")
                .key("Delete")
                .when(has_sel, "Nothing is selected"),
        ],
        _ => return,
    };
    let x = (p.scene.width as i32 - 270).max(0);
    view::menu(p, s, st, x, 44, 260, &items);
}
