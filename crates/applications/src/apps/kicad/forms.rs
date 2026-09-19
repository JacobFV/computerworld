//! Dialogs that are a column of labelled fields with a few choices under them — sheet
//! and sheet-pin properties, footprint properties, the library editors' pin, pad,
//! symbol and footprint properties, new-library and new-item dialogs — drawn one way.
use super::widgets::{self as w, chrome};
use super::{Dialog, Kicad};
use crate::desktop_scene::{shared::Align, Painter};
use cw_eda::symbols::PinType;
use cw_scene::{Color, Rect};

/// Electrical pin types in the order KiCad's pin dialog lists them.
pub(super) const PIN_TYPES: [PinType; 12] = [
    PinType::Input,
    PinType::Output,
    PinType::Bidirectional,
    PinType::TriState,
    PinType::Passive,
    PinType::Free,
    PinType::Unspecified,
    PinType::PowerIn,
    PinType::PowerOut,
    PinType::OpenCollector,
    PinType::OpenEmitter,
    PinType::NoConnect,
];
/// Simulation model choices of the symbol editor, keyword and label.
pub(super) const MODELS: [(&str, &str); 19] = [
    ("none", "None"),
    ("R", "Resistor"),
    ("C", "Capacitor"),
    ("L", "Inductor"),
    ("D", "Diode"),
    ("NPN", "NPN BJT"),
    ("PNP", "PNP BJT"),
    ("NMOS", "NMOS"),
    ("PMOS", "PMOS"),
    ("V", "Voltage source"),
    ("I", "Current source"),
    ("AND", "AND gate"),
    ("NAND", "NAND gate"),
    ("OR", "OR gate"),
    ("NOR", "NOR gate"),
    ("XOR", "XOR gate"),
    ("NOT", "Inverter"),
    ("DFF", "D flip-flop"),
    ("555", "555 timer"),
];

impl Kicad {
    pub(super) fn render_router_settings(
        &self,
        p: &mut Painter,
        env: &crate::AppEnv<'_>,
        walkaround: bool,
    ) {
        let c = chrome(env.theme);
        let d = self.ui.dialog.as_ref().expect("a dialog is open");
        let body = w::dialog(p, &c, env.theme, env.width, env.height, 460, 230, d.title());
        p.label(body.x, body.y, 300, "Mode", 13, w::INK, true, Align::Left);
        w::radio(
            p,
            &c,
            body.x,
            body.y + 24,
            "Highlight collisions",
            !walkaround,
            "kicad:dlg:mode:highlight",
        );
        w::radio(
            p,
            &c,
            body.x,
            body.y + 48,
            "Walk around",
            walkaround,
            "kicad:dlg:mode:walkaround",
        );
        p.paragraph(
            body.x,
            body.y + 80,
            body.width,
            "Walk around finds a 45° path round pads, tracks, vias and rule areas of other nets, keeping the clearance. Highlight collisions places what is drawn and marks what it violates.",
            12,
            w::MUTED,
        );
        let by = body.y + body.height as i32 - 30;
        w::button(
            p,
            &c,
            Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
            "OK",
            "kicad:dlg:ok",
            true,
        );
    }

    /// Fields, then the dialog's own choices, then its error and OK/Cancel.
    pub(super) fn render_form_dialog(
        &self,
        p: &mut Painter,
        env: &crate::AppEnv<'_>,
        dialog: &Dialog,
    ) {
        let c = chrome(env.theme);
        let focus = self.ui.focus.as_deref();
        let names = dialog.field_names();
        // Choices take rows under the fields.
        let choice_rows = match dialog {
            Dialog::SheetPin { .. } => 2,
            Dialog::FootprintProperties { .. } => 1,
            Dialog::PinProperties { .. } => 6,
            Dialog::SymbolFields { .. } => 8,
            Dialog::PadProperties { .. } => 3,
            _ => 0,
        };
        let height = 110 + names.len() as u32 * 34 + choice_rows * 26;
        let body = w::dialog(
            p,
            &c,
            env.theme,
            env.width,
            env.height,
            600,
            height,
            dialog.title(),
        );
        let mut y = body.y;
        let lw = 150;
        for n in &names {
            let value = dialog.value(n);
            // Show the raw text (with any spaces the user typed) while editing.
            let raw = {
                let mut d = dialog.clone();
                d.field_mut(n).cloned().unwrap_or(value)
            };
            w::label_field(
                p,
                &c,
                body.x,
                y,
                lw,
                body.width - lw,
                &format!("{n}:"),
                &raw,
                n,
                focus == Some(n.as_str()),
            );
            y += 34;
        }
        let grid = |p: &mut Painter,
                    items: Vec<(String, bool, String)>,
                    per_row: usize,
                    x0: i32,
                    col: i32,
                    y: &mut i32| {
            for (i, (label, on, target)) in items.iter().enumerate() {
                let x = x0 + (i % per_row) as i32 * col;
                if i > 0 && i % per_row == 0 {
                    *y += 24;
                }
                w::radio(p, &c, x, *y, label, *on, target);
            }
            *y += 26;
        };
        match dialog {
            Dialog::SheetPin { shape, .. } => {
                p.label(body.x, y + 1, lw, "Shape:", 13, w::INK, false, Align::Left);
                let items = super::sch::SHAPES
                    .iter()
                    .map(|s| {
                        (
                            (*s).to_owned(),
                            super::sch::shape_name(*shape) == *s,
                            format!("kicad:dlg:shape:{s}"),
                        )
                    })
                    .collect();
                grid(p, items, 3, body.x + lw as i32, 130, &mut y);
            }
            Dialog::FootprintProperties { back, locked, .. } => {
                w::checkbox(
                    p,
                    &c,
                    body.x + lw as i32,
                    y,
                    "Back side (flipped)",
                    *back,
                    "kicad:dlg:side",
                );
                w::checkbox(
                    p,
                    &c,
                    body.x + lw as i32 + 200,
                    y,
                    "Locked",
                    *locked,
                    "kicad:dlg:lock",
                );
                y += 26;
            }
            Dialog::PinProperties { kind, orient, .. } => {
                p.label(
                    body.x,
                    y + 1,
                    lw,
                    "Electrical type:",
                    13,
                    w::INK,
                    false,
                    Align::Left,
                );
                let items = PIN_TYPES
                    .iter()
                    .map(|t| {
                        (
                            t.label().to_owned(),
                            t == kind,
                            format!("kicad:dlg:kind:{}", t.keyword()),
                        )
                    })
                    .collect();
                grid(p, items, 3, body.x + lw as i32, 145, &mut y);
                p.label(
                    body.x,
                    y + 1,
                    lw,
                    "Orientation:",
                    13,
                    w::INK,
                    false,
                    Align::Left,
                );
                let items = [(0u16, "Right"), (180, "Left"), (90, "Up"), (270, "Down")]
                    .iter()
                    .map(|(a, l)| {
                        // KiCad names a pin's orientation by the way it runs from its
                        // connection point into the body.
                        (
                            (*l).to_owned(),
                            *orient == *a,
                            format!("kicad:dlg:orient:{a}"),
                        )
                    })
                    .collect();
                grid(p, items, 4, body.x + lw as i32, 100, &mut y);
            }
            Dialog::SymbolFields {
                model,
                power,
                pin_names_hidden,
                pin_numbers_hidden,
                ..
            } => {
                p.label(
                    body.x,
                    y + 1,
                    lw,
                    "Simulation model:",
                    13,
                    w::INK,
                    false,
                    Align::Left,
                );
                let items = MODELS
                    .iter()
                    .map(|(k, l)| ((*l).to_owned(), model == k, format!("kicad:dlg:model:{k}")))
                    .collect();
                grid(p, items, 3, body.x + lw as i32, 145, &mut y);
                w::checkbox(
                    p,
                    &c,
                    body.x + lw as i32,
                    y,
                    "Power symbol",
                    *power,
                    "kicad:dlg:toggle:power",
                );
                y += 24;
                w::checkbox(
                    p,
                    &c,
                    body.x + lw as i32,
                    y,
                    "Show pin names",
                    !*pin_names_hidden,
                    "kicad:dlg:toggle:names",
                );
                w::checkbox(
                    p,
                    &c,
                    body.x + lw as i32 + 200,
                    y,
                    "Show pin numbers",
                    !*pin_numbers_hidden,
                    "kicad:dlg:toggle:numbers",
                );
                y += 26;
            }
            Dialog::PadProperties { smd, shape, .. } => {
                p.label(
                    body.x,
                    y + 1,
                    lw,
                    "Pad type:",
                    13,
                    w::INK,
                    false,
                    Align::Left,
                );
                w::radio(
                    p,
                    &c,
                    body.x + lw as i32,
                    y,
                    "Through-hole",
                    !*smd,
                    "kicad:dlg:smd:no",
                );
                w::radio(
                    p,
                    &c,
                    body.x + lw as i32 + 150,
                    y,
                    "SMD",
                    *smd,
                    "kicad:dlg:smd:yes",
                );
                y += 26;
                p.label(
                    body.x,
                    y + 1,
                    lw,
                    "Pad shape:",
                    13,
                    w::INK,
                    false,
                    Align::Left,
                );
                let items = ["circle", "rect", "oval", "roundrect"]
                    .iter()
                    .map(|s| ((*s).to_owned(), shape == s, format!("kicad:dlg:shape:{s}")))
                    .collect();
                grid(p, items, 4, body.x + lw as i32, 105, &mut y);
            }
            _ => {}
        }
        let error = match dialog {
            Dialog::SheetProperties { error, .. }
            | Dialog::SheetPin { error, .. }
            | Dialog::FootprintProperties { error, .. }
            | Dialog::NewLibrary { error, .. }
            | Dialog::NewSymbol { error, .. }
            | Dialog::PinProperties { error, .. }
            | Dialog::SymbolFields { error, .. }
            | Dialog::NewFootprint { error, .. }
            | Dialog::PadProperties { error, .. }
            | Dialog::FootprintFields { error, .. } => error.as_str(),
            _ => "",
        };
        if !error.is_empty() {
            p.label(
                body.x,
                y + 4,
                body.width,
                error,
                12,
                Color::rgb(190, 30, 30),
                false,
                Align::Left,
            );
        }
        let by = body.y + body.height as i32 - 30;
        w::button(
            p,
            &c,
            Rect::new(body.x + body.width as i32 - 180, by, 84, 28),
            "Cancel",
            "kicad:dlg:cancel",
            false,
        );
        w::button(
            p,
            &c,
            Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
            "OK",
            "kicad:dlg:ok",
            true,
        );
    }
}
