//! Merged cells, borders, conditional formatting, pivot tables, Text to Columns and
//! Calculate Now: the commands, and the dialogs some of them open.
use super::{Book, DrawMode, Flavor, SheetDialog};
use crate::AppEffect;
use cw_sheet::conditional::{CellOp, Cfvo, CondFormat, Dxf, Period, Rule, TextOp, ICON_SETS};
use cw_sheet::pivot::{Agg, PivotStyle};
use cw_sheet::{BorderPreset, Cell, Edge, Line, MergeMode, Range, Value};

/// Whether typing goes to the dialog (it has a text field with focus).
pub(super) fn dialog_takes_text(d: &SheetDialog) -> bool {
    match d {
        SheetDialog::Rule { fields, .. } => !fields.is_empty(),
        SheetDialog::Pivot { .. } => true,
        _ => false,
    }
}
/// Excel's colour scale presets: (name, colours low to high).
pub const COLOR_SCALES: [(&str, &str, &[[u8; 3]]); 12] = [
    (
        "gyr",
        "Green - Yellow - Red",
        &[[99, 190, 123], [255, 235, 132], [248, 105, 107]],
    ),
    (
        "ryg",
        "Red - Yellow - Green",
        &[[248, 105, 107], [255, 235, 132], [99, 190, 123]],
    ),
    (
        "gwr",
        "Green - White - Red",
        &[[99, 190, 123], [252, 252, 255], [248, 105, 107]],
    ),
    (
        "rwg",
        "Red - White - Green",
        &[[248, 105, 107], [252, 252, 255], [99, 190, 123]],
    ),
    (
        "bwr",
        "Blue - White - Red",
        &[[90, 138, 198], [252, 252, 255], [248, 105, 107]],
    ),
    (
        "rwb",
        "Red - White - Blue",
        &[[248, 105, 107], [252, 252, 255], [90, 138, 198]],
    ),
    ("wr", "White - Red", &[[252, 252, 255], [248, 105, 107]]),
    ("rw", "Red - White", &[[248, 105, 107], [252, 252, 255]]),
    ("gw", "Green - White", &[[99, 190, 123], [252, 252, 255]]),
    ("wg", "White - Green", &[[252, 252, 255], [99, 190, 123]]),
    ("gy", "Green - Yellow", &[[99, 190, 123], [255, 239, 156]]),
    ("yg", "Yellow - Green", &[[255, 239, 156], [99, 190, 123]]),
];
/// Excel's data bar colours (gradient fill).
pub const BAR_COLORS: [(&str, [u8; 3]); 6] = [
    ("Blue", [99, 142, 198]),
    ("Green", [99, 190, 123]),
    ("Red", [248, 105, 107]),
    ("Orange", [255, 182, 40]),
    ("Light Blue", [0, 138, 239]),
    ("Purple", [214, 0, 123]),
];
/// The highlight format presets, as Excel's dialogs name them.
pub const PRESETS: [(&str, &str); 5] = [
    ("lightred", "Light Red Fill with Dark Red Text"),
    ("yellow", "Yellow Fill with Dark Yellow Text"),
    ("green", "Green Fill with Dark Green Text"),
    ("redfill", "Light Red Fill"),
    ("redtext", "Red Text"),
];
/// What each rule dialog is called and asks.
pub fn rule_title(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "greater" => ("Greater Than", "Format cells that are GREATER THAN:"),
        "less" => ("Less Than", "Format cells that are LESS THAN:"),
        "between" => ("Between", "Format cells that are BETWEEN:"),
        "equal" => ("Equal To", "Format cells that are EQUAL TO:"),
        "text" => ("Text That Contains", "Format cells that contain the text:"),
        "date" => (
            "A Date Occurring",
            "Format cells that contain a date occurring:",
        ),
        "duplicate" => ("Duplicate Values", "Format cells that contain:"),
        "top" => ("Top 10 Items", "Format cells that rank in the TOP:"),
        "bottom" => ("Bottom 10 Items", "Format cells that rank in the BOTTOM:"),
        "toppct" => ("Top 10%", "Format cells that rank in the TOP:"),
        "bottompct" => ("Bottom 10%", "Format cells that rank in the BOTTOM:"),
        "above" => ("Above Average", "Format cells that are ABOVE AVERAGE:"),
        "below" => ("Below Average", "Format cells that are BELOW AVERAGE:"),
        _ => (
            "New Formatting Rule",
            "Format values where this formula is true:",
        ),
    }
}
fn points(n: usize) -> Vec<Cfvo> {
    (0..n)
        .map(|i| Cfvo::Percent(if i == 0 { 0.0 } else { (100 * i / n) as f64 }))
        .collect()
}
fn strip_eq(s: &str) -> String {
    s.trim().trim_start_matches('=').trim().to_owned()
}

impl Book {
    /// The part of the selection a whole-row or whole-column selection really covers.
    fn bounded_selection(&self) -> Range {
        let sel = self.selection();
        match self.workbook.used_range(self.sheet) {
            Some(u) if u64::from(sel.rows()) * u64::from(sel.cols()) > 100_000 => {
                sel.intersect(&u).unwrap_or(Range::single(sel.start))
            }
            _ => sel,
        }
    }
    fn open_rule_dialog(&mut self, kind: &str) -> Result<(), String> {
        let sel = self.bounded_selection();
        let stats = self.workbook.stats(self.sheet, sel);
        let g = |x: f64| cw_sheet::value::general(cw_sheet::format::round_decimal(x, 2));
        let fields: Vec<String> = match kind {
            // Excel suggests values from the selection.
            "greater" | "less" | "equal" => vec![stats.average.map(g).unwrap_or_default()],
            "between" => vec![
                stats.min.map(g).unwrap_or_default(),
                stats.max.map(g).unwrap_or_default(),
            ],
            "text" => vec![String::new()],
            "top" | "bottom" | "toppct" | "bottompct" => vec!["10".into()],
            "formula" => vec![String::new()],
            "date" | "duplicate" | "above" | "below" => vec![],
            other => return Err(format!("unknown conditional formatting rule {other}")),
        };
        let choice = match kind {
            "date" => "yesterday",
            "duplicate" => "duplicate",
            _ => "",
        };
        self.dialog = Some(SheetDialog::Rule {
            kind: kind.into(),
            fields,
            focus: 0,
            preset: "lightred".into(),
            choice: choice.into(),
        });
        Ok(())
    }
    /// The rule the open dialog describes, for the selection.
    fn rule_from_dialog(&self) -> Result<CondFormat, String> {
        let Some(SheetDialog::Rule {
            kind,
            fields,
            preset,
            choice,
            ..
        }) = &self.dialog
        else {
            return Err("no rule dialog is open".into());
        };
        let style = Dxf::preset(preset).ok_or("choose a format")?;
        let f = |i: usize| strip_eq(fields.get(i).map_or("", String::as_str));
        let need = |s: String| {
            if s.is_empty() {
                Err("Enter a value.".to_string())
            } else {
                Ok(s)
            }
        };
        let range = self.bounded_selection();
        let rank = || -> Result<u32, String> {
            f(0).parse::<u32>()
                .map_err(|_| "Enter a whole number between 1 and 1000.".to_string())
        };
        let rule = match kind.as_str() {
            "greater" | "less" | "equal" => Rule::CellIs {
                op: match kind.as_str() {
                    "greater" => CellOp::Greater,
                    "less" => CellOp::Less,
                    _ => CellOp::Equal,
                },
                formulas: vec![quote_text(&need(f(0))?)],
                style,
            },
            "between" => Rule::CellIs {
                op: CellOp::Between,
                formulas: vec![quote_text(&need(f(0))?), quote_text(&need(f(1))?)],
                style,
            },
            "text" => Rule::Text {
                op: TextOp::Contains,
                text: need(fields[0].clone())?,
                style,
            },
            "date" => Rule::Dates {
                period: Period::parse(choice).ok_or("choose when the date occurs")?,
                style,
            },
            "duplicate" => Rule::Duplicates {
                unique: choice == "unique",
                style,
            },
            "top" | "bottom" | "toppct" | "bottompct" => Rule::Top {
                bottom: kind.starts_with("bottom"),
                rank: rank()?,
                percent: kind.ends_with("pct"),
                style,
            },
            "above" | "below" => Rule::Average {
                below: kind == "below",
                equal: false,
                style,
            },
            _ => {
                // Written for the active cell; stored for the range's top-left.
                let text = need(f(0))?;
                let parsed = cw_sheet::parser::Formula::parse(&text)
                    .map_err(|e| format!("There's a problem with this formula: {e}"))?;
                let moved = cw_sheet::Workbook::shift_formula(
                    &parsed,
                    i64::from(range.start.row) - i64::from(self.active.row),
                    i64::from(range.start.col) - i64::from(self.active.col),
                );
                Rule::Expression {
                    formula: moved.text(),
                    style,
                }
            }
        };
        Ok(CondFormat::new(range, rule))
    }
    /// OK: what the dialog set up happens, or a message box says what is wrong and the
    /// dialog stays open, as the products do.
    fn dialog_ok(
        &mut self,
        window: u64,
        clock_us: u64,
        flavor: Flavor,
    ) -> Result<Vec<AppEffect>, String> {
        if self.dialog.is_none() {
            return Err("no dialog is open".into());
        }
        match self.dialog_accept(window, clock_us, flavor) {
            Err(problem) => {
                self.message = Some(problem);
                Ok(vec![])
            }
            ok => ok,
        }
    }
    fn dialog_accept(
        &mut self,
        window: u64,
        clock_us: u64,
        flavor: Flavor,
    ) -> Result<Vec<AppEffect>, String> {
        match self.dialog.clone() {
            None => Err("no dialog is open".into()),
            Some(SheetDialog::Rule { .. }) => {
                let cf = self.rule_from_dialog()?;
                self.workbook.add_conditional(self.sheet, cf)?;
                self.dialog = None;
                self.modified = true;
                Ok(vec![])
            }
            Some(SheetDialog::Rules { .. }) => {
                self.dialog = None;
                Ok(vec![])
            }
            Some(SheetDialog::Pivot {
                source,
                new_sheet,
                place,
                ..
            }) => {
                self.create_pivot(&source, new_sheet, &place, flavor)?;
                self.dialog = None;
                Ok(vec![])
            }
            Some(SheetDialog::TextToColumns {
                delimiter,
                merge_runs,
            }) => {
                let d = match delimiter.as_str() {
                    "tab" => '\t',
                    "semicolon" => ';',
                    "space" => ' ',
                    _ => ',',
                };
                self.workbook.text_to_columns(
                    self.sheet,
                    self.bounded_selection(),
                    d,
                    merge_runs,
                )?;
                self.dialog = None;
                self.modified = true;
                Ok(vec![])
            }
            Some(SheetDialog::Confirm { then, .. }) => {
                self.dialog = None;
                self.command(window, &then, clock_us, flavor)
            }
        }
    }
    fn create_pivot(
        &mut self,
        source: &str,
        new_sheet: bool,
        place: &str,
        flavor: Flavor,
    ) -> Result<(), String> {
        let reference = |text: &str| -> Result<(usize, Range), String> {
            let t = text.trim().trim_start_matches('=');
            let (sheet, body) = match t.rsplit_once('!') {
                Some((s, b)) => {
                    let name = s.trim_matches('\'').replace("''", "'");
                    (
                        self.workbook
                            .sheet_index(&name)
                            .ok_or_else(|| format!("there is no sheet named {name}"))?,
                        b,
                    )
                }
                None => (self.sheet, t),
            };
            Ok((sheet, Range::parse(body).ok_or("Reference isn't valid.")?))
        };
        let (src_sheet, src) = reference(source)?;
        let style = match flavor {
            Flavor::Calc => PivotStyle::Calc,
            Flavor::Sheets => PivotStyle::Sheets,
            _ => PivotStyle::Excel,
        };
        // Sheets and Numbers keep their pivot tables up to date; Excel and Calc refresh
        // on request.
        let auto = matches!(flavor, Flavor::Sheets | Flavor::Numbers);
        let (sheet, at) = if new_sheet {
            let n = self.workbook.sheets.len() + 1;
            let (name, index, at) = match flavor {
                Flavor::Excel => (
                    (1..)
                        .map(|i| format!("Sheet{i}"))
                        .find(|s| self.workbook.sheet_index(s).is_none())
                        .unwrap_or_default(),
                    src_sheet,
                    Cell::new(2, 0),
                ),
                Flavor::Calc => (
                    (1..)
                        .map(|i| {
                            format!("Pivot Table_{}_{i}", self.workbook.sheets[src_sheet].name)
                        })
                        .find(|s| self.workbook.sheet_index(s).is_none())
                        .unwrap_or_default(),
                    src_sheet,
                    Cell::new(0, 0),
                ),
                _ => (
                    (1..)
                        .map(|i| format!("Pivot Table {i}"))
                        .find(|s| self.workbook.sheet_index(s).is_none())
                        .unwrap_or_default(),
                    n - 1,
                    Cell::new(0, 0),
                ),
            };
            let at_index = self.workbook.insert_sheet(index, &name)?;
            (at_index, at)
        } else {
            reference(place)
                .map(|(s, r)| (s, r.start))
                .map_err(|_| "Enter a destination cell for the PivotTable.".to_string())?
        };
        // The source moved one place right if the new sheet went in before it.
        let src_sheet = if new_sheet && sheet <= src_sheet {
            src_sheet + 1
        } else {
            src_sheet
        };
        if let Err(e) = self
            .workbook
            .add_pivot(src_sheet, src, sheet, at, style, auto)
        {
            if new_sheet {
                let _ = self.workbook.undo();
            }
            return Err(e);
        }
        self.sheet = sheet;
        self.active = at;
        self.anchor = at;
        self.scroll = (0, 0);
        self.pivot_pane_closed = false;
        self.modified = true;
        Ok(())
    }
    /// The pivot table the active cell is in.
    pub fn current_pivot(&self) -> Option<usize> {
        self.workbook.pivot_at(self.sheet, self.active)
    }
    fn pivot_edit(
        &mut self,
        f: impl FnOnce(&mut cw_sheet::pivot::Pivot) -> Result<(), String>,
    ) -> Result<(), String> {
        let i = self
            .current_pivot()
            .ok_or("select a cell in the PivotTable first")?;
        self.workbook.edit_pivot(self.sheet, i, f)?;
        self.modified = true;
        Ok(())
    }
    /// Whether a source field holds numbers (it then goes to Values when checked).
    fn numeric_field(&self, k: usize) -> bool {
        let Some(i) = self.current_pivot() else {
            return false;
        };
        let p = &self.workbook.sheets[self.sheet].pivots[i];
        let Some(si) = self.workbook.sheet_index(&p.source_sheet) else {
            return false;
        };
        (p.source.start.row + 1..=p.source.end.row).any(|r| {
            matches!(
                self.workbook
                    .value(si, Cell::new(r, p.source.start.col + k as u32)),
                Value::Number(_)
            )
        })
    }
    /// Commands for this module's features; `None` when `verb` is not one of them.
    pub(super) fn feature_command(
        &mut self,
        window: u64,
        verb: &str,
        arg: &str,
        clock_us: u64,
        flavor: Flavor,
    ) -> Option<Result<Vec<AppEffect>, String>> {
        let done = |r: Result<(), String>| Some(r.map(|()| vec![]));
        match verb {
            "merge" | "merge!" => {
                self.commit_edit();
                let sel = self.selection();
                let mode = match arg {
                    "across" => MergeMode::Across,
                    "down" => MergeMode::Down,
                    _ => MergeMode::All,
                };
                let center = arg == "center";
                // Merge & Center on a merged cell unmerges it, as the button toggles.
                if center && self.sheet_ref().merges.contains(&sel) {
                    return done(self.workbook.unmerge(self.sheet, sel).map(|()| {
                        self.modified = true;
                    }));
                }
                if verb == "merge" && self.workbook.merge_loses_data(self.sheet, sel, mode) {
                    let message = match flavor {
                        Flavor::Calc => "Some cells are not empty. Merging keeps the contents of the first cell and empties the hidden cells.",
                        Flavor::Sheets => "Merging cells only preserves the top-leftmost value. Merge anyway?",
                        _ => "Merging cells only keeps the upper-left value and discards other values.",
                    };
                    self.dialog = Some(SheetDialog::Confirm {
                        message: message.into(),
                        then: format!("merge!:{arg}"),
                    });
                    return Some(Ok(vec![]));
                }
                let r = self.workbook.merge(self.sheet, sel, mode, center);
                if r.is_ok() {
                    self.modified = true;
                    // The merged cell is the selection, and active.
                    let s = self.selection();
                    self.anchor = s.end;
                    self.active = s.start;
                }
                return done(r);
            }
            "unmerge" => {
                self.commit_edit();
                let r = self.workbook.unmerge(self.sheet, self.selection());
                if r.is_ok() {
                    self.modified = true;
                }
                return done(r);
            }
            "border" => {
                self.commit_edit();
                let Some(preset) = BorderPreset::parse(arg) else {
                    return Some(Err(format!("unknown border {arg}")));
                };
                let pen = Edge::new(self.pen.line, self.pen.color);
                let r =
                    self.workbook
                        .apply_border(self.sheet, self.bounded_selection(), preset, pen);
                if r.is_ok() {
                    self.modified = true;
                    self.draw = None;
                }
                return done(r);
            }
            "borderline" => {
                let Some(line) = Line::parse(arg) else {
                    return Some(Err(format!("unknown line style {arg}")));
                };
                self.pen.line = line;
                // Excel picks up the Draw Border pencil when a line is chosen.
                if flavor == Flavor::Excel {
                    self.draw = Some(DrawMode::Border);
                }
            }
            "bordercolor" => {
                self.pen.color = if arg == "auto" {
                    [0, 0, 0]
                } else {
                    match super::parse_hex(arg) {
                        Some(c) => c,
                        None => return Some(Err("that is not a colour".into())),
                    }
                };
                if flavor == Flavor::Excel {
                    self.draw = Some(DrawMode::Border);
                }
            }
            "drawborder" => {
                self.commit_edit();
                self.draw = match arg {
                    "border" => Some(DrawMode::Border),
                    "grid" => Some(DrawMode::Grid),
                    "erase" => Some(DrawMode::Erase),
                    "off" => None,
                    _ => return Some(Err("unknown drawing tool".into())),
                };
            }
            "cf" => {
                self.commit_edit();
                let (kind, rest) = arg.split_once(':').unwrap_or((arg, ""));
                let range = self.bounded_selection();
                let rule = match kind {
                    "bar" => {
                        let color = super::parse_hex(rest).unwrap_or(BAR_COLORS[0].1);
                        Rule::DataBar {
                            color,
                            min: Cfvo::Min,
                            max: Cfvo::Max,
                        }
                    }
                    "scale" => {
                        let Some((_, _, colors)) = COLOR_SCALES.iter().find(|(n, _, _)| *n == rest)
                        else {
                            return Some(Err("unknown colour scale".into()));
                        };
                        let mut stops = vec![(Cfvo::Min, colors[0])];
                        if colors.len() == 3 {
                            stops.push((Cfvo::Percentile(50.0), colors[1]));
                        }
                        stops.push((Cfvo::Max, colors[colors.len() - 1]));
                        Rule::ColorScale { stops }
                    }
                    "icons" => {
                        let Some((set, n)) = ICON_SETS.iter().find(|(s, _)| *s == rest) else {
                            return Some(Err("unknown icon set".into()));
                        };
                        Rule::IconSet {
                            set: (*set).into(),
                            points: points(*n),
                            reverse: false,
                            show_value: true,
                        }
                    }
                    other => return done(self.open_rule_dialog(other)),
                };
                let r = self
                    .workbook
                    .add_conditional(self.sheet, CondFormat::new(range, rule));
                if r.is_ok() {
                    self.modified = true;
                }
                return done(r);
            }
            "cfclear" => {
                self.commit_edit();
                let range = (arg != "sheet").then(|| self.selection());
                let r = self.workbook.clear_conditional(self.sheet, range);
                if r.is_ok() {
                    self.modified = true;
                }
                return done(r);
            }
            "cfmanage" => {
                self.commit_edit();
                self.dialog = Some(SheetDialog::Rules { selected: None });
            }
            "cfrule" | "cfdelete" | "cfup" | "cfdown" | "cfstop" => {
                let rules = self.sheet_ref().conditional.len();
                let Some(SheetDialog::Rules { selected }) = &mut self.dialog else {
                    return Some(Err("the Rules Manager is not open".into()));
                };
                if verb == "cfrule" {
                    match arg.parse::<usize>() {
                        Ok(i) if i < rules => *selected = Some(i),
                        _ => return Some(Err("no such rule".into())),
                    }
                    return Some(Ok(vec![]));
                }
                let Some(i) = *selected else {
                    return Some(Err("select a rule first".into()));
                };
                let r = match verb {
                    "cfdelete" => {
                        let r = self.workbook.remove_conditional(self.sheet, i);
                        *selected = None;
                        r
                    }
                    "cfup" => {
                        let r = self.workbook.move_conditional(self.sheet, i, true);
                        if r.is_ok() {
                            *selected = Some(i - 1);
                        }
                        r
                    }
                    "cfdown" => {
                        let r = self.workbook.move_conditional(self.sheet, i, false);
                        if r.is_ok() {
                            *selected = Some(i + 1);
                        }
                        r
                    }
                    _ => {
                        let on = !self.workbook.sheets[self.sheet].conditional[i].stop_if_true;
                        self.workbook.set_stop_if_true(self.sheet, i, on)
                    }
                };
                if r.is_ok() {
                    self.modified = true;
                }
                return done(r);
            }
            "dialog" => {
                let (what, value) = arg.split_once(':').unwrap_or((arg, ""));
                match what {
                    "ok" => return Some(self.dialog_ok(window, clock_us, flavor)),
                    "cancel" => self.dialog = None,
                    "field" => {
                        let i: usize = value.parse().unwrap_or(0);
                        match &mut self.dialog {
                            Some(SheetDialog::Rule { fields, focus, .. }) if i < fields.len() => {
                                *focus = i
                            }
                            Some(SheetDialog::Pivot { focus, .. }) if i < 2 => *focus = i,
                            _ => return Some(Err("no such field".into())),
                        }
                    }
                    "preset" => match &mut self.dialog {
                        Some(SheetDialog::Rule { preset, .. }) if Dxf::preset(value).is_some() => {
                            *preset = value.into()
                        }
                        _ => return Some(Err("no such format".into())),
                    },
                    "choice" => match &mut self.dialog {
                        Some(SheetDialog::Rule { choice, .. }) => *choice = value.into(),
                        _ => return Some(Err("nothing to choose here".into())),
                    },
                    "newsheet" | "existing" => match &mut self.dialog {
                        Some(SheetDialog::Pivot {
                            new_sheet, focus, ..
                        }) => {
                            *new_sheet = what == "newsheet";
                            if !*new_sheet {
                                *focus = 1;
                            }
                        }
                        _ => return Some(Err("no PivotTable dialog is open".into())),
                    },
                    "delim" => match &mut self.dialog {
                        Some(SheetDialog::TextToColumns { delimiter, .. }) => {
                            if !["comma", "tab", "semicolon", "space"].contains(&value) {
                                return Some(Err("unknown delimiter".into()));
                            }
                            *delimiter = value.into()
                        }
                        _ => return Some(Err("no Text to Columns dialog is open".into())),
                    },
                    "mergeruns" => match &mut self.dialog {
                        Some(SheetDialog::TextToColumns { merge_runs, .. }) => {
                            *merge_runs = !*merge_runs
                        }
                        _ => return Some(Err("no Text to Columns dialog is open".into())),
                    },
                    _ => return Some(Err(format!("unknown dialog control {what}"))),
                }
            }
            "pivot" => {
                self.commit_edit();
                match arg {
                    "new" => {
                        let region = self.selection();
                        let region = if region.is_single() {
                            self.current_region()
                        } else {
                            region
                        };
                        let name = cw_sheet::parser::quote_sheet(&self.sheet_ref().name);
                        let abs =
                            |c: Cell| format!("${}${}", cw_sheet::column_name(c.col), c.row + 1);
                        self.dialog = Some(SheetDialog::Pivot {
                            source: format!("{name}!{}:{}", abs(region.start), abs(region.end)),
                            new_sheet: true,
                            place: String::new(),
                            focus: 0,
                        });
                    }
                    "refresh" => {
                        let Some(i) = self.current_pivot() else {
                            return Some(Err("select a cell in the PivotTable first".into()));
                        };
                        let r = self.workbook.refresh_pivot(self.sheet, i);
                        if r.is_ok() {
                            self.modified = true;
                        }
                        return done(r);
                    }
                    "refreshall" => {
                        let list: Vec<(usize, usize)> = (0..self.workbook.sheets.len())
                            .flat_map(|s| {
                                (0..self.workbook.sheets[s].pivots.len()).map(move |i| (s, i))
                            })
                            .collect();
                        if list.is_empty() {
                            return Some(Err("there are no PivotTables to refresh".into()));
                        }
                        for (s, i) in list {
                            if let Err(e) = self.workbook.refresh_pivot(s, i) {
                                return Some(Err(e));
                            }
                        }
                        self.modified = true;
                    }
                    "delete" => {
                        let Some(i) = self.current_pivot() else {
                            return Some(Err("select a cell in the PivotTable first".into()));
                        };
                        let r = self.workbook.remove_pivot(self.sheet, i);
                        if r.is_ok() {
                            self.modified = true;
                        }
                        return done(r);
                    }
                    "pane" => self.pivot_pane_closed = !self.pivot_pane_closed,
                    _ => return Some(Err(format!("unknown PivotTable command {arg}"))),
                }
            }
            "pivotfield" => {
                let Ok(k) = arg.parse::<usize>() else {
                    return Some(Err("no such field".into()));
                };
                let numeric = self.numeric_field(k);
                return done(self.pivot_edit(|p| {
                    if k >= p.source.cols() as usize {
                        return Err("no such field".into());
                    }
                    let used = p.rows.contains(&k)
                        || p.cols.contains(&k)
                        || p.filters.contains(&k)
                        || p.values.iter().any(|(f, _)| *f == k);
                    if used {
                        p.rows.retain(|f| *f != k);
                        p.cols.retain(|f| *f != k);
                        p.filters.retain(|f| *f != k);
                        p.values.retain(|(f, _)| *f != k);
                        p.hidden.remove(&k);
                    } else if numeric {
                        p.values.push((k, Agg::Sum));
                    } else {
                        p.rows.push(k);
                    }
                    Ok(())
                }));
            }
            "pivotarea" => {
                let Some((k, area)) = arg.split_once(':') else {
                    return Some(Err("which field, and where?".into()));
                };
                let Ok(k) = k.parse::<usize>() else {
                    return Some(Err("no such field".into()));
                };
                let numeric = self.numeric_field(k);
                let area = area.to_owned();
                return done(self.pivot_edit(|p| {
                    if k >= p.source.cols() as usize {
                        return Err("no such field".into());
                    }
                    p.rows.retain(|f| *f != k);
                    p.cols.retain(|f| *f != k);
                    p.filters.retain(|f| *f != k);
                    match area.as_str() {
                        "rows" => {
                            p.values.retain(|(f, _)| *f != k);
                            p.rows.push(k)
                        }
                        "cols" => {
                            p.values.retain(|(f, _)| *f != k);
                            if !p.cols.is_empty() {
                                return Err("this PivotTable takes one column field; move the other one out first".into());
                            }
                            p.cols.push(k)
                        }
                        "filters" => {
                            p.values.retain(|(f, _)| *f != k);
                            p.filters.push(k)
                        }
                        "values" => p.values.push((k, if numeric { Agg::Sum } else { Agg::Count })),
                        "remove" => {
                            p.values.retain(|(f, _)| *f != k);
                            p.hidden.remove(&k);
                        }
                        _ => return Err("unknown area".into()),
                    }
                    Ok(())
                }));
            }
            "pivotagg" => {
                let Some((i, agg)) = arg.split_once(':') else {
                    return Some(Err("which value, and how?".into()));
                };
                let (Ok(i), Some(agg)) = (i.parse::<usize>(), Agg::parse(agg)) else {
                    return Some(Err("unknown summary".into()));
                };
                return done(self.pivot_edit(|p| {
                    p.values.get_mut(i).ok_or("no such value field")?.1 = agg;
                    Ok(())
                }));
            }
            "pivotfilter" => {
                self.menu = Some(format!("pivotitems:{arg}"));
                return Some(Ok(vec![]));
            }
            "pivothide" => {
                let Some((k, item)) = arg.split_once(':') else {
                    return Some(Err("which item?".into()));
                };
                let Ok(k) = k.parse::<usize>() else {
                    return Some(Err("no such field".into()));
                };
                let item = item.to_owned();
                let r = self.pivot_edit(|p| {
                    let set = p.hidden.entry(k).or_default();
                    if !set.remove(&item) {
                        set.insert(item);
                    }
                    if set.is_empty() {
                        p.hidden.remove(&k);
                    }
                    Ok(())
                });
                self.menu = Some(format!("pivotitems:{k}"));
                return done(r);
            }
            "calcnow" => {
                self.commit_edit();
                self.workbook.calculate_now();
            }
            "ttc" => {
                self.commit_edit();
                if self.selection().cols() != 1 {
                    // Excel answers with a message box rather than the wizard.
                    self.message =
                        Some("Text to Columns can convert only one column at a time.".into());
                    return Some(Ok(vec![]));
                }
                self.dialog = Some(SheetDialog::TextToColumns {
                    delimiter: "comma".into(),
                    merge_runs: false,
                });
            }
            _ => return None,
        };
        Some(Ok(vec![]))
    }
    /// Typing into the open dialog's focused field.
    pub(super) fn dialog_text(&mut self, text: &str) -> bool {
        match &mut self.dialog {
            Some(SheetDialog::Rule { fields, focus, .. }) => {
                if let Some(f) = fields.get_mut(*focus) {
                    super::push_bounded(f, text, 1024);
                }
                true
            }
            Some(SheetDialog::Pivot {
                source,
                place,
                focus,
                new_sheet,
            }) => {
                if *focus == 0 {
                    super::push_bounded(source, text, 256);
                } else {
                    *new_sheet = false;
                    super::push_bounded(place, text, 256);
                }
                true
            }
            _ => false,
        }
    }
    /// Keys while a dialog is open: Enter is OK, Escape Cancel, Tab the next field.
    pub(super) fn dialog_key(
        &mut self,
        window: u64,
        key: &str,
        clock_us: u64,
        flavor: Flavor,
    ) -> Result<Vec<AppEffect>, String> {
        match key {
            "Enter" => self.dialog_ok(window, clock_us, flavor),
            "Escape" => {
                self.dialog = None;
                Ok(vec![])
            }
            "Tab" | "Shift+Tab" => {
                match &mut self.dialog {
                    Some(SheetDialog::Rule { fields, focus, .. }) if !fields.is_empty() => {
                        *focus = (*focus + 1) % fields.len()
                    }
                    Some(SheetDialog::Pivot { focus, .. }) => *focus = 1 - (*focus).min(1),
                    _ => {}
                }
                Ok(vec![])
            }
            "Backspace" => {
                match &mut self.dialog {
                    Some(SheetDialog::Rule { fields, focus, .. }) => {
                        if let Some(f) = fields.get_mut(*focus) {
                            f.pop();
                        }
                    }
                    Some(SheetDialog::Pivot {
                        source,
                        place,
                        focus,
                        ..
                    }) => {
                        if *focus == 0 {
                            source.pop();
                        } else {
                            place.pop();
                        }
                    }
                    _ => {}
                }
                Ok(vec![])
            }
            other => Err(format!("unsupported dialog key {other}")),
        }
    }
    /// A Draw Borders drag finished over `range`.
    pub(super) fn draw_borders(&mut self, range: Range) -> Result<(), String> {
        let pen = Edge::new(self.pen.line, self.pen.color);
        let preset = match self.draw {
            Some(DrawMode::Border) => BorderPreset::Outside,
            Some(DrawMode::Grid) => BorderPreset::All,
            Some(DrawMode::Erase) => BorderPreset::None,
            None => return Ok(()),
        };
        self.workbook.apply_border(self.sheet, range, preset, pen)?;
        self.modified = true;
        Ok(())
    }
}
/// A comparison value typed into a rule dialog: numbers, references and formulas stay
/// as typed; other text becomes a text constant, as Excel quotes it.
fn quote_text(s: &str) -> String {
    let looks_like_formula = cw_sheet::parser::Formula::parse(s).is_ok()
        && (s.parse::<f64>().is_ok()
            || s.starts_with('$')
            || cw_sheet::Range::parse(s).is_some()
            || s.contains('(')
            || s.starts_with('"'));
    if looks_like_formula {
        s.to_owned()
    } else {
        format!("\"{}\"", s.replace('"', "\"\""))
    }
}
