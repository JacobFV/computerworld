//! What the merged-cell, border, conditional formatting and pivot table features put on
//! screen, shared by every product: their menus, their dialogs and the PivotTable
//! field list.
use super::features::{rule_title, BAR_COLORS, COLOR_SCALES, PRESETS};
use super::{Book, Flavor, SheetDialog};
use crate::desktop_scene::shared::Align;
use crate::desktop_scene::Painter;
use cw_scene::{Color, Rect};
use cw_sheet::conditional::{Period, ICON_SETS};
use cw_sheet::pivot::Agg;
use cw_sheet::Line;

const INK: Color = Color::rgb(36, 36, 36);
const MUTED: Color = Color::rgb(110, 110, 110);
const FAINT: Color = Color::rgb(165, 165, 165);
const LINE: Color = Color::rgb(218, 218, 218);

type Items = Vec<(String, Result<String, &'static str>)>;

fn hex(c: [u8; 3]) -> String {
    format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}
fn line_name(l: Line) -> &'static str {
    match l {
        Line::Thin => "Thin",
        Line::Medium => "Medium",
        Line::Thick => "Thick",
        Line::Double => "Double",
        Line::Dotted => "Dotted",
        Line::Dashed => "Dashed",
        Line::Hair => "Hair",
        Line::MediumDashed => "Medium Dashed",
        Line::DashDot => "Dash Dot",
        Line::MediumDashDot => "Medium Dash Dot",
        Line::DashDotDot => "Dash Dot Dot",
        Line::MediumDashDotDot => "Medium Dash Dot Dot",
        Line::SlantDashDot => "Slanted Dash Dot",
    }
}
pub fn icon_set_name(set: &str) -> &'static str {
    match set {
        "3Arrows" => "3 Arrows (Colored)",
        "3ArrowsGray" => "3 Arrows (Gray)",
        "3TrafficLights1" => "3 Traffic Lights",
        "3Symbols" => "3 Symbols (Circled)",
        "3Flags" => "3 Flags",
        "4Arrows" => "4 Arrows (Colored)",
        "4TrafficLights" => "4 Traffic Lights",
        _ => "5 Arrows (Colored)",
    }
}
/// The menus these features add, by name; `None` for a menu that is not theirs.
pub fn menu_items(book: &Book, name: &str, flavor: Flavor) -> Option<Items> {
    let s = |label: &str, t: &str| (label.to_owned(), Ok(t.to_owned()));
    let sep = || (String::new(), Err(""));
    let merged_here = book
        .sheet_ref()
        .merges
        .iter()
        .any(|m| m.intersect(&book.selection()).is_some());
    let unmerge = |label: &str| {
        (
            label.to_owned(),
            if merged_here {
                Ok("unmerge".to_owned())
            } else {
                Err("there are no merged cells in the selection")
            },
        )
    };
    let pivot_here = book.current_pivot().is_some();
    let in_pivot = |label: &str, t: &str| {
        (
            label.to_owned(),
            if pivot_here {
                Ok(t.to_owned())
            } else {
                Err("select a cell in a pivot table first")
            },
        )
    };
    Some(match name {
        "merge" => match flavor {
            Flavor::Sheets => vec![
                s("Merge all", "merge:cells"),
                s("Merge vertically", "merge:down"),
                s("Merge horizontally", "merge:across"),
                unmerge("Unmerge"),
            ],
            Flavor::Calc => vec![
                s("Merge and Center Cells", "merge:center"),
                s("Merge Cells", "merge:cells"),
                unmerge("Unmerge Cells"),
            ],
            Flavor::Numbers => vec![s("Merge Cells", "merge:cells"), unmerge("Unmerge Cells")],
            Flavor::Excel => vec![
                s("Merge & Center", "merge:center"),
                s("Merge Across", "merge:across"),
                s("Merge Cells", "merge:cells"),
                unmerge("Unmerge Cells"),
            ],
        },
        "borders" => {
            let mut v = match flavor {
                Flavor::Sheets => vec![
                    s("All borders", "border:all"),
                    s("Inner borders", "border:inside"),
                    s("Horizontal borders", "border:insideh"),
                    s("Vertical borders", "border:insidev"),
                    s("Outer borders", "border:outside"),
                    s("Left border", "border:left"),
                    s("Top border", "border:top"),
                    s("Right border", "border:right"),
                    s("Bottom border", "border:bottom"),
                    s("Clear borders", "border:none"),
                ],
                Flavor::Calc => vec![
                    s("No Borders", "border:none"),
                    s("Left Border", "border:left"),
                    s("Right Border", "border:right"),
                    s("Top Border", "border:top"),
                    s("Bottom Border", "border:bottom"),
                    s("Top and Bottom Borders", "border:topbottom"),
                    s("Outer Border", "border:outside"),
                    s("Outer Border and All Inner Lines", "border:all"),
                    s("Inner Horizontal Lines", "border:insideh"),
                    s("Inner Vertical Lines", "border:insidev"),
                ],
                Flavor::Numbers => vec![
                    s("All Borders", "border:all"),
                    s("Outside Borders", "border:outside"),
                    s("Inside Borders", "border:inside"),
                    s("Top Border", "border:top"),
                    s("Bottom Border", "border:bottom"),
                    s("Left Border", "border:left"),
                    s("Right Border", "border:right"),
                    s("No Borders", "border:none"),
                ],
                Flavor::Excel => vec![
                    s("Bottom Border", "border:bottom"),
                    s("Top Border", "border:top"),
                    s("Left Border", "border:left"),
                    s("Right Border", "border:right"),
                    sep(),
                    s("No Border", "border:none"),
                    s("All Borders", "border:all"),
                    s("Outside Borders", "border:outside"),
                    s("Thick Outside Borders", "border:thickoutside"),
                    sep(),
                    s("Bottom Double Border", "border:doublebottom"),
                    s("Thick Bottom Border", "border:thickbottom"),
                    s("Top and Bottom Border", "border:topbottom"),
                    s("Top and Thick Bottom Border", "border:topthickbottom"),
                    s("Top and Double Bottom Border", "border:topdoublebottom"),
                    sep(),
                    s("Draw Border", "drawborder:border"),
                    s("Draw Border Grid", "drawborder:grid"),
                    s("Erase Border", "drawborder:erase"),
                ],
            };
            let (color, style) = match flavor {
                Flavor::Sheets => ("Border color ›", "Border style ›"),
                Flavor::Calc => ("Border Color ›", "Border Style ›"),
                _ => ("Line Color ›", "Line Style ›"),
            };
            v.push(s(color, "menu:bordercolor"));
            v.push(s(style, "menu:borderline"));
            if book.draw.is_some() {
                v.push(s("Stop Drawing Borders", "drawborder:off"));
            }
            v
        }
        "bordercolor" => {
            let mut v = vec![s(
                if book.pen.color == [0, 0, 0] {
                    "✓ Automatic"
                } else {
                    "Automatic"
                },
                "bordercolor:auto",
            )];
            for (label, c) in super::chrome::SWATCHES {
                let mark = if book.pen.color == c { "✓ " } else { "" };
                v.push(s(
                    &format!("{mark}{label}"),
                    &format!("bordercolor:{}", hex(c)),
                ));
            }
            v
        }
        "borderline" => Line::ALL
            .iter()
            .map(|l| {
                let mark = if book.pen.line == *l { "✓ " } else { "" };
                s(
                    &format!("{mark}{}", line_name(*l)),
                    &format!("borderline:{}", l.name()),
                )
            })
            .collect(),
        "cf" => vec![
            s("Highlight Cells Rules ›", "menu:cfhighlight"),
            s("Top/Bottom Rules ›", "menu:cftop"),
            s("Data Bars ›", "menu:cfbars"),
            s("Color Scales ›", "menu:cfscales"),
            s("Icon Sets ›", "menu:cficons"),
            sep(),
            s("New Rule…", "cf:formula"),
            s("Clear Rules ›", "menu:cfclear"),
            (
                "Manage Rules…".into(),
                if book.sheet_ref().conditional.is_empty() {
                    Err("this sheet has no conditional formatting rules")
                } else {
                    Ok("cfmanage".into())
                },
            ),
        ],
        "cfhighlight" => vec![
            s("Greater Than…", "cf:greater"),
            s("Less Than…", "cf:less"),
            s("Between…", "cf:between"),
            s("Equal To…", "cf:equal"),
            s("Text that Contains…", "cf:text"),
            s("A Date Occurring…", "cf:date"),
            s("Duplicate Values…", "cf:duplicate"),
        ],
        "cftop" => vec![
            s("Top 10 Items…", "cf:top"),
            s("Top 10%…", "cf:toppct"),
            s("Bottom 10 Items…", "cf:bottom"),
            s("Bottom 10%…", "cf:bottompct"),
            s("Above Average…", "cf:above"),
            s("Below Average…", "cf:below"),
        ],
        "cfbars" => BAR_COLORS
            .iter()
            .map(|(n, c)| s(&format!("{n} Data Bar"), &format!("cf:bar:{}", hex(*c))))
            .collect(),
        "cfscales" => COLOR_SCALES
            .iter()
            .map(|(id, label, _)| s(&format!("{label} Color Scale"), &format!("cf:scale:{id}")))
            .collect(),
        "cficons" => ICON_SETS
            .iter()
            .map(|(set, _)| s(icon_set_name(set), &format!("cf:icons:{set}")))
            .collect(),
        "cfclear" => vec![
            s("Clear Rules from Selected Cells", "cfclear:selection"),
            s("Clear Rules from Entire Sheet", "cfclear:sheet"),
        ],
        "pivotmenu" => vec![
            s("Insert or Edit…", "pivot:new"),
            in_pivot("Refresh", "pivot:refresh"),
            in_pivot("Delete", "pivot:delete"),
        ],
        other => {
            if let Some(k) = other.strip_prefix("pivotitems:") {
                let k: usize = k.parse().ok()?;
                let i = book.current_pivot()?;
                let p = &book.workbook.sheets[book.sheet].pivots[i];
                let si = book.workbook.sheet_index(&p.source_sheet)?;
                let mut items: Vec<cw_sheet::Value> = Vec::new();
                for r in p.source.start.row + 1..=p.source.end.row {
                    let v = book
                        .workbook
                        .value(si, cw_sheet::Cell::new(r, p.source.start.col + k as u32));
                    if !items.contains(&v) {
                        items.push(v);
                    }
                }
                items.sort_by(cw_sheet::value::compare);
                let hidden = p.hidden.get(&k).cloned().unwrap_or_default();
                return Some(
                    items
                        .iter()
                        .map(|v| {
                            let label = cw_sheet::pivot::item_label(v);
                            let mark = if hidden.contains(&label) {
                                "☐"
                            } else {
                                "☑"
                            };
                            (
                                format!("{mark} {label}"),
                                Ok(format!("pivothide:{k}:{label}")),
                            )
                        })
                        .collect(),
                );
            }
            if let Some(i) = other.strip_prefix("pivotvalue:") {
                let i: usize = i.parse().ok()?;
                return Some(
                    Agg::ALL
                        .iter()
                        .map(|a| {
                            let name = match a {
                                Agg::Sum => "Sum",
                                Agg::Count => "Count",
                                Agg::Average => "Average",
                                Agg::Max => "Max",
                                Agg::Min => "Min",
                            };
                            s(name, &format!("pivotagg:{i}:{}", a.name()))
                        })
                        .collect(),
                );
            }
            return None;
        }
    })
}

/// A clickable choice chip: selected ones are filled with the accent.
fn chip(p: &mut Painter, r: Rect, label: &str, target: &str, on: bool, accent: Color) {
    p.button(
        r,
        if on {
            Color(accent.0, accent.1, accent.2, 40)
        } else {
            Color::WHITE
        },
        4,
        &format!("sheet:{target}"),
        label,
    );
    p.border(r, Color::TRANSPARENT, 4, if on { accent } else { LINE });
    p.label(
        r.x + 4,
        r.y + (r.height as i32 - 16) / 2,
        r.width.saturating_sub(8),
        label,
        12,
        INK,
        on,
        Align::Center,
    );
}
fn field(
    p: &mut Painter,
    r: Rect,
    text: &str,
    focused: bool,
    target: &str,
    label: &str,
    accent: Color,
) {
    p.border(r, Color::WHITE, 3, if focused { accent } else { LINE });
    p.region(r, &format!("sheet:{target}"), label);
    p.left(
        r.x + 6,
        r.y + (r.height as i32 - 17) / 2,
        r.width.saturating_sub(12),
        text,
        12,
        INK,
    );
    if focused {
        let cx = r.x + 6 + p.measure(text, 12, false) as i32;
        p.vline(cx, r.y + 4, r.height.saturating_sub(8), INK);
    }
}
fn buttons(p: &mut Painter, d: Rect, accent: Color, ok: &str) {
    let okr = Rect::new(
        d.x + d.width as i32 - 186,
        d.y + d.height as i32 - 42,
        84,
        28,
    );
    let cancel = Rect::new(
        d.x + d.width as i32 - 94,
        d.y + d.height as i32 - 42,
        80,
        28,
    );
    p.button(okr, accent, 4, "sheet:dialog:ok", ok);
    p.label(
        okr.x,
        okr.y + 6,
        okr.width,
        ok,
        13,
        Color::WHITE,
        true,
        Align::Center,
    );
    p.button(cancel, Color::WHITE, 4, "sheet:dialog:cancel", "Cancel");
    p.border(cancel, Color::TRANSPARENT, 4, LINE);
    p.label(
        cancel.x,
        cancel.y + 6,
        cancel.width,
        "Cancel",
        13,
        INK,
        false,
        Align::Center,
    );
}
/// The open dialog, over everything else.
pub fn paint_dialog(book: &Book, p: &mut Painter, w: u32, h: u32, accent: Color) {
    let Some(dialog) = &book.dialog else {
        return;
    };
    let (dw, dh) = match dialog {
        SheetDialog::Rule { kind, .. } if kind == "date" => (480, 250),
        SheetDialog::Rule { .. } => (480, 220),
        SheetDialog::Rules { .. } => (620, 360),
        SheetDialog::Pivot { .. } => (460, 250),
        SheetDialog::TextToColumns { .. } => (440, 210),
        SheetDialog::Confirm { .. } => (400, 150),
    };
    let dw = dw.min(w.saturating_sub(16));
    let d = Rect::new(
        (w as i32 - dw as i32) / 2,
        (h as i32 - dh as i32) / 3,
        dw,
        dh,
    );
    p.box_(Rect::new(0, 0, w, h), Color(0, 0, 0, 40), 0);
    p.region(Rect::new(0, 0, w, h), "sheet:noop", "Dialog");
    p.drop_shadow(d, 8, 16, 70, 4);
    p.box_(d, Color::WHITE, 8);
    let (x, inner) = (d.x + 18, dw.saturating_sub(36));
    match dialog {
        SheetDialog::Rule {
            kind,
            fields,
            focus,
            preset,
            choice,
        } => {
            let (title, prompt) = rule_title(kind);
            p.strong(x, d.y + 14, inner, title, 14, INK);
            p.left(x, d.y + 44, inner, prompt, 12, INK);
            let mut y = d.y + 68;
            match kind.as_str() {
                "date" => {
                    for (i, per) in Period::ALL.iter().enumerate() {
                        let cw = (inner as i32 - 16) / 5;
                        let r = Rect::new(
                            x + (i as i32 % 5) * (cw + 4),
                            y + (i as i32 / 5) * 30,
                            cw as u32,
                            26,
                        );
                        chip(
                            p,
                            r,
                            &period_label(*per),
                            &format!("dialog:choice:{}", per.name()),
                            choice == per.name(),
                            accent,
                        );
                    }
                    y += 66;
                }
                "duplicate" => {
                    for (i, (label, v)) in [("Duplicate", "duplicate"), ("Unique", "unique")]
                        .iter()
                        .enumerate()
                    {
                        let r = Rect::new(x + i as i32 * 110, y, 100, 26);
                        chip(
                            p,
                            r,
                            label,
                            &format!("dialog:choice:{v}"),
                            choice == v,
                            accent,
                        );
                    }
                    y += 34;
                }
                _ => {
                    let n = fields.len().max(1) as i32;
                    let fw = ((inner as i32 - 150 - 30 * (n - 1)) / n).max(60) as u32;
                    for (i, f) in fields.iter().enumerate() {
                        let r = Rect::new(x + i as i32 * (fw as i32 + 30), y, fw, 26);
                        field(
                            p,
                            r,
                            f,
                            *focus == i,
                            &format!("dialog:field:{i}"),
                            "Value",
                            accent,
                        );
                        if i + 1 < fields.len() {
                            p.label(
                                r.x + fw as i32,
                                y + 5,
                                30,
                                "and",
                                12,
                                MUTED,
                                false,
                                Align::Center,
                            );
                        }
                    }
                    if kind.starts_with("top") || kind.starts_with("bottom") {
                        let unit = if kind.ends_with("pct") { "%" } else { "items" };
                        p.left(x + fw as i32 + 8, y + 5, 60, unit, 12, MUTED);
                    }
                    y += 34;
                }
            }
            p.left(x, y + 4, 40, "with", 12, INK);
            // The format presets, as Excel's drop-down lists them.
            for (i, (id, label)) in PRESETS.iter().enumerate() {
                let r = Rect::new(
                    x + 44 + (i as i32 % 2) * 210,
                    y + (i as i32 / 2) * 28,
                    204,
                    24,
                );
                let dxf = cw_sheet::conditional::Dxf::preset(id).unwrap_or_default();
                let on = preset == id;
                p.button(
                    r,
                    dxf.fill
                        .map_or(Color::WHITE, |c| Color::rgb(c[0], c[1], c[2])),
                    3,
                    &format!("sheet:dialog:preset:{id}"),
                    label,
                );
                p.border(r, Color::TRANSPARENT, 3, if on { accent } else { LINE });
                let ink = dxf.color.map_or(INK, |c| Color::rgb(c[0], c[1], c[2]));
                p.label(
                    r.x + 4,
                    r.y + 4,
                    r.width - 8,
                    label,
                    11,
                    ink,
                    on,
                    Align::Center,
                );
            }
            buttons(p, d, accent, "OK");
        }
        SheetDialog::Rules { selected } => {
            p.strong(
                x,
                d.y + 14,
                inner,
                "Conditional Formatting Rules Manager",
                14,
                INK,
            );
            p.left(
                x,
                d.y + 42,
                inner,
                &format!(
                    "Showing formatting rules for: This Worksheet ({})",
                    book.sheet_ref().name
                ),
                12,
                MUTED,
            );
            let tools = [
                ("Delete Rule", "cfdelete"),
                ("▲ Move Up", "cfup"),
                ("▼ Move Down", "cfdown"),
            ];
            let n = book.sheet_ref().conditional.len();
            for (i, (label, t)) in tools.iter().enumerate() {
                let r = Rect::new(x + i as i32 * 108, d.y + 64, 100, 26);
                let why = match (selected, *t) {
                    (None, _) => Some("select a rule first"),
                    (Some(0), "cfup") => Some("that rule is already first"),
                    (Some(k), "cfdown") if k + 1 >= n => Some("that rule is already last"),
                    _ => None,
                };
                let live = why.is_none();
                p.button(r, Color::WHITE, 4, &format!("sheet:{t}"), label);
                if let Some(why) = why {
                    p.disabled(why);
                }
                p.border(r, Color::TRANSPARENT, 4, LINE);
                p.label(
                    r.x,
                    r.y + 5,
                    r.width,
                    label,
                    12,
                    if live { INK } else { FAINT },
                    false,
                    Align::Center,
                );
            }
            // Columns: Rule (applied in order shown), Format, Applies to, Stop If True.
            let ty = d.y + 100;
            p.box_(Rect::new(x, ty, inner, 24), Color::rgb(243, 243, 243), 0);
            for (label, at) in [
                ("Rule (applied in order shown)", 0),
                ("Format", 250),
                ("Applies to", 360),
                ("Stop If True", 480),
            ] {
                p.left(x + 6 + at, ty + 4, 150, label, 11, MUTED);
            }
            for (i, cf) in book.sheet_ref().conditional.iter().enumerate().take(8) {
                let r = Rect::new(x, ty + 26 + i as i32 * 26, inner, 24);
                if *selected == Some(i) {
                    p.box_(r, Color(accent.0, accent.1, accent.2, 36), 0);
                }
                p.region(r, &format!("sheet:cfrule:{i}"), &cf.rule.describe());
                p.left(r.x + 6, r.y + 4, 240, &cf.rule.describe(), 12, INK);
                let sw = Rect::new(r.x + 256, r.y + 3, 90, 18);
                match cf.rule.style() {
                    Some(st) => {
                        p.box_(
                            sw,
                            st.fill
                                .map_or(Color::WHITE, |c| Color::rgb(c[0], c[1], c[2])),
                            0,
                        );
                        p.border(sw, Color::TRANSPARENT, 0, LINE);
                        p.label(
                            sw.x,
                            sw.y + 1,
                            sw.width,
                            "AaBbCcYyZz",
                            11,
                            st.color.map_or(INK, |c| Color::rgb(c[0], c[1], c[2])),
                            st.bold,
                            Align::Center,
                        );
                    }
                    None => {
                        p.left(sw.x, sw.y + 1, sw.width, "(graphic)", 11, MUTED);
                    }
                }
                p.left(
                    r.x + 366,
                    r.y + 4,
                    110,
                    &format!("={}", cf.sqref()),
                    12,
                    INK,
                );
                let cb = Rect::new(r.x + 500, r.y + 4, 16, 16);
                p.border(cb, Color::WHITE, 2, MUTED);
                if cf.stop_if_true {
                    p.symbol("check", cb.x + 1, cb.y + 1, 14, accent);
                }
                if *selected == Some(i) {
                    p.region(cb, "sheet:cfstop", "Stop If True");
                }
            }
            let okr = Rect::new(
                d.x + d.width as i32 - 94,
                d.y + d.height as i32 - 42,
                80,
                28,
            );
            p.button(okr, accent, 4, "sheet:dialog:ok", "Close");
            p.label(
                okr.x,
                okr.y + 6,
                okr.width,
                "Close",
                13,
                Color::WHITE,
                true,
                Align::Center,
            );
        }
        SheetDialog::Pivot {
            source,
            new_sheet,
            place,
            focus,
        } => {
            p.strong(x, d.y + 14, inner, "Create PivotTable", 14, INK);
            p.left(x, d.y + 44, inner, "Table/Range:", 12, INK);
            field(
                p,
                Rect::new(x + 100, d.y + 40, inner - 100, 26),
                source,
                *focus == 0,
                "dialog:field:0",
                "Table/Range",
                accent,
            );
            p.left(
                x,
                d.y + 82,
                inner,
                "Choose where you want the PivotTable report to be placed:",
                12,
                INK,
            );
            chip(
                p,
                Rect::new(x, d.y + 106, 140, 26),
                "New Worksheet",
                "dialog:newsheet",
                *new_sheet,
                accent,
            );
            chip(
                p,
                Rect::new(x + 150, d.y + 106, 150, 26),
                "Existing Worksheet",
                "dialog:existing",
                !*new_sheet,
                accent,
            );
            p.left(
                x,
                d.y + 150,
                90,
                "Location:",
                12,
                if *new_sheet { FAINT } else { INK },
            );
            field(
                p,
                Rect::new(x + 100, d.y + 146, inner - 100, 26),
                place,
                *focus == 1,
                "dialog:field:1",
                "Location",
                accent,
            );
            buttons(p, d, accent, "OK");
        }
        SheetDialog::TextToColumns {
            delimiter,
            merge_runs,
        } => {
            p.strong(x, d.y + 14, inner, "Convert Text to Columns", 14, INK);
            p.left(x, d.y + 44, inner, "Delimiters:", 12, INK);
            for (i, (label, v)) in [
                ("Tab", "tab"),
                ("Semicolon", "semicolon"),
                ("Comma", "comma"),
                ("Space", "space"),
            ]
            .iter()
            .enumerate()
            {
                chip(
                    p,
                    Rect::new(x + i as i32 * 100, d.y + 66, 92, 26),
                    label,
                    &format!("dialog:delim:{v}"),
                    delimiter == v,
                    accent,
                );
            }
            chip(
                p,
                Rect::new(x, d.y + 104, 260, 26),
                "Treat consecutive delimiters as one",
                "dialog:mergeruns",
                *merge_runs,
                accent,
            );
            buttons(p, d, accent, "Finish");
        }
        SheetDialog::Confirm { message, .. } => {
            p.paragraph(x, d.y + 20, inner, message, 13, INK);
            buttons(p, d, accent, "OK");
        }
    }
}
fn period_label(p: Period) -> String {
    match p {
        Period::Yesterday => "Yesterday",
        Period::Today => "Today",
        Period::Tomorrow => "Tomorrow",
        Period::Last7Days => "Last 7 days",
        Period::LastWeek => "Last week",
        Period::ThisWeek => "This week",
        Period::NextWeek => "Next week",
        Period::LastMonth => "Last month",
        Period::ThisMonth => "This month",
        Period::NextMonth => "Next month",
    }
    .into()
}
/// The pivot table field list, in `r`, when the active cell is in a pivot table.
/// Returns where each of its menus drops from.
pub fn pivot_pane(
    book: &Book,
    p: &mut Painter,
    r: Rect,
    flavor: Flavor,
    accent: Color,
) -> Vec<(String, i32, i32)> {
    let mut anchors = Vec::new();
    let Some(i) = book.current_pivot() else {
        return anchors;
    };
    let pv = &book.workbook.sheets[book.sheet].pivots[i];
    let Some(si) = book.workbook.sheet_index(&pv.source_sheet) else {
        return anchors;
    };
    let names = cw_sheet::pivot::fields(&book.workbook, si, pv.source).unwrap_or_default();
    p.box_(r, Color::rgb(248, 248, 248), 0);
    p.vline(r.x, r.y, r.height, LINE);
    let title = match flavor {
        Flavor::Excel => "PivotTable Fields",
        Flavor::Calc => "Pivot Table Layout",
        Flavor::Numbers => "Pivot Options",
        Flavor::Sheets => "Pivot table editor",
    };
    let (x, inner) = (r.x + 12, r.width.saturating_sub(24));
    p.strong(x, r.y + 10, inner.saturating_sub(24), title, 14, INK);
    let close = Rect::new(r.x + r.width as i32 - 30, r.y + 8, 22, 22);
    p.button(
        close,
        Color::TRANSPARENT,
        4,
        "sheet:pivot:pane",
        "Close the field list",
    );
    p.symbol("close", close.x + 3, close.y + 3, 16, MUTED);
    p.left(
        x,
        r.y + 38,
        inner,
        "Choose fields to add to report:",
        11,
        MUTED,
    );
    let mut y = r.y + 58;
    let area_of = |k: usize| -> Option<&'static str> {
        if pv.rows.contains(&k) {
            Some("rows")
        } else if pv.cols.contains(&k) {
            Some("cols")
        } else if pv.filters.contains(&k) {
            Some("filters")
        } else if pv.values.iter().any(|(f, _)| *f == k) {
            Some("values")
        } else {
            None
        }
    };
    for (k, name) in names.iter().enumerate().take(12) {
        let used = area_of(k).is_some();
        let row = Rect::new(x, y, inner, 24);
        let cb = Rect::new(x, y + 4, 16, 16);
        p.border(cb, Color::WHITE, 2, MUTED);
        if used {
            p.symbol("check", cb.x + 1, cb.y + 1, 14, accent);
        }
        p.region(
            Rect::new(x, y, inner.saturating_sub(120), 24),
            &format!("sheet:pivotfield:{k}"),
            name,
        );
        p.left(x + 22, y + 3, inner.saturating_sub(150), name, 12, INK);
        // Move it to an area: Filters, Columns, Rows, Values.
        for (j, (label, area)) in [
            ("F", "filters"),
            ("C", "cols"),
            ("R", "rows"),
            ("Σ", "values"),
        ]
        .iter()
        .enumerate()
        {
            let b = Rect::new(
                row.x + row.width as i32 - 116 + j as i32 * 24,
                y + 1,
                22,
                22,
            );
            let on = area_of(k) == Some(*area);
            let target = if *area == "cols" && !pv.cols.is_empty() && !pv.cols.contains(&k) {
                None
            } else {
                Some(format!("sheet:pivotarea:{k}:{area}"))
            };
            match &target {
                Some(t) => p.button(
                    b,
                    if on {
                        Color(accent.0, accent.1, accent.2, 50)
                    } else {
                        Color::WHITE
                    },
                    3,
                    t,
                    label,
                ),
                None => {
                    p.region(b, "sheet:noop", label);
                    p.disabled("this pivot table takes one column field; take the other out first");
                }
            }
            p.border(b, Color::TRANSPARENT, 3, LINE);
            p.label(
                b.x,
                b.y + 3,
                b.width,
                label,
                11,
                if target.is_some() { INK } else { FAINT },
                on,
                Align::Center,
            );
        }
        y += 26;
    }
    y += 8;
    let section = |p: &mut Painter, y: &mut i32, label: &str| {
        p.hline(x, *y, inner, LINE);
        p.strong(x, *y + 6, inner, label, 12, INK);
        *y += 28;
    };
    let entry = |p: &mut Painter,
                 y: &mut i32,
                 k: usize,
                 text: &str,
                 anchors: &mut Vec<(String, i32, i32)>,
                 menu: Option<String>| {
        let e = Rect::new(x, *y, inner, 24);
        p.border(e, Color::WHITE, 3, LINE);
        if let Some(m) = &menu {
            p.region(
                Rect::new(e.x, e.y, e.width.saturating_sub(28), 24),
                &format!("sheet:menu:{m}"),
                text,
            );
            p.symbol(
                "chevron-down",
                e.x + e.width as i32 - 44,
                e.y + 5,
                14,
                MUTED,
            );
            anchors.push((m.clone(), e.x, e.y + 24));
        }
        p.left(e.x + 6, e.y + 3, e.width.saturating_sub(56), text, 12, INK);
        let rm = Rect::new(e.x + e.width as i32 - 24, e.y + 2, 20, 20);
        p.button(
            rm,
            Color::TRANSPARENT,
            3,
            &format!("sheet:pivotarea:{k}:remove"),
            "Remove field",
        );
        p.symbol("close", rm.x + 3, rm.y + 3, 14, MUTED);
        *y += 28;
    };
    section(p, &mut y, "Filters");
    for f in &pv.filters {
        entry(
            p,
            &mut y,
            *f,
            &names[*f],
            &mut anchors,
            Some(format!("pivotitems:{f}")),
        );
    }
    section(p, &mut y, "Columns");
    for f in &pv.cols {
        entry(
            p,
            &mut y,
            *f,
            &names[*f],
            &mut anchors,
            Some(format!("pivotitems:{f}")),
        );
    }
    section(p, &mut y, "Rows");
    for f in &pv.rows {
        entry(
            p,
            &mut y,
            *f,
            &names[*f],
            &mut anchors,
            Some(format!("pivotitems:{f}")),
        );
    }
    section(p, &mut y, "Values");
    for (vi, (f, agg)) in pv.values.iter().enumerate() {
        entry(
            p,
            &mut y,
            *f,
            &pv.caption(&names[*f], *agg),
            &mut anchors,
            Some(format!("pivotvalue:{vi}")),
        );
    }
    // Refresh and delete at the foot.
    let by = (r.y + r.height as i32 - 40).max(y + 6);
    for (j, (label, t)) in [("Refresh", "pivot:refresh"), ("Delete", "pivot:delete")]
        .iter()
        .enumerate()
    {
        let b = Rect::new(x + j as i32 * 96, by, 88, 28);
        p.button(
            b,
            if j == 0 { accent } else { Color::WHITE },
            4,
            &format!("sheet:{t}"),
            label,
        );
        if j == 1 {
            p.border(b, Color::TRANSPARENT, 4, LINE);
        }
        p.label(
            b.x,
            b.y + 6,
            b.width,
            label,
            12,
            if j == 0 { Color::WHITE } else { INK },
            true,
            Align::Center,
        );
    }

    anchors
}
