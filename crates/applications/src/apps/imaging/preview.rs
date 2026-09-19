//! Preview on macOS: the unified toolbar with zoom, Markup and Rotate, the Markup
//! toolbar (Selection, Sketch, Shapes, Text, Sign, Adjust Color, Adjust Size, Shape
//! Style, Border and Fill colour, Text Style, Crop), the Adjust Color panel with its
//! histogram, and the Image Dimensions sheet. Save and Export come from the menu bar
//! (File ▸ Save) and ⌘S, as on a Mac.
use super::view::{self, Item, Skin};
use super::{dialog_params, Panel, Studio, Tool};
use crate::desktop_scene::{shared::Align, Painter};
use cw_raster::draw::ShapeKind;
use cw_scene::{Color, Rect};

fn skin(dark: bool) -> Skin {
    if dark {
        Skin {
            bg: Color::rgb(30, 30, 30),
            bar: Color::rgb(44, 44, 46),
            panel: Color::rgb(50, 50, 52),
            text: Color::rgb(236, 236, 236),
            muted: Color::rgb(152, 152, 157),
            accent: Color::rgb(10, 132, 255),
            on_accent: Color::WHITE,
            selected: Color(255, 255, 255, 30),
            line: Color(255, 255, 255, 30),
            backdrop: Color::rgb(30, 30, 30),
            radius: 6,
            font: 12,
        }
    } else {
        Skin {
            bg: Color::rgb(236, 236, 236),
            bar: Color::rgb(246, 246, 247),
            panel: Color::rgb(250, 250, 250),
            text: Color::rgb(29, 29, 31),
            muted: Color::rgb(110, 110, 115),
            accent: Color::rgb(0, 122, 255),
            on_accent: Color::WHITE,
            selected: Color(0, 0, 0, 24),
            line: Color(0, 0, 0, 30),
            backdrop: Color::rgb(236, 236, 236),
            radius: 6,
            font: 12,
        }
    }
}

/// Markup's colour popover: the system colours.
const COLORS: [[u8; 3]; 12] = [
    [255, 59, 48],
    [255, 149, 0],
    [255, 204, 0],
    [52, 199, 89],
    [0, 199, 190],
    [0, 122, 255],
    [88, 86, 214],
    [175, 82, 222],
    [255, 45, 85],
    [0, 0, 0],
    [142, 142, 147],
    [255, 255, 255],
];

pub fn render(st: &Studio, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let s = skin(env.settings.dark_mode);
    let (w, h) = (env.width, env.height);
    p.scene.background = s.bg;
    let t = |c: &str| st.target(c);
    let doc = st.doc.as_ref();
    let open = doc.is_some();
    let markup = st.tab == "markup";

    // Unified toolbar: the document, then zoom, Markup and Rotate on the right.
    p.box_(Rect::new(0, 0, w, 44), s.bar, 0);
    p.hline(0, 44, w, s.line);
    p.strong(
        14,
        7,
        w.saturating_sub(260),
        &st.document_name(),
        13,
        s.text,
    );
    if let Some(d) = doc {
        p.left(
            14,
            25,
            200,
            &format!("{} × {}", d.width(), d.height()),
            11,
            s.muted,
        );
    }
    let mut x = w as i32 - 12;
    let area_w = w;
    let area_h = h.saturating_sub(if markup { 84 } else { 45 });
    for (symbol, label, command, on) in [
        ("rotate-ccw", "Rotate Left", "rotate:ccw", false),
        ("marker", "Markup", "tab:markup-toggle", markup),
        ("zoom-in", "Zoom In", "zoom:in", false),
        ("zoom-out", "Zoom Out", "zoom:out", false),
    ] {
        x -= 36;
        let r = Rect::new(x, 8, 32, 28);
        if open {
            let command = if command.starts_with("zoom") {
                format!("{command}:{area_w}:{area_h}")
            } else {
                command.to_owned()
            };
            view::tool(p, &s, r, symbol, label, &t(&command), on);
        } else {
            view::inert_tool(p, &s, r, symbol, "No image is open");
        }
    }

    let mut top = 45;
    if markup && open {
        p.box_(Rect::new(0, top, w, 38), s.bar, 0);
        p.hline(0, top + 38, w, s.line);
        let mut x = 10;
        let tool_button =
            |p: &mut Painter, symbol: &str, label: &str, command: &str, on: bool, x: &mut i32| {
                view::tool(
                    p,
                    &s,
                    Rect::new(*x, top + 5, 30, 28),
                    symbol,
                    label,
                    &t(command),
                    on,
                );
                *x += 32;
            };
        let selecting = matches!(
            st.tool,
            Tool::RectSelect | Tool::EllipseSelect | Tool::Lasso
        );
        let sel_symbol = match st.tool {
            Tool::EllipseSelect => "select-ellipse",
            Tool::Lasso => "lasso",
            _ => "select-rect",
        };
        tool_button(
            p,
            sel_symbol,
            "Selection Tools",
            "menu:selection",
            selecting,
            &mut x,
        );
        tool_button(
            p,
            "pencil",
            "Sketch",
            "tool:pen",
            st.tool == Tool::Pen,
            &mut x,
        );
        tool_button(
            p,
            "shapes",
            "Shapes",
            "menu:shapes",
            matches!(st.tool, Tool::Shape | Tool::Highlighter),
            &mut x,
        );
        tool_button(
            p,
            "text-tool",
            "Text",
            "tool:text",
            st.tool == Tool::Text,
            &mut x,
        );
        let sign = Rect::new(x, top + 5, 30, 28);
        view::inert_tool(p, &s, sign, "edit", "No signature has been created");
        x += 38;
        p.vline(x - 4, top + 9, 20, s.line);
        tool_button(
            p,
            "sliders",
            "Adjust Color",
            "dialog:adjust-color",
            matches!(&st.panel, Some(Panel::Dialog { id, .. }) if id == "adjust-color"),
            &mut x,
        );
        tool_button(p, "resize", "Adjust Size", "dialog:resize", false, &mut x);
        x += 6;
        p.vline(x - 4, top + 9, 20, s.line);
        tool_button(p, "line-tool", "Shape Style", "menu:style", false, &mut x);
        // Border and fill colour wells.
        for (label, command, color, on) in [
            ("Border Color", "menu:border", st.primary, true),
            ("Fill Color", "menu:fill-color", st.secondary, st.fill),
        ] {
            let r = Rect::new(x, top + 5, 38, 28);
            p.button(r, Color::TRANSPARENT, 5, &t(command), label);
            let well = Rect::new(x + 5, top + 11, 16, 16);
            if on {
                p.box_(well, view::rgba(color), 3);
            } else {
                p.border(well, Color::WHITE, 3, s.line);
                p.line(
                    vec![(well.x + 2, well.y + 14), (well.x + 14, well.y + 2)],
                    Color::rgb(255, 59, 48),
                    2,
                );
            }
            p.border(well, Color::TRANSPARENT, 3, s.line);
            p.symbol("chevron-down", x + 24, top + 13, 10, s.muted);
            x += 40;
        }
        tool_button(
            p,
            "text-tool",
            "Text Style",
            "menu:text-style",
            false,
            &mut x,
        );
        x += 6;
        let crop = Rect::new(x, top + 5, 30, 28);
        if doc.and_then(|d| d.selection()).is_some() {
            view::tool(p, &s, crop, "crop", "Crop", &t("crop-selection"), false);
        } else {
            view::inert_tool(p, &s, crop, "crop", "Select part of the image to crop to");
        }
        top += 39;
    }

    let area = Rect::new(0, top, w, h.saturating_sub(top as u32));
    if open {
        view::canvas(p, &s, area, st, false, env.pointer);
        view::scrollbars(p, &s, area, st);
    } else {
        p.box_(area, s.backdrop, 0);
        match &st.loading {
            Some(path) => p.center(0, area.y + 60, w, &format!("Opening {path}…"), 13, s.muted),
            None => {
                p.center(0, area.y + 60, w, "No image is open", 14, s.muted);
                view::button(
                    p,
                    &s,
                    Rect::new(w as i32 / 2 - 50, area.y + 90, 100, 28),
                    "Open…",
                    &t("open"),
                    true,
                );
            }
        }
    }
    if let Some(msg) = &st.status {
        let bw = p.measure(msg, 12, false) + 28;
        let r = Rect::new((w as i32 - bw as i32) / 2, h as i32 - 44, bw, 30);
        p.box_(r, Color(0, 0, 0, 150), 15);
        p.label(
            r.x,
            r.y + 7,
            r.width,
            msg,
            12,
            Color::WHITE,
            false,
            Align::Center,
        );
    }

    menus(st, p, &s, top);
    adjust_panel(st, p, &s, w, h, top);
    if !matches!(&st.panel, Some(Panel::Dialog { id, .. }) if id == "adjust-color") {
        view::dialog(p, &s, st, w, h);
    }
    view::chooser(p, &s, st, w, h);
}

/// The Adjust Color panel: a floating panel beside the image rather than a modal
/// sheet, with the histogram, Auto Levels, the sliders and Reset All.
fn adjust_panel(st: &Studio, p: &mut Painter, s: &Skin, w: u32, h: u32, top: i32) {
    let Some(Panel::Dialog { id, .. }) = &st.panel else {
        return;
    };
    if id != "adjust-color" {
        return;
    }
    let pw = 270u32.min(w.saturating_sub(20));
    let params = dialog_params(id);
    let ph = (190 + params.len() as u32 * 40).min(h.saturating_sub(top as u32 + 20));
    let r = Rect::new(w as i32 - pw as i32 - 12, top + 10, pw, ph);
    let z = p.z;
    p.z += 400;
    p.drop_shadow(r, 10, 14, 60, 4);
    p.border(r, s.panel, 10, s.line);
    p.region(r, &st.target("noop"), "Adjust Color panel");
    view::tool(
        p,
        s,
        Rect::new(r.x + 6, r.y + 6, 20, 20),
        "close",
        "Close and keep the adjustments",
        &st.target("apply"),
        false,
    );
    p.strong_center(r.x, r.y + 8, pw, "Adjust Color", 12, s.text);
    // Histogram of the image as it is now.
    let g = Rect::new(r.x + 14, r.y + 34, pw - 28, 70);
    p.box_(g, Color(0, 0, 0, 200), 4);
    if let Some(doc) = &st.doc {
        let mut bins = [[0u32; 64]; 3];
        let (dw, dh) = (doc.width() as i32, doc.height() as i32);
        let step = ((dw * dh / 20_000) as f32).sqrt().max(1.0) as i32;
        for y in (0..dh).step_by(step as usize) {
            for x in (0..dw).step_by(step as usize) {
                let px = doc.pixel(x, y);
                for c in 0..3 {
                    bins[c][(px[c] >> 2) as usize] += 1;
                }
            }
        }
        let peak = bins
            .iter()
            .flat_map(|b| b.iter())
            .copied()
            .max()
            .unwrap_or(1)
            .max(1);
        for (c, color) in [
            (0, Color(255, 70, 70, 150)),
            (1, Color(70, 220, 70, 150)),
            (2, Color(80, 120, 255, 150)),
        ] {
            let pts: Vec<(i32, i32)> = (0..64)
                .map(|i| {
                    (
                        g.x + (i * (g.width as i32 - 1) / 63),
                        g.y + g.height as i32
                            - 2
                            - (bins[c][i as usize] * (g.height - 4) / peak) as i32,
                    )
                })
                .collect();
            p.line(pts, color, 1);
        }
    }
    view::button(
        p,
        s,
        Rect::new(r.x + 14, r.y + 110, 100, 24),
        "Auto Levels",
        &st.target("action:auto-levels"),
        false,
    );
    let mut y = r.y + 142;
    for (param, _, _, _) in params {
        if y + 36 > r.y + ph as i32 - 36 {
            break;
        }
        view::slider(
            p,
            s,
            Rect::new(r.x + 14, y, pw - 28, 34),
            st,
            view::param_label(param),
            param,
            false,
        );
        y += 40;
    }
    view::button(
        p,
        s,
        Rect::new(r.x + pw as i32 / 2 - 50, r.y + ph as i32 - 34, 100, 24),
        "Reset All",
        &st.target("reset"),
        false,
    );
    p.z = z;
}

fn menus(st: &Studio, p: &mut Painter, s: &Skin, top: i32) {
    let Some(Panel::Menu { id }) = &st.panel else {
        return;
    };
    let x0 = 10;
    let y = top - 2;
    match id.as_str() {
        "selection" => {
            let items = vec![
                Item::new("Rectangular Selection", "tool:select-rect")
                    .checked(st.tool == Tool::RectSelect),
                Item::new("Elliptical Selection", "tool:select-ellipse")
                    .checked(st.tool == Tool::EllipseSelect),
                Item::new("Lasso Selection", "tool:lasso").checked(st.tool == Tool::Lasso),
            ];
            view::menu(p, s, st, x0, y, 220, &items);
        }
        "shapes" => {
            let shapes = [
                ("Line", ShapeKind::Line),
                ("Arrow", ShapeKind::Arrow),
                ("Rectangle", ShapeKind::Rectangle),
                ("Rounded Rectangle", ShapeKind::RoundedRectangle),
                ("Oval", ShapeKind::Ellipse),
                ("Star", ShapeKind::Star),
                ("Polygon", ShapeKind::Hexagon),
            ];
            let mut items: Vec<Item<'_>> = shapes
                .iter()
                .map(|(label, k)| {
                    Item::new(label, &format!("shape:{}", k.id()))
                        .checked(st.tool == Tool::Shape && st.shape == *k)
                })
                .collect();
            items.push(Item::separator());
            items.push(
                Item::new("Highlight", "tool:highlighter").checked(st.tool == Tool::Highlighter),
            );
            view::menu(p, s, st, x0 + 64, y, 210, &items);
        }
        "style" => {
            let items: Vec<Item<'_>> = [
                (1u32, "1 pt"),
                (2, "2 pt"),
                (3, "3 pt"),
                (5, "5 pt"),
                (8, "8 pt"),
                (12, "12 pt"),
            ]
            .iter()
            .map(|(n, label)| Item::new(label, &format!("set:size:{n}")).checked(st.size == *n))
            .collect();
            view::menu(p, s, st, x0 + 250, y, 150, &items);
        }
        "text-style" => {
            let mut items: Vec<Item<'_>> = [
                (12u16, "12"),
                (18, "18"),
                (24, "24"),
                (36, "36"),
                (48, "48"),
                (72, "72"),
            ]
            .iter()
            .map(|(n, label)| {
                Item::new(label, &format!("set:font-size:{n}")).checked(st.font_size == *n)
            })
            .collect();
            items.push(Item::separator());
            items.push(Item::new("Bold", "bold").checked(st.bold));
            view::menu(p, s, st, x0 + 360, y, 150, &items);
        }
        "border" | "fill-color" => {
            let fill = id == "fill-color";
            let r = Rect::new(
                x0 + if fill { 330 } else { 290 },
                y,
                176,
                if fill { 132 } else { 104 },
            );
            let z = p.z;
            p.z += 500;
            p.drop_shadow(r, 8, 10, 60, 3);
            p.border(r, s.panel, 8, s.line);
            p.region(r, &st.target("noop"), "Colors");
            for (i, c) in COLORS.iter().enumerate() {
                let color = [c[0], c[1], c[2], 255];
                let cell = Rect::new(
                    r.x + 12 + (i as i32 % 6) * 26,
                    r.y + 12 + (i as i32 / 6) * 30,
                    22,
                    22,
                );
                let command = if fill {
                    format!("bg:{}", cw_raster::hex(color))
                } else {
                    format!("fg:{}", cw_raster::hex(color))
                };
                let current = if fill { st.secondary } else { st.primary };
                view::swatch(
                    p,
                    s,
                    cell,
                    color,
                    &st.target(&command),
                    current == color && (!fill || st.fill),
                    true,
                );
            }
            if fill {
                view::chip(
                    p,
                    s,
                    Rect::new(r.x + 12, r.y + 76, 72, 22),
                    "No Fill",
                    &st.target("fill:none"),
                    !st.fill,
                );
                view::chip(
                    p,
                    s,
                    Rect::new(r.x + 90, r.y + 76, 72, 22),
                    "Fill",
                    &st.target("fill:solid"),
                    st.fill,
                );
            }
            view::button(
                p,
                s,
                Rect::new(r.x + 12, r.y + r.height as i32 - 30, 150, 22),
                "Show Colors…",
                &st.target("dialog:color"),
                false,
            );
            p.z = z;
        }
        _ => {}
    }
}
