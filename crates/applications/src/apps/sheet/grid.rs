//! The cell grid every spreadsheet product draws: headers, cells with their formats,
//! the selection and fill handle, frozen panes, filter buttons and charts.
use super::{Book, Drag};
use crate::desktop_scene::{shared::Align as TextAlign, Painter};
use cw_scene::{Color, Rect};
use cw_sheet::{Align, Cell, Range, Sheet, Value};

/// Grid metrics the pointer surfaces carry, so a press maps to the cell drawn there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geom {
    pub row_h: u32,
    /// Percent of each column's stored width.
    pub scale: u32,
}
impl Geom {
    pub fn col_px(&self, sheet: &Sheet, col: u32) -> u32 {
        (sheet.col_width(col) * self.scale / 100).max(4)
    }
    pub fn target(&self, surface: &str) -> String {
        format!("sheet:{surface}:{}:{}", self.row_h, self.scale)
    }
    pub fn from_target(target: &str) -> Option<Self> {
        let mut parts = target.rsplit(':');
        let scale = parts.next()?.parse().ok()?;
        let row_h = parts.next()?.parse().ok()?;
        (row_h > 0 && scale > 0).then_some(Self { row_h, scale })
    }
}
/// How one product draws its grid.
pub struct Palette {
    pub header_bg: Color,
    pub header_text: Color,
    pub header_sel_bg: Color,
    pub header_sel_text: Color,
    pub header_line: Color,
    pub grid_line: Color,
    pub sel_fill: Color,
    pub sel_border: Color,
    pub text: Color,
    pub font: u16,
    pub header_font: u16,
    pub head_w: u32,
    pub head_h: u32,
    /// Numbers draws no row and column headers until a table is selected; the others
    /// always do.
    pub headers: bool,
    pub chart_colors: &'static [Color],
}
/// Visible columns: (column, x offset from the cell area, width).
pub fn visible_cols(book: &Book, geom: Geom, width: u32) -> Vec<(u32, i32, u32)> {
    let sheet = book.sheet_ref();
    let fc = sheet.freeze.1;
    let mut out = Vec::new();
    let mut x = 0i32;
    for col in (0..fc).chain(book.scroll.1.max(fc)..cw_sheet::MAX_COLS) {
        if x >= width as i32 {
            break;
        }
        let w = geom.col_px(sheet, col);
        out.push((col, x, w));
        x += w as i32;
    }
    out
}
/// Visible rows: (row, y offset from the cell area), skipping rows a filter hides.
pub fn visible_rows(book: &Book, geom: Geom, height: u32) -> Vec<(u32, i32)> {
    let sheet = book.sheet_ref();
    let fr = sheet.freeze.0;
    let hidden = sheet.hidden_rows();
    let mut out = Vec::new();
    let mut y = 0i32;
    for row in (0..fr).chain(book.scroll.0.max(fr)..cw_sheet::MAX_ROWS) {
        if y >= height as i32 {
            break;
        }
        if hidden.contains(&row) {
            continue;
        }
        out.push((row, y));
        y += geom.row_h as i32;
    }
    out
}
fn rgb(c: [u8; 3]) -> Color {
    Color::rgb(c[0], c[1], c[2])
}
/// Clip the node just painted to `r`, so cell text is cut at the cell edge as a
/// spreadsheet does, instead of ellipsized.
fn clip_last(p: &mut Painter, r: Rect) {
    if let Some(n) = p.scene.nodes.last_mut() {
        n.clip = Some(match n.clip {
            Some(c) => c.intersection(r).unwrap_or(Rect::new(r.x, r.y, 0, 0)),
            None => r,
        });
    }
}
/// Draw the grid into `area` (headers included). Returns the cell area's rectangle.
pub fn paint(p: &mut Painter, book: &Book, area: Rect, geom: Geom, pal: &Palette) -> Rect {
    let sheet = book.sheet_ref();
    let (head_w, head_h) = if pal.headers {
        (pal.head_w, pal.head_h)
    } else {
        (0, 0)
    };
    let cells = Rect::new(
        area.x + head_w as i32,
        area.y + head_h as i32,
        area.width.saturating_sub(head_w),
        area.height.saturating_sub(head_h),
    );
    p.box_(area, Color::WHITE, 0);
    let cols = visible_cols(book, geom, cells.width);
    let rows = visible_rows(book, geom, cells.height);
    let sel = book.selection();
    let sheet_index = book.sheet.min(book.workbook.sheets.len() - 1);
    // The drag surface under everything interactive in the grid.
    p.region(cells, &geom.target("grid"), "Cells");
    // Headers.
    if pal.headers {
        p.box_(
            Rect::new(area.x, area.y, area.width, head_h),
            pal.header_bg,
            0,
        );
        p.box_(
            Rect::new(area.x, area.y, head_w, area.height),
            pal.header_bg,
            0,
        );
        let whole_rows = sel.start.col == 0 && sel.end.col == cw_sheet::MAX_COLS - 1;
        let whole_cols = sel.start.row == 0 && sel.end.row == cw_sheet::MAX_ROWS - 1;
        for &(col, x, w) in &cols {
            let r = Rect::new(cells.x + x, area.y, w, head_h);
            let on = (sel.start.col..=sel.end.col).contains(&col);
            if on {
                p.box_(
                    r,
                    if whole_cols {
                        pal.sel_border
                    } else {
                        pal.header_sel_bg
                    },
                    0,
                );
            }
            p.label(
                r.x,
                r.y + (head_h as i32 - i32::from(pal.header_font) - 4) / 2,
                w,
                &cw_sheet::column_name(col),
                pal.header_font,
                if on && whole_cols {
                    Color::WHITE
                } else if on {
                    pal.header_sel_text
                } else {
                    pal.header_text
                },
                on,
                TextAlign::Center,
            );
            clip_last(p, r);
            p.vline(r.x + w as i32 - 1, area.y, head_h, pal.header_line);
            p.region(
                r,
                &format!("sheet:col:{}", cw_sheet::column_name(col)),
                &format!("Column {}", cw_sheet::column_name(col)),
            );
        }
        for &(row, y) in &rows {
            let r = Rect::new(area.x, cells.y + y, head_w, geom.row_h);
            let on = (sel.start.row..=sel.end.row).contains(&row);
            if on {
                p.box_(
                    r,
                    if whole_rows {
                        pal.sel_border
                    } else {
                        pal.header_sel_bg
                    },
                    0,
                );
            }
            p.label(
                r.x,
                r.y + (geom.row_h as i32 - i32::from(pal.header_font) - 4) / 2,
                head_w - 4,
                &(row + 1).to_string(),
                pal.header_font,
                if on && whole_rows {
                    Color::WHITE
                } else if on {
                    pal.header_sel_text
                } else {
                    pal.header_text
                },
                on,
                TextAlign::Center,
            );
            clip_last(p, r);
            p.hline(area.x, r.y + geom.row_h as i32 - 1, head_w, pal.header_line);
            p.region(
                r,
                &format!("sheet:row:{}", row + 1),
                &format!("Row {}", row + 1),
            );
        }
        p.hline(
            area.x,
            area.y + head_h as i32 - 1,
            area.width,
            pal.header_line,
        );
        p.vline(
            area.x + head_w as i32 - 1,
            area.y,
            area.height,
            pal.header_line,
        );
        p.region(
            Rect::new(area.x, area.y, head_w, head_h),
            "sheet:all",
            "Select all",
        );
    }
    // Fills first, then gridlines, then text, so text sits over its own background.
    let row_span = |row: u32| rows.iter().find(|(r, _)| *r == row).map(|(_, y)| *y);
    let col_span = |col: u32| {
        cols.iter()
            .find(|(c, _, _)| *c == col)
            .map(|(_, x, w)| (*x, *w))
    };
    let (first_col, last_col) = (
        cols.first().map_or(0, |c| c.0),
        cols.last().map_or(0, |c| c.0),
    );
    let (fr, fc) = sheet.freeze;
    let visible_cells = |row: u32| {
        let lo = if fc > 0 { 0 } else { first_col };
        sheet
            .cells
            .range(Cell::new(row, lo)..=Cell::new(row, last_col))
    };
    for &(row, y) in &rows {
        for (c, d) in visible_cells(row) {
            if let (Some(fill), Some((x, w))) = (d.style.fill, col_span(c.col)) {
                p.box_(
                    Rect::new(cells.x + x, cells.y + y, w, geom.row_h),
                    rgb(fill),
                    0,
                );
            }
        }
    }
    if book.gridlines {
        for &(_, x, w) in &cols {
            p.vline(
                cells.x + x + w as i32 - 1,
                cells.y,
                cells.height,
                pal.grid_line,
            );
        }
        for &(_, y) in &rows {
            p.hline(
                cells.x,
                cells.y + y + geom.row_h as i32 - 1,
                cells.width,
                pal.grid_line,
            );
        }
    }
    let font = (u32::from(pal.font) * geom.scale / 100).clamp(6, 40) as u16;
    for &(row, y) in &rows {
        let stored: Vec<(Cell, &cw_sheet::workbook::CellData)> =
            visible_cells(row).map(|(c, d)| (*c, d)).collect();
        for (i, (c, d)) in stored.iter().enumerate() {
            let Some((x, w)) = col_span(c.col) else {
                continue;
            };
            if book.editing.is_some()
                && *c == book.active
                && !book.editing.as_ref().is_some_and(|e| e.in_bar)
            {
                continue;
            }
            let shown = cw_sheet::format::format(&d.value, &d.style.format);
            let mut text = shown.text;
            if text.is_empty() {
                continue;
            }
            let numeric = matches!(d.value, Value::Number(_));
            let align = match d.style.align {
                Align::Left => TextAlign::Left,
                Align::Center => TextAlign::Center,
                Align::Right => TextAlign::Right,
                Align::General => match d.value {
                    Value::Number(_) => TextAlign::Right,
                    Value::Bool(_) | Value::Error(_) => TextAlign::Center,
                    _ => TextAlign::Left,
                },
            };
            let cell_rect = Rect::new(cells.x + x, cells.y + y, w, geom.row_h);
            // Left-aligned text runs on into empty neighbours; numbers that do not fit
            // show as ####, as they do in every spreadsheet.
            let mut span = w;
            if !numeric && align == TextAlign::Left {
                let next_filled = stored.get(i + 1).map(|(n, _)| n.col);
                for &(col, _, cw) in cols.iter().filter(|(col, _, _)| *col > c.col) {
                    if next_filled.is_some_and(|n| n <= col) || span > 2000 {
                        break;
                    }
                    span += cw;
                }
            }
            // Bold and italic cells are set in the real bold and italic faces, and
            // measured in them, so overflow and #### follow what is drawn.
            let bold = cw_scene::Style::new(d.style.bold, d.style.italic, cw_scene::Lang::Auto);
            let width = p.measure(&text, font, bold);
            if numeric && width + 6 > w {
                let hashes = (w.saturating_sub(4) / p.measure("#", font, false).max(1)).max(1);
                text = "#".repeat(hashes as usize);
            }
            let color = shown
                .color
                .map(rgb)
                .or(d.style.color.map(rgb))
                .unwrap_or(pal.text);
            let ty =
                cell_rect.y + (geom.row_h as i32 - i32::from(font) - i32::from(font) / 2) / 2 + 1;
            let (lx, lw) = match align {
                TextAlign::Left => (cell_rect.x + 3, span.saturating_sub(4).max(1)),
                _ => (cell_rect.x + 2, w.saturating_sub(5).max(1)),
            };
            // Left-aligned text keeps its full width (the clip cuts it); aligned text is
            // placed within the cell.
            let text_w = p.measure(&text, font, bold);
            let label_w = if align == TextAlign::Left {
                lw.max(text_w + 2)
            } else {
                lw
            };
            p.label(lx, ty, label_w, &text, font, color, bold, align);
            let clip = Rect::new(
                cell_rect.x,
                cell_rect.y,
                if align == TextAlign::Left { span } else { w },
                geom.row_h,
            );
            clip_last(p, clip.intersection(cells).unwrap_or(clip));
            if d.style.underline {
                let tw = p.measure(&text, font, bold).min(lw);
                let ux = match align {
                    TextAlign::Left => lx,
                    TextAlign::Center => lx + (lw as i32 - tw as i32) / 2,
                    TextAlign::Right => lx + lw as i32 - tw as i32,
                };
                p.hline(ux, ty + i32::from(font) + 2, tw, color);
                clip_last(p, clip);
            }
        }
    }
    // Frozen panes are marked with a darker rule.
    if fr > 0 {
        if let Some(y) = rows.iter().find(|(r, _)| *r >= fr).map(|(_, y)| *y) {
            p.hline(
                cells.x,
                cells.y + y - 1,
                cells.width,
                Color::rgb(160, 160, 160),
            );
        }
    }
    if fc > 0 {
        if let Some(x) = cols.iter().find(|(c, _, _)| *c >= fc).map(|(_, x, _)| *x) {
            p.vline(
                cells.x + x - 1,
                cells.y,
                cells.height,
                Color::rgb(160, 160, 160),
            );
        }
    }
    // The copy marquee: a dashed rule around what Copy or Cut took.
    if let Some((s, _, _, range, _)) = &book.clip {
        if *s == sheet_index {
            if let Some(r) = range_rect(&rows, &cols, geom, *range, cells) {
                dashed(p, r, pal.sel_border);
            }
        }
    }
    // Selection: a tint over the range, a border, the active cell left clear.
    let sel_rect = range_rect(&rows, &cols, geom, sel, cells);
    if let Some(r) = sel_rect {
        let clipped = r.intersection(cells).unwrap_or(r);
        if !sel.is_single() {
            p.box_(clipped, pal.sel_fill, 0);
            if let Some(a) = range_rect(&rows, &cols, geom, Range::single(book.active), cells) {
                p.box_(a.intersection(cells).unwrap_or(a), Color::WHITE, 0);
                // The active cell's own text stays readable over the clearing.
                if let Some(d) = book.workbook.cell(sheet_index, book.active) {
                    let text = cw_sheet::format::format(&d.value, &d.style.format).text;
                    let right = matches!(d.value, Value::Number(_));
                    p.label(
                        a.x + 3,
                        a.y + (geom.row_h as i32 - i32::from(font) - i32::from(font) / 2) / 2 + 1,
                        a.width.saturating_sub(6).max(1),
                        &text,
                        font,
                        pal.text,
                        cw_scene::Style::new(d.style.bold, d.style.italic, cw_scene::Lang::Auto),
                        if right {
                            TextAlign::Right
                        } else {
                            TextAlign::Left
                        },
                    );
                    clip_last(p, a);
                }
            }
        }
        for (dx, dy, w, h) in [
            (0, 0, r.width, 2),
            (0, r.height as i32 - 2, r.width, 2),
            (0, 0, 2, r.height),
            (r.width as i32 - 2, 0, 2, r.height),
        ] {
            p.box_(Rect::new(r.x + dx, r.y + dy, w, h), pal.sel_border, 0);
            clip_last(p, cells);
        }
        // The fill handle on the bottom-right corner, a drag surface of its own.
        let handle = Rect::new(r.x + r.width as i32 - 4, r.y + r.height as i32 - 4, 7, 7);
        if cells.contains(handle.x, handle.y) && book.editing.is_none() {
            p.box_(handle, Color::WHITE, 0);
            p.box_(
                Rect::new(handle.x + 1, handle.y + 1, 5, 5),
                pal.sel_border,
                0,
            );
            p.region(handle, &geom.target("fill"), "Fill handle");
        }
    }
    // A fill in progress outlines where it will land.
    if let Some(Drag::Fill { source, to }) = &book.drag {
        let target = super::fill_target(*source, *to);
        if let Some(r) = range_rect(&rows, &cols, geom, target, cells) {
            dashed(p, r, Color::rgb(120, 120, 120));
        }
    }
    // Auto filter buttons in the header row of the filtered range.
    if let Some(f) = &sheet.filter {
        if let Some(y) = row_span(f.range.start.row) {
            for col in f.range.start.col..=f.range.end.col {
                if let Some((x, w)) = col_span(col) {
                    let b = Rect::new(
                        cells.x + x + w as i32 - 17,
                        cells.y + y + 2,
                        15,
                        geom.row_h.saturating_sub(4),
                    );
                    let active = f.hidden.contains_key(&col);
                    p.border(b, Color::rgb(245, 245, 245), 2, Color::rgb(170, 170, 170));
                    p.symbol(
                        if active { "filters" } else { "chevron-down" },
                        b.x + 2,
                        b.y + (b.height as i32 - 11) / 2,
                        11,
                        Color::rgb(70, 70, 70),
                    );
                    p.region(
                        b,
                        &format!("sheet:filterpick:{}", cw_sheet::column_name(col)),
                        &format!("Filter column {}", cw_sheet::column_name(col)),
                    );
                }
            }
        }
    }
    // Charts float over the grid at their anchor.
    for (i, chart) in sheet.charts.iter().enumerate() {
        let Some((ax, ay)) = book.cell_origin(geom, chart.anchor) else {
            continue;
        };
        let w: u32 = (chart.anchor.col..chart.anchor.col + chart.cols)
            .map(|c| geom.col_px(sheet, c))
            .sum();
        let h = chart.rows * geom.row_h;
        let r = Rect::new(cells.x + ax, cells.y + ay, w.max(120), h.max(90));
        if r.x > cells.x + cells.width as i32 || r.y > cells.y + cells.height as i32 {
            continue;
        }
        let mark = p.scene.nodes.len();
        super::chart::paint(p, &book.workbook, sheet_index, chart, r, pal);
        p.region(
            r,
            &format!("sheet:chartsel:{i}"),
            &format!("Chart {}", chart.title),
        );
        if book.chart == Some(i) {
            p.border(r, Color::TRANSPARENT, 0, pal.sel_border);
            for (hx, hy) in [
                (r.x, r.y),
                (r.x + r.width as i32, r.y),
                (r.x, r.y + r.height as i32),
                (r.x + r.width as i32, r.y + r.height as i32),
            ] {
                p.box_(Rect::new(hx - 3, hy - 3, 7, 7), Color::WHITE, 1);
                p.border(
                    Rect::new(hx - 3, hy - 3, 7, 7),
                    Color::TRANSPARENT,
                    1,
                    pal.sel_border,
                );
            }
        }
        for n in &mut p.scene.nodes[mark..] {
            n.clip = Some(match n.clip {
                Some(c) => c.intersection(cells).unwrap_or(Rect::new(0, 0, 0, 0)),
                None => cells,
            });
        }
    }
    // In-cell editing: the editor covers the cell and grows to fit its text.
    if let Some(e) = &book.editing {
        if !e.in_bar {
            if let Some(a) = range_rect(&rows, &cols, geom, Range::single(book.active), cells) {
                let tw = p.measure(&e.text, font, false) + 12;
                let r = Rect::new(a.x, a.y, a.width.max(tw), a.height);
                p.box_(r, Color::WHITE, 0);
                p.border(r, Color::TRANSPARENT, 0, pal.sel_border);
                p.left(
                    r.x + 3,
                    r.y + (geom.row_h as i32 - i32::from(font) - i32::from(font) / 2) / 2 + 1,
                    r.width.saturating_sub(4),
                    &e.text,
                    font,
                    pal.text,
                );
                let caret_x = r.x + 3 + p.measure(&e.text[..e.caret], font, false) as i32;
                p.vline(caret_x, r.y + 3, r.height.saturating_sub(6), pal.text);
            }
        }
    }
    cells
}
/// Screen rectangle of a range, clipped to the visible rows and columns.
fn range_rect(
    rows: &[(u32, i32)],
    cols: &[(u32, i32, u32)],
    geom: Geom,
    r: Range,
    cells: Rect,
) -> Option<Rect> {
    let vis_rows: Vec<&(u32, i32)> = rows
        .iter()
        .filter(|(row, _)| (r.start.row..=r.end.row).contains(row))
        .collect();
    let vis_cols: Vec<&(u32, i32, u32)> = cols
        .iter()
        .filter(|(col, _, _)| (r.start.col..=r.end.col).contains(col))
        .collect();
    let (top, bottom) = (vis_rows.first()?, vis_rows.last()?);
    let (left, right) = (vis_cols.first()?, vis_cols.last()?);
    let x = cells.x + left.1;
    let y = cells.y + top.1;
    let w = (right.1 + right.2 as i32) - left.1;
    let h = bottom.1 + geom.row_h as i32 - top.1;
    Some(Rect::new(
        x - 1,
        y - 1,
        (w + 1).max(1) as u32,
        (h + 1).max(1) as u32,
    ))
}
fn dashed(p: &mut Painter, r: Rect, color: Color) {
    let mut x = r.x;
    while x < r.x + r.width as i32 {
        p.box_(Rect::new(x, r.y, 4, 2), color, 0);
        p.box_(Rect::new(x, r.y + r.height as i32 - 2, 4, 2), color, 0);
        x += 8;
    }
    let mut y = r.y;
    while y < r.y + r.height as i32 {
        p.box_(Rect::new(r.x, y, 2, 4), color, 0);
        p.box_(Rect::new(r.x + r.width as i32 - 2, y, 2, 4), color, 0);
        y += 8;
    }
}
