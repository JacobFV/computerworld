//! Sketchbook on an Android phone: the condensed top toolbar (main menu, undo, redo,
//! tools, brush library, colour editor, layer editor), a full-bleed canvas, the brush
//! and colour pucks in the corner, and the Layer Editor over the canvas's edge.
use super::view::{self, Item, Skin};
use super::{Panel, Studio, Tool};
use crate::desktop_scene::{shared::Align, Painter};
use cw_scene::{Color, Rect};

fn skin() -> Skin {
    Skin {
        bg: Color::rgb(255, 255, 255),
        bar: Color::rgb(242, 242, 242),
        panel: Color::rgb(250, 250, 250),
        text: Color::rgb(74, 74, 74),
        muted: Color::rgb(140, 140, 140),
        accent: Color::rgb(30, 136, 229),
        on_accent: Color::WHITE,
        selected: Color(30, 136, 229, 40),
        line: Color(0, 0, 0, 28),
        backdrop: Color::rgb(214, 214, 214),
        radius: 8,
        font: 13,
    }
}

/// The brush library, in its Basic set's order.
const BRUSHES: [(&str, Tool, &str); 5] = [
    ("Pencil", Tool::Pencil, "pencil"),
    ("Paint Brush", Tool::Brush, "brush"),
    ("Airbrush", Tool::Airbrush, "brush"),
    ("Marker", Tool::Marker, "marker"),
    ("Eraser", Tool::Eraser, "eraser"),
];
/// The colour editor's quick swatches.
const SWATCHES: [[u8; 3]; 12] = [
    [0, 0, 0],
    [90, 90, 90],
    [255, 255, 255],
    [229, 57, 53],
    [251, 140, 0],
    [253, 216, 53],
    [67, 160, 71],
    [0, 172, 193],
    [30, 136, 229],
    [94, 53, 177],
    [216, 27, 96],
    [121, 85, 72],
];

pub fn render(st: &Studio, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let s = skin();
    let (w, h) = (env.width, env.height);
    p.scene.background = s.backdrop;
    let t = |c: &str| st.target(c);
    let doc = st.doc.as_ref();

    // Canvas under everything.
    let bar = 52;
    let area = Rect::new(0, bar, w, h.saturating_sub(bar as u32));
    if doc.is_some() {
        view::canvas(p, &s, area, st, false, env.pointer);
    } else {
        p.box_(area, s.backdrop, 0);
    }

    // Top toolbar.
    p.box_(Rect::new(0, 0, w, bar as u32), s.bar, 0);
    p.hline(0, bar, w, s.line);
    let mut x = 6;
    let brush_symbol = BRUSHES
        .iter()
        .find(|(_, tool, _)| *tool == st.tool)
        .map_or("brush", |(_, _, symbol)| symbol);
    for (symbol, label, command, live, why, on) in [
        ("menu", "Main menu", "menu:main", true, "", false),
        (
            "undo",
            "Undo",
            "undo",
            doc.is_some_and(|d| d.can_undo()),
            "Nothing to undo",
            false,
        ),
        (
            "redo",
            "Redo",
            "redo",
            doc.is_some_and(|d| d.can_redo()),
            "Nothing to redo",
            false,
        ),
        (
            "apps",
            "Tools",
            "menu:tools",
            true,
            "",
            matches!(st.tool, Tool::Fill | Tool::RectSelect | Tool::Picker),
        ),
        (
            brush_symbol,
            "Brush Library",
            "menu:brushes",
            true,
            "",
            st.tool.paints(),
        ),
        ("palette", "Color Editor", "dialog:color", true, "", false),
        ("layers", "Layer Editor", "layers", true, "", st.layers_open),
    ] {
        let r = Rect::new(x, 6, 40, 40);
        if live {
            view::tool(p, &s, r, symbol, label, &t(command), on);
        } else {
            view::inert_tool(p, &s, r, symbol, why);
        }
        x += 44;
    }
    if doc.and_then(|d| d.selection()).is_some() {
        view::tool(
            p,
            &s,
            Rect::new(w as i32 - 46, 6, 40, 40),
            "close",
            "Deselect",
            &t("select-none"),
            false,
        );
    }

    // Brush and colour pucks, bottom right.
    let cx = w as i32 - 48;
    let cy = h as i32 - 60;
    p.drop_shadow(Rect::new(cx - 28, cy - 28, 56, 56), 28, 8, 50, 2);
    p.circle(cx, cy, 28, Color::WHITE);
    let dot = (st.size.clamp(2, 40) / 2) + 2;
    p.circle(cx, cy, dot, view::rgba(st.primary));
    p.region(
        Rect::new(cx - 28, cy - 28, 56, 56),
        &t("menu:brush"),
        "Brush puck: size and opacity",
    );
    let color = Rect::new(cx - 18, cy - 88, 36, 36);
    p.drop_shadow(color, 18, 6, 50, 2);
    view::swatch(p, &s, color, st.primary, &t("menu:colors"), false, true);

    // Layer Editor.
    if let (true, Some(d)) = (st.layers_open, doc) {
        let pw = 176u32.min(w / 2);
        let px = w as i32 - pw as i32 - 8;
        let ph = h.saturating_sub(bar as u32 + 150);
        let r = Rect::new(px, bar + 8, pw, ph);
        p.drop_shadow(r, 10, 10, 50, 3);
        p.border(r, s.panel, 10, s.line);
        p.region(r, &t("noop"), "Layer Editor");
        view::tool(
            p,
            &s,
            Rect::new(px + 6, r.y + 6, 32, 32),
            "plus",
            "Add layer",
            &t("layer:new"),
            false,
        );
        view::layer_rows(
            p,
            &s,
            Rect::new(px + 6, r.y + 44, pw - 12, ph.saturating_sub(150)),
            st,
            56,
        );
        let by = r.y + ph as i32 - 102;
        view::slider(
            p,
            &s,
            Rect::new(px + 10, by, pw - 20, 34),
            st,
            "Opacity",
            "layer-opacity",
            true,
        );
        view::chip(
            p,
            &s,
            Rect::new(px + 10, by + 40, pw - 20, 24),
            &format!("{} ▾", d.active_layer().blend.label()),
            &t("menu:blend"),
            false,
        );
        let count = d.layers().len();
        let active = d.active();
        let mut bx = px + 8;
        for (symbol, command, label, live, why) in [
            ("copy", "layer:duplicate", "Duplicate", true, ""),
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
                "Delete",
                count > 1,
                "A sketch keeps at least one layer",
            ),
        ] {
            let rr = Rect::new(bx, by + 70, 36, 28);
            if live {
                view::tool(p, &s, rr, symbol, label, &t(command), false);
            } else {
                view::inert_tool(p, &s, rr, symbol, why);
            }
            bx += 40;
        }
    }

    if let Some(msg) = &st.status {
        let bw = (p.measure(msg, 13, false) + 32).min(w - 32);
        let r = Rect::new((w as i32 - bw as i32) / 2, h as i32 - 140, bw, 36);
        p.box_(r, Color(50, 50, 50, 230), 18);
        p.label(
            r.x,
            r.y + 9,
            r.width,
            msg,
            13,
            Color::WHITE,
            false,
            Align::Center,
        );
    }

    menus(st, p, &s, w, h);
    view::dialog(p, &s, st, w, h);
    view::chooser(p, &s, st, w, h);
}

fn menus(st: &Studio, p: &mut Painter, s: &Skin, w: u32, h: u32) {
    let Some(Panel::Menu { id }) = &st.panel else {
        return;
    };
    let open = st.doc.is_some();
    match id.as_str() {
        "main" => {
            let items = vec![
                Item::new("New Sketch", "new"),
                Item::new("Open from Gallery", "open"),
                Item::new("Save to Gallery", "save").when(open, "Nothing to save"),
                Item::new("Save as…", "save-as").when(open, "Nothing to save"),
            ];
            view::menu(p, s, st, 6, 50, 220, &items);
        }
        "tools" => {
            let items = vec![
                Item::new("Selection", "tool:select-rect").checked(st.tool == Tool::RectSelect),
                Item::new("Flood fill", "tool:fill").checked(st.tool == Tool::Fill),
                Item::new("Color picker", "tool:picker").checked(st.tool == Tool::Picker),
                Item::separator(),
                Item::new("Flip canvas horizontally", "flip:h").when(open, "No sketch is open"),
                Item::new("Rotate canvas", "rotate:cw").when(open, "No sketch is open"),
            ];
            view::menu(p, s, st, 138, 50, 240, &items);
        }
        "brushes" => {
            let items: Vec<Item<'_>> = BRUSHES
                .iter()
                .map(|(label, tool, _)| {
                    Item::new(label, &format!("tool:{}", tool.id())).checked(st.tool == *tool)
                })
                .collect();
            view::menu(p, s, st, 182, 50, 200, &items);
        }
        "blend" => {
            let items: Vec<Item<'_>> = cw_raster::BlendMode::ALL
                .iter()
                .map(|m| {
                    Item::new(m.label(), &format!("layer:blend:{}", m.id())).checked(
                        st.doc
                            .as_ref()
                            .is_some_and(|d| d.active_layer().blend == *m),
                    )
                })
                .collect();
            view::menu(p, s, st, (w as i32 - 200).max(0), 120, 190, &items);
        }
        "brush" | "colors" => {
            let pw = 260u32.min(w - 16);
            let ph = if id == "brush" { 120 } else { 132 };
            let r = Rect::new(w as i32 - pw as i32 - 8, h as i32 - ph as i32 - 104, pw, ph);
            let z = p.z;
            p.z += 500;
            p.drop_shadow(r, 12, 12, 60, 3);
            p.border(r, s.panel, 12, s.line);
            p.region(r, &st.target("noop"), "Puck");
            if id == "brush" {
                view::slider(
                    p,
                    s,
                    Rect::new(r.x + 14, r.y + 12, pw - 28, 36),
                    st,
                    "Size",
                    "size",
                    true,
                );
                view::slider(
                    p,
                    s,
                    Rect::new(r.x + 14, r.y + 60, pw - 28, 36),
                    st,
                    "Opacity",
                    "opacity",
                    true,
                );
            } else {
                for (i, c) in SWATCHES.iter().enumerate() {
                    let color = [c[0], c[1], c[2], 255];
                    let cell = Rect::new(
                        r.x + 14 + (i as i32 % 6) * 40,
                        r.y + 14 + (i as i32 / 6) * 40,
                        30,
                        30,
                    );
                    view::swatch(
                        p,
                        s,
                        cell,
                        color,
                        &st.target(&format!("color:{}", cw_raster::hex(color))),
                        st.primary == color,
                        true,
                    );
                }
                view::button(
                    p,
                    s,
                    Rect::new(r.x + 14, r.y + 96, pw - 28, 28),
                    "Color Editor…",
                    &st.target("dialog:color"),
                    false,
                );
            }
            p.z = z;
        }
        _ => {}
    }
}
