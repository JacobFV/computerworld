//! The phones' photo editors, which live inside Photos:
//!
//! * iOS Photos' edit mode — Cancel and Done, the Adjust / Filters / Crop tabs, the row
//!   of round adjustment buttons over the scrubbing dial, filter thumbnails, the crop
//!   tools with the straighten dial, and Markup's tray of pens.
//! * Google Photos' editor — the close button, Suggestions / Crop / Adjust / Filters /
//!   Markup tabs, chips under a slider, and the Save copy button.
//!
//! Adjustments and filters are a non-destructive look previewed on screen and baked
//! into the full-resolution photo when it is saved.
use super::look::{self, ASPECTS};
use super::view::{self, Skin};
use super::{Product, Studio, Tool};
use crate::desktop_scene::{shared::Align, Painter};
use cw_raster::Canvas;
use cw_scene::{Color, Primitive, Rect};

pub const PHOTO_EDIT_PREFIX: &str = "photos:edit";

fn skin(product: Product) -> Skin {
    if product == Product::IosPhotos {
        Skin {
            bg: Color::BLACK,
            bar: Color::BLACK,
            panel: Color::rgb(28, 28, 30),
            text: Color::WHITE,
            muted: Color::rgb(142, 142, 147),
            accent: Color::rgb(255, 214, 10),
            on_accent: Color::BLACK,
            selected: Color(255, 214, 10, 40),
            line: Color(255, 255, 255, 40),
            backdrop: Color::BLACK,
            radius: 10,
            font: 14,
        }
    } else {
        Skin {
            bg: Color::rgb(19, 19, 20),
            bar: Color::rgb(19, 19, 20),
            panel: Color::rgb(31, 31, 31),
            text: Color::rgb(227, 227, 227),
            muted: Color::rgb(150, 150, 150),
            accent: Color::rgb(168, 199, 250),
            on_accent: Color::rgb(6, 46, 111),
            selected: Color(211, 227, 253, 40),
            line: Color(255, 255, 255, 30),
            backdrop: Color::rgb(19, 19, 20),
            radius: 16,
            font: 13,
        }
    }
}

/// A small preview of the photo under one filter preset.
fn filter_thumb(st: &Studio, p: &mut Painter, r: Rect, preset: usize) {
    let Some(doc) = &st.doc else {
        return;
    };
    let (w, h) = (doc.width(), doc.height());
    let k = w.max(h).div_ceil(r.width.max(1)).max(1);
    let (tw, th) = ((w / k).max(1), (h / k).max(1));
    let mut small = Canvas::new(tw, th);
    for y in 0..th {
        for x in 0..tw {
            small.set(
                x as i32,
                y as i32,
                doc.pixel((x * k) as i32, (y * k) as i32),
            );
        }
    }
    let mut probe = st.clone();
    probe.look.clear();
    probe.look.insert("filter".into(), preset as i32);
    let out = look::apply(&probe, &small);
    p.node(
        Rect::new(
            r.x + (r.width as i32 - tw.min(r.width) as i32) / 2,
            r.y + (r.height as i32 - th.min(r.height) as i32) / 2,
            tw.min(r.width),
            th.min(r.height),
        ),
        Primitive::Image {
            width: out.width(),
            height: out.height(),
            rgba: out.into_pixels(),
        },
        None,
    );
}

/// The scrubbing dial: tick marks around the current value, a drag surface.
fn dial(st: &Studio, p: &mut Painter, s: &Skin, r: Rect, param: &str) {
    let value = st.param(param);
    let mid = r.x + r.width as i32 / 2;
    // One tick per unit, three pixels apart, scrolled so the value sits at the centre.
    for i in -60..=60 {
        let v = value + i;
        let x = mid + i * 3;
        if x < r.x || x > r.x + r.width as i32 {
            continue;
        }
        let major = v % 10 == 0;
        p.box_(
            Rect::new(
                x,
                r.y + if major { 4 } else { 10 },
                1,
                if major { 20 } else { 10 },
            ),
            if major { s.text } else { s.muted },
            0,
        );
    }
    p.box_(Rect::new(mid - 1, r.y, 2, 28), s.accent, 1);
    p.region(r, &st.target(&format!("dial:{param}")), "Adjustment dial");
}

pub fn render_photo_editor(st: &Studio, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let s = skin(st.product);
    let (w, h) = (env.width, env.height);
    p.scene.background = s.bg;
    p.box_(Rect::new(0, 0, w, h), s.bg, 0);
    if st.product == Product::IosPhotos {
        ios(st, p, &s, w, h);
    } else {
        google(st, p, &s, w, h);
    }
    view::dialog(p, &s, st, w, h);
}

fn photo(st: &Studio, p: &mut Painter, s: &Skin, r: Rect) {
    if st.doc.is_some() {
        view::canvas_with(p, s, r, st, false, st.tab == "markup");
    } else {
        let text = match (&st.loading, &st.status) {
            (_, Some(msg)) => msg.clone(),
            (Some(_), None) => "Loading…".into(),
            _ => String::new(),
        };
        p.center(
            r.x,
            r.y + r.height as i32 / 2 - 10,
            r.width,
            &text,
            14,
            s.muted,
        );
    }
}

fn ios(st: &Studio, p: &mut Painter, s: &Skin, w: u32, h: u32) {
    let t = |c: &str| st.target(c);
    let markup = st.tab == "markup";
    // Top bar: Cancel, then Markup and Done.
    p.button(
        Rect::new(8, 8, 80, 36),
        Color::TRANSPARENT,
        8,
        &t("discard"),
        "Cancel",
    );
    p.label(8, 16, 80, "Cancel", 16, s.text, false, Align::Center);
    if markup {
        p.button(
            Rect::new(w as i32 - 88, 8, 80, 36),
            Color::TRANSPARENT,
            8,
            &t("tab:adjust"),
            "Done",
        );
        p.label(
            w as i32 - 88,
            16,
            80,
            "Done",
            16,
            s.accent,
            true,
            Align::Center,
        );
    } else {
        view::tool(
            p,
            s,
            Rect::new(w as i32 - 132, 8, 38, 36),
            "marker",
            "Markup",
            &t("tab:markup"),
            false,
        );
        let done = Rect::new(w as i32 - 84, 10, 72, 32);
        if st.doc.is_some() {
            p.button(done, s.accent, 16, &t("look:done"), "Done");
        } else {
            p.box_(done, Color(255, 214, 10, 60), 16);
            p.disabled("The photo is still loading");
        }
        p.label(
            done.x,
            done.y + 7,
            done.width,
            "Done",
            15,
            Color::BLACK,
            true,
            Align::Center,
        );
    }
    let bottom = if markup { 150 } else { 196 };
    let area = Rect::new(0, 52, w, h.saturating_sub(52 + bottom));
    photo(st, p, s, area);
    let by = h as i32 - bottom as i32;
    if markup {
        // White tray of pens with the colour row above.
        let tray = Rect::new(8, by + 50, w - 16, 92);
        p.box_(tray, Color::rgb(242, 242, 247), 16);
        let pens = [
            (Tool::Pen, "pencil", "Pen"),
            (Tool::Marker, "marker", "Marker"),
            (Tool::Pencil, "pencil", "Pencil"),
            (Tool::Eraser, "eraser", "Eraser"),
            (Tool::Text, "text-tool", "Text"),
        ];
        let step = (tray.width as i32 - 20) / pens.len() as i32;
        for (i, (tool, symbol, label)) in pens.iter().enumerate() {
            let r = Rect::new(
                tray.x + 10 + i as i32 * step,
                tray.y + 8,
                step.max(1) as u32 - 4,
                76,
            );
            let on = st.tool == *tool;
            p.button(
                r,
                if on {
                    Color(0, 0, 0, 16)
                } else {
                    Color::TRANSPARENT
                },
                10,
                &t(&format!("tool:{}", tool.id())),
                label,
            );
            p.symbol(
                symbol,
                r.x + (r.width as i32 - 28) / 2,
                r.y + if on { 8 } else { 18 },
                28,
                Color::rgb(28, 28, 30),
            );
            p.label(
                r.x,
                r.y + 52,
                r.width,
                label,
                11,
                Color::rgb(60, 60, 67),
                false,
                Align::Center,
            );
        }
        let colors: [[u8; 3]; 7] = [
            [0, 0, 0],
            [255, 255, 255],
            [255, 59, 48],
            [255, 204, 0],
            [52, 199, 89],
            [0, 122, 255],
            [175, 82, 222],
        ];
        let mut x = 16;
        for c in colors {
            let color = [c[0], c[1], c[2], 255];
            view::swatch(
                p,
                s,
                Rect::new(x, by + 10, 28, 28),
                color,
                &t(&format!("color:{}", cw_raster::hex(color))),
                st.primary == color,
                true,
            );
            x += 36;
        }
        let d = st.doc.as_ref();
        for (i, (symbol, command, live)) in [
            ("undo", "undo", d.is_some_and(|d| d.can_undo())),
            ("redo", "redo", d.is_some_and(|d| d.can_redo())),
        ]
        .iter()
        .enumerate()
        {
            let r = Rect::new(w as i32 - 88 + i as i32 * 40, by + 6, 36, 36);
            if *live {
                view::tool(p, s, r, symbol, command, &t(command), false);
            } else {
                view::inert_tool(p, s, r, symbol, "Nothing to undo or redo");
            }
        }
        return;
    }
    match st.tab.as_str() {
        "filters" => {
            let list = look::IOS_FILTERS;
            let current = st.look.get("filter").copied().unwrap_or(0).max(0) as usize;
            p.center(
                0,
                by + 6,
                w,
                &list[current.min(list.len() - 1)].1.to_uppercase(),
                12,
                s.muted,
            );
            let size = 64;
            let start = (current as i32 - 2).max(0) as usize;
            let mut x = (w as i32 - (5 * (size + 10))) / 2;
            for (i, (id, _)) in list.iter().enumerate().skip(start).take(5) {
                let r = Rect::new(x, by + 32, size as u32, size as u32);
                p.button(
                    r,
                    Color(255, 255, 255, 20),
                    4,
                    &t(&format!("preset:{id}")),
                    id,
                );
                filter_thumb(st, p, r, i);
                if i == current {
                    p.border(
                        Rect::new(r.x - 2, r.y - 2, r.width + 4, r.height + 4),
                        Color::TRANSPARENT,
                        5,
                        s.accent,
                    );
                }
                x += size + 10;
            }
        }
        "crop" => {
            let mut x = 12;
            for (symbol, label, command) in [
                ("flip-h", "Flip", "look:flip"),
                ("rotate-ccw", "Rotate", "look:rotate"),
            ] {
                view::tool(
                    p,
                    s,
                    Rect::new(x, by + 4, 40, 36),
                    symbol,
                    label,
                    &t(command),
                    false,
                );
                x += 44;
            }
            for (id, label, _, _) in ASPECTS {
                let bw = p.measure(label, 12, false) + 18;
                if x + bw as i32 > w as i32 - 8 {
                    break;
                }
                view::chip(
                    p,
                    s,
                    Rect::new(x, by + 10, bw, 26),
                    label,
                    &t(&format!("look:aspect:{id}")),
                    false,
                );
                x += bw as i32 + 6;
            }
            p.center(
                0,
                by + 54,
                w,
                &format!("STRAIGHTEN {}°", st.param("straighten")),
                12,
                s.muted,
            );
            dial(st, p, s, Rect::new(20, by + 76, w - 40, 30), "straighten");
        }
        _ => {
            let list = look::IOS_ADJUST;
            let focus = if st.focus.is_empty() {
                "exposure"
            } else {
                st.focus.as_str()
            };
            let label = list
                .iter()
                .find(|(k, _, _)| *k == focus)
                .map_or("", |(_, l, _)| l);
            let value = st.param(focus);
            p.center(
                0,
                by + 4,
                w,
                &format!(
                    "{} {}",
                    label.to_uppercase(),
                    if focus == "auto" {
                        String::new()
                    } else {
                        value.to_string()
                    }
                ),
                12,
                s.muted,
            );
            let at = list.iter().position(|(k, _, _)| *k == focus).unwrap_or(0);
            let size = 46i32;
            let gap = 12;
            let visible = ((w as i32 - 20) / (size + gap)).max(1) as usize;
            let start = at
                .saturating_sub(visible / 2)
                .min(list.len().saturating_sub(visible));
            let row_w = visible.min(list.len()) as i32 * (size + gap) - gap;
            let mut x = (w as i32 - row_w) / 2;
            for (id, name, symbol) in list.iter().skip(start).take(visible) {
                let r = Rect::new(x, by + 28, size as u32, size as u32);
                let on = *id == focus;
                let v = st.param(id);
                let command = if *id == "auto" {
                    format!("set:auto:{}", 1 - v)
                } else {
                    format!("focus:{id}")
                };
                p.button(
                    r,
                    Color(255, 255, 255, if on { 40 } else { 18 }),
                    size as u32 / 2,
                    &t(&command),
                    name,
                );
                if v != 0 {
                    p.ring(r.x + size / 2, r.y + size / 2, size as u32 / 2, 2, s.accent);
                }
                p.symbol(
                    symbol,
                    r.x + 13,
                    r.y + 13,
                    20,
                    if v != 0 { s.accent } else { s.text },
                );
                x += size + gap;
            }
            if focus != "auto" {
                dial(st, p, s, Rect::new(20, by + 86, w - 40, 30), focus);
            }
        }
    }
    // Bottom tabs.
    let tabs = [
        ("adjust", "Adjust", "dial"),
        ("filters", "Filters", "filters"),
        ("crop", "Crop", "crop"),
    ];
    let tw = w / tabs.len() as u32;
    for (i, (id, label, symbol)) in tabs.iter().enumerate() {
        let on = st.tab == *id || (st.tab.is_empty() && *id == "adjust");
        let r = Rect::new(i as i32 * tw as i32, h as i32 - 64, tw, 60);
        p.button(r, Color::TRANSPARENT, 0, &t(&format!("tab:{id}")), label);
        let c = if on { s.text } else { s.muted };
        p.symbol(symbol, r.x + (tw as i32 - 22) / 2, r.y + 8, 22, c);
        p.label(r.x, r.y + 34, tw, label, 11, c, on, Align::Center);
    }
}

fn google(st: &Studio, p: &mut Painter, s: &Skin, w: u32, h: u32) {
    let t = |c: &str| st.target(c);
    view::tool(
        p,
        s,
        Rect::new(8, 8, 40, 40),
        "close",
        "Close without saving",
        &t("discard"),
        false,
    );
    let bottom = 190;
    let area = Rect::new(0, 56, w, h.saturating_sub(56 + bottom));
    photo(st, p, s, area);
    let by = h as i32 - bottom as i32;
    // Tabs.
    let tabs = [
        ("suggestions", "Suggestions"),
        ("crop", "Crop"),
        ("adjust", "Adjust"),
        ("filters", "Filters"),
        ("markup", "Markup"),
    ];
    let mut x = 8;
    for (id, label) in tabs {
        let bw = p.measure(label, 13, true) + 22;
        let on = st.tab == id || (st.tab.is_empty() && id == "adjust");
        let r = Rect::new(x, by + 6, bw, 32);
        p.button(
            r,
            if on { s.selected } else { Color::TRANSPARENT },
            16,
            &t(&format!("tab:{id}")),
            label,
        );
        p.label(
            r.x,
            r.y + 8,
            r.width,
            label,
            13,
            if on { s.accent } else { s.text },
            on,
            Align::Center,
        );
        x += bw as i32 + 2;
    }
    let row = by + 48;
    match st.tab.as_str() {
        "suggestions" => {
            let mut x = 12;
            for (id, label) in look::GOOGLE_SUGGESTIONS {
                let bw = p.measure(label, 13, false) + 30;
                view::chip(
                    p,
                    s,
                    Rect::new(x, row + 20, bw, 36),
                    label,
                    &t(&format!("preset:{id}")),
                    false,
                );
                x += bw as i32 + 8;
            }
        }
        "crop" => {
            let mut x = 12;
            for (symbol, label, command) in [
                ("rotate-ccw", "Rotate", "look:rotate"),
                ("flip-h", "Flip", "look:flip"),
            ] {
                view::tool(
                    p,
                    s,
                    Rect::new(x, row, 40, 36),
                    symbol,
                    label,
                    &t(command),
                    false,
                );
                x += 44;
            }
            for (id, label, _, _) in ASPECTS {
                let bw = p.measure(label, 12, false) + 18;
                if x + bw as i32 > w as i32 - 8 {
                    break;
                }
                view::chip(
                    p,
                    s,
                    Rect::new(x, row + 6, bw, 26),
                    label,
                    &t(&format!("look:aspect:{id}")),
                    false,
                );
                x += bw as i32 + 6;
            }
            view::slider(
                p,
                s,
                Rect::new(20, row + 46, w - 40, 36),
                st,
                &format!("{}°", st.param("straighten")),
                "straighten",
                false,
            );
        }
        "filters" => {
            let list = look::GOOGLE_FILTERS;
            let current = st.look.get("filter").copied().unwrap_or(0).max(0) as usize;
            let size = 60;
            let start = current.saturating_sub(2).min(list.len().saturating_sub(5));
            let mut x = (w as i32 - 5 * (size + 10)) / 2;
            for (i, (id, label)) in list.iter().enumerate().skip(start).take(5) {
                let r = Rect::new(x, row, size as u32, size as u32);
                p.button(
                    r,
                    Color(255, 255, 255, 20),
                    8,
                    &t(&format!("preset:{id}")),
                    label,
                );
                filter_thumb(st, p, r, i);
                if i == current {
                    p.border(
                        Rect::new(r.x - 2, r.y - 2, r.width + 4, r.height + 4),
                        Color::TRANSPARENT,
                        9,
                        s.accent,
                    );
                }
                p.label(
                    r.x - 4,
                    r.y + size + 4,
                    size as u32 + 8,
                    label,
                    11,
                    if i == current { s.accent } else { s.text },
                    false,
                    Align::Center,
                );
                x += size + 10;
            }
        }
        "markup" => {
            let mut x = 12;
            for (tool, symbol, label) in [
                (Tool::Pen, "pencil", "Pen"),
                (Tool::Highlighter, "highlighter", "Highlighter"),
                (Tool::Text, "text-tool", "Text"),
            ] {
                let bw = p.measure(label, 12, false) + 40;
                let on = st.tool == tool;
                let r = Rect::new(x, row, bw, 32);
                p.button(
                    r,
                    if on { s.selected } else { Color::TRANSPARENT },
                    16,
                    &t(&format!("tool:{}", tool.id())),
                    label,
                );
                p.symbol(
                    symbol,
                    r.x + 8,
                    r.y + 7,
                    18,
                    if on { s.accent } else { s.text },
                );
                p.label(
                    r.x + 28,
                    r.y + 8,
                    bw - 30,
                    label,
                    12,
                    if on { s.accent } else { s.text },
                    false,
                    Align::Left,
                );
                x += bw as i32 + 6;
            }
            let colors: [[u8; 3]; 8] = [
                [234, 67, 53],
                [251, 140, 0],
                [251, 188, 4],
                [52, 168, 83],
                [66, 133, 244],
                [161, 66, 244],
                [255, 255, 255],
                [0, 0, 0],
            ];
            let mut x = 14;
            for c in colors {
                let color = [c[0], c[1], c[2], 255];
                view::swatch(
                    p,
                    s,
                    Rect::new(x, row + 46, 26, 26),
                    color,
                    &t(&format!("color:{}", cw_raster::hex(color))),
                    st.primary == color,
                    true,
                );
                x += 34;
            }
            let d = st.doc.as_ref();
            let r = Rect::new(w as i32 - 48, row + 42, 36, 34);
            if d.is_some_and(|d| d.can_undo()) {
                view::tool(p, s, r, "undo", "Undo", &t("undo"), false);
            } else {
                view::inert_tool(p, s, r, "undo", "Nothing to undo");
            }
        }
        _ => {
            let list = look::GOOGLE_ADJUST;
            let focus = if st.focus.is_empty() {
                "brightness"
            } else {
                st.focus.as_str()
            };
            view::slider(
                p,
                s,
                Rect::new(20, row - 2, w - 40, 40),
                st,
                list.iter()
                    .find(|(k, _, _)| *k == focus)
                    .map_or("", |(_, l, _)| l),
                focus,
                true,
            );
            let at = list.iter().position(|(k, _, _)| *k == focus).unwrap_or(0);
            let visible = ((w as i32 - 16) / 82).max(1) as usize;
            let start = at
                .saturating_sub(visible / 2)
                .min(list.len().saturating_sub(visible));
            let mut x = 10;
            for (id, label, symbol) in list.iter().skip(start).take(visible) {
                let on = *id == focus;
                let r = Rect::new(x, row + 50, 76, 60);
                p.button(r, Color::TRANSPARENT, 12, &t(&format!("focus:{id}")), label);
                p.circle(
                    r.x + 38,
                    r.y + 18,
                    18,
                    if on {
                        s.accent
                    } else {
                        Color(255, 255, 255, 24)
                    },
                );
                p.symbol(
                    symbol,
                    r.x + 28,
                    r.y + 8,
                    20,
                    if on { s.on_accent } else { s.text },
                );
                p.label(r.x, r.y + 40, 76, label, 11, s.text, false, Align::Center);
                x += 82;
            }
        }
    }
    // Save copy.
    let save = Rect::new(w as i32 - 128, h as i32 - 48, 116, 40);
    if st.doc.is_some() {
        p.button(save, s.accent, 20, &t("look:done"), "Save copy");
    } else {
        p.box_(save, Color(168, 199, 250, 60), 20);
        p.disabled("The photo is still loading");
    }
    p.label(
        save.x,
        save.y + 11,
        save.width,
        "Save copy",
        13,
        s.on_accent,
        true,
        Align::Center,
    );
}
