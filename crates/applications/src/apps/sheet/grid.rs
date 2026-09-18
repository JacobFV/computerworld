//! The cell grid every spreadsheet product draws: headers, cells with their formats,
//! borders, merged cells and conditional formatting, the selection and fill handle,
//! frozen panes, filter buttons, the references of a formula being typed, and charts
//! that can be dragged and resized.
use super::{Book, Drag};
use crate::desktop_scene::{shared::Align as TextAlign, Painter};
use cw_scene::{Color, Rect};
use cw_sheet::conditional::Effect;
use cw_sheet::{Align, Cell, Edge, Line, Range, Sheet, Value};
use std::collections::BTreeMap;

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
    /// The colours a formula's references take while it is typed, in order.
    pub ref_colors: &'static [Color],
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
/// Where a column starts, relative to the cell area, even when scrolled off it.
pub fn col_offset(book: &Book, geom: Geom, col: u32) -> i32 {
    let sheet = book.sheet_ref();
    let fc = sheet.freeze.1;
    let w = |c: u32| geom.col_px(sheet, c) as i32;
    let frozen: i32 = (0..fc).map(w).sum();
    let start = book.scroll.1.max(fc);
    if col < fc {
        (0..col).map(w).sum()
    } else if col >= start {
        frozen + (start..col.min(start + 2000)).map(w).sum::<i32>()
    } else {
        frozen - (col..start).map(w).sum::<i32>()
    }
}
/// Where a row starts, relative to the cell area, even when scrolled off it.
pub fn row_offset(book: &Book, geom: Geom, row: u32) -> i32 {
    let sheet = book.sheet_ref();
    let fr = sheet.freeze.0;
    let hidden = sheet.hidden_rows();
    let h = |r: u32| {
        if hidden.contains(&r) {
            0
        } else {
            geom.row_h as i32
        }
    };
    let frozen: i32 = (0..fr).map(h).sum();
    let start = book.scroll.0.max(fr);
    if row < fr {
        (0..row).map(h).sum()
    } else if row >= start {
        frozen + (start..row.min(start + 5000)).map(h).sum::<i32>()
    } else {
        frozen - (row..start).map(h).sum::<i32>()
    }
}
/// A chart's box in sheet pixels, drawn on screen relative to the cell area.
pub fn chart_screen(book: &Book, geom: Geom, b: (i64, i64, i64, i64)) -> Rect {
    let sheet = book.sheet_ref();
    let to_screen = |x: i64, y: i64| -> (i32, i32) {
        let (cell, ox, oy) = {
            let mut left = x.max(0);
            let mut col = 0u32;
            while col < cw_sheet::MAX_COLS - 1 && left >= i64::from(sheet.col_width(col)) {
                left -= i64::from(sheet.col_width(col));
                col += 1;
            }
            let rh = i64::from(cw_sheet::workbook::ROW_HEIGHT);
            (
                Cell::new(((y.max(0)) / rh) as u32, col),
                left,
                y.max(0) % rh,
            )
        };
        (
            col_offset(book, geom, cell.col) + (ox * i64::from(geom.scale) / 100) as i32,
            row_offset(book, geom, cell.row)
                + (oy * i64::from(geom.row_h) / i64::from(cw_sheet::workbook::ROW_HEIGHT)) as i32,
        )
    };
    let (x0, y0) = to_screen(b.0, b.1);
    let (x1, y1) = to_screen(b.2, b.3);
    Rect::new(x0, y0, (x1 - x0).max(20) as u32, (y1 - y0).max(20) as u32)
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
/// Clip every node from `mark` on to `r`.
fn clip_from(p: &mut Painter, mark: usize, r: Rect) {
    for n in &mut p.scene.nodes[mark..] {
        n.clip = Some(match n.clip {
            Some(c) => c.intersection(r).unwrap_or(Rect::new(r.x, r.y, 0, 0)),
            None => r,
        });
    }
}
/// One border edge, `horizontal` from (x, y) to the right or else downwards, in its
/// line style: widths, dashes and double lines as spreadsheets draw them.
fn edge(p: &mut Painter, x: i32, y: i32, len: u32, horizontal: bool, e: Edge) {
    let color = rgb(e.color);
    let seg = |p: &mut Painter, at: i32, n: u32, off: i32, w: u32| {
        if horizontal {
            p.box_(Rect::new(x + at, y + off, n, w), color, 0);
        } else {
            p.box_(Rect::new(x + off, y + at, w, n), color, 0);
        }
    };
    if e.line == Line::Double {
        seg(p, 0, len, -1, 1);
        seg(p, 0, len, 1, 1);
        return;
    }
    let w = e.line.width();
    let off = -((w as i32 - 1) / 2);
    let dashes = e.line.dashes();
    if dashes.is_empty() {
        seg(p, 0, len, off, w);
        return;
    }
    let unit = if w > 1 { 2 } else { 1 };
    let mut at = 0i32;
    let mut k = 0;
    while at < len as i32 {
        let on = dashes[k % dashes.len()] * unit;
        let gap = dashes[(k + 1) % dashes.len()] * unit;
        let n = on.min((len as i32 - at) as u32);
        seg(p, at, n, off, w);
        at += (on + gap) as i32;
        k += 2;
    }
}
/// One of Excel's conditional formatting icons, `idx` of a set of `n`.
pub fn icon(p: &mut Painter, set: &str, idx: usize, n: usize, x: i32, y: i32, size: u32) {
    let red = Color::rgb(237, 28, 36);
    let yellow = Color::rgb(255, 192, 0);
    let green = Color::rgb(0, 176, 80);
    let gray = Color::rgb(128, 128, 128);
    let s = size as i32;
    let (cx, cy) = (x + s / 2, y + s / 2);
    // Arrows: an angle from straight down (0) to straight up (4), in quarter turns.
    let arrow = |p: &mut Painter, steps: i32, color: Color| {
        let r = s as f64 / 2.0;
        let angle = std::f64::consts::PI / 2.0 - std::f64::consts::PI / 4.0 * f64::from(steps);
        let (dx, dy) = (
            cw_determinism::math::cos(angle) * r * 0.9,
            -cw_determinism::math::sin(angle) * r * 0.9,
        );
        let tip = (cx + dx.round() as i32, cy - dy.round() as i32);
        let tail = (cx - dx.round() as i32, cy + dy.round() as i32);
        p.line(vec![tail, tip], color, 2);
        let (px, py) = (-dy * 0.55, dx * 0.55);
        let base = (
            (f64::from(tip.0) - dx * 0.55).round(),
            (f64::from(tip.1) + dy * 0.55).round(),
        );
        p.path(
            vec![
                tip,
                ((base.0 + px) as i32, (base.1 - py) as i32),
                ((base.0 - px) as i32, (base.1 + py) as i32),
            ],
            color,
        );
    };
    match set {
        "3Arrows" | "3ArrowsGray" => {
            let colors = if set == "3ArrowsGray" {
                [gray, gray, gray]
            } else {
                [red, yellow, green]
            };
            arrow(p, [0, 2, 4][idx.min(2)], colors[idx.min(2)]);
        }
        "4Arrows" => arrow(
            p,
            [0, 1, 3, 4][idx.min(3)],
            [red, yellow, yellow, green][idx.min(3)],
        ),
        "5Arrows" => arrow(
            p,
            [0, 1, 2, 3, 4][idx.min(4)],
            [red, yellow, yellow, yellow, green][idx.min(4)],
        ),
        "3TrafficLights1" => p.circle(cx, cy, size / 2, [red, yellow, green][idx.min(2)]),
        "4TrafficLights" => p.circle(
            cx,
            cy,
            size / 2,
            [Color::rgb(40, 40, 40), red, yellow, green][idx.min(3)],
        ),
        "3Symbols" => {
            p.circle(cx, cy, size / 2, [red, yellow, green][idx.min(2)]);
            match idx {
                0 => p.symbol("close", x + 2, y + 2, size.saturating_sub(4), Color::WHITE),
                1 => {
                    p.box_(Rect::new(cx - 1, y + 3, 2, (s / 2) as u32), Color::WHITE, 0);
                    p.box_(Rect::new(cx - 1, y + s - 5, 2, 2), Color::WHITE, 0);
                }
                _ => p.symbol("check", x + 2, y + 2, size.saturating_sub(4), Color::WHITE),
            }
        }
        _ => {
            // Flags: a pole and a pennant.
            let c = [red, yellow, green][idx.min(2)];
            p.box_(
                Rect::new(x + 2, y + 1, 1, size.saturating_sub(2)),
                Color::rgb(90, 90, 90),
                0,
            );
            p.path(
                vec![
                    (x + 3, y + 1),
                    (x + s - 1, y + s / 3),
                    (x + 3, y + 2 * s / 3),
                ],
                c,
            );
        }
    }
    let _ = n;
}
/// Formula text with each reference in its colour, as the editors show it.
#[allow(clippy::too_many_arguments)]
pub fn formula_text(
    p: &mut Painter,
    x: i32,
    y: i32,
    width: u32,
    text: &str,
    size: u16,
    ink: Color,
    colors: &[Color],
) {
    if !text.starts_with('=') || colors.is_empty() {
        p.left(x, y, width, text, size, ink);
        return;
    }
    let spans = cw_sheet::parser::reference_spans(text);
    if spans.is_empty() {
        p.left(x, y, width, text, size, ink);
        return;
    }
    let order = ref_order(&spans);
    let mut at = 0;
    let mut cx = x;
    let end = x + width as i32;
    let piece = |p: &mut Painter, s: &str, c: Color, cx: &mut i32| {
        if s.is_empty() || *cx >= end {
            return;
        }
        let w = p.measure(s, size, false);
        p.left(*cx, y, (end - *cx).max(1) as u32, s, size, c);
        *cx += w as i32;
    };
    for (k, sp) in spans.iter().enumerate() {
        piece(p, &text[at..sp.start], ink, &mut cx);
        piece(
            p,
            &text[sp.start..sp.end],
            colors[order[k] % colors.len()],
            &mut cx,
        );
        at = sp.end;
    }
    piece(p, &text[at..], ink, &mut cx);
}
/// Each reference's colour number: the same reference text keeps one colour.
pub fn ref_order(spans: &[cw_sheet::parser::RefSpan]) -> Vec<usize> {
    let mut seen: Vec<(Option<String>, Range)> = Vec::new();
    spans
        .iter()
        .map(|s| {
            let key = (s.sheet.as_ref().map(|n| n.to_lowercase()), s.range);
            match seen.iter().position(|k| *k == key) {
                Some(i) => i,
                None => {
                    seen.push(key);
                    seen.len() - 1
                }
            }
        })
        .collect()
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
    let lo_col = if fc > 0 { 0 } else { first_col };
    let visible_cells = |row: u32| {
        sheet
            .cells
            .range(Cell::new(row, lo_col)..=Cell::new(row, last_col))
    };
    // Conditional formatting of what is on screen.
    let window = Range::new(
        Cell::new(rows.first().map_or(0, |r| r.0.min(fr)), lo_col.min(fc)),
        Cell::new(rows.last().map_or(0, |r| r.0), last_col),
    );
    let effects: BTreeMap<Cell, Effect> = if sheet.conditional.is_empty() {
        BTreeMap::new()
    } else {
        book.workbook.conditional_effects(sheet_index, window)
    };
    // Merged areas on screen, with their rectangles.
    let merges: Vec<(Range, Rect)> = sheet
        .merges
        .iter()
        .filter_map(|m| range_rect(&rows, &cols, geom, *m, cells).map(|r| (*m, r)))
        .collect();
    let merged_rect = |c: Cell| merges.iter().find(|(m, _)| m.start == c).map(|(_, r)| *r);
    let covered = |c: Cell| merges.iter().any(|(m, _)| m.contains(c) && m.start != c);
    let fill_of = |c: Cell, own: Option<[u8; 3]>| -> Option<Color> {
        let e = effects.get(&c);
        e.and_then(|e| e.style.fill.or(e.scale)).or(own).map(rgb)
    };
    // Fills first, then gridlines, then merged areas over them, borders, then text.
    for &(row, y) in &rows {
        for (c, d) in visible_cells(row) {
            if covered(*c) || merged_rect(*c).is_some() {
                continue;
            }
            if let (Some(fill), Some((x, w))) = (fill_of(*c, d.style.fill), col_span(c.col)) {
                p.box_(Rect::new(cells.x + x, cells.y + y, w, geom.row_h), fill, 0);
            }
        }
        // Colour scales on cells whose own style is plain (so not stored).
        for (c, e) in effects.range(Cell::new(row, 0)..=Cell::new(row, last_col)) {
            if sheet.cells.contains_key(c) || covered(*c) {
                continue;
            }
            if let (Some(fill), Some((x, w))) = (e.style.fill.or(e.scale), col_span(c.col)) {
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
    // A merged area is one cell: its own background covers the gridlines inside it.
    for (m, r) in &merges {
        let own = sheet.cells.get(&m.start).and_then(|d| d.style.fill);
        let bg = fill_of(m.start, own).unwrap_or(Color::WHITE);
        let inner = Rect::new(
            r.x + 1,
            r.y + 1,
            r.width.saturating_sub(2),
            r.height.saturating_sub(2),
        );
        p.box_(inner, bg, 0);
        clip_last(p, cells);
    }
    // Data bars under the text.
    for (c, e) in &effects {
        let Some((frac, color)) = e.bar else {
            continue;
        };
        let (Some(y), Some((x, w))) = (row_span(c.row), col_span(c.col)) else {
            continue;
        };
        let full = w.saturating_sub(4);
        let len = ((f64::from(full) * frac).round() as u32).max(2).min(full);
        let bar = Rect::new(
            cells.x + x + 2,
            cells.y + y + 2,
            len,
            geom.row_h.saturating_sub(5),
        );
        let c = rgb(color);
        p.box_(bar, Color(c.0, c.1, c.2, 150), 0);
        p.border(bar, Color::TRANSPARENT, 0, c);
    }
    // Borders: each edge once, the heavier of the two cells that share it.
    let mark = p.scene.nodes.len();
    for &(row, y) in &rows {
        for &(col, x, w) in &cols {
            let c = Cell::new(row, col);
            let (sx, sy) = (cells.x + x, cells.y + y);
            let inside =
                |a: Cell, b: Cell| merges.iter().any(|(m, _)| m.contains(a) && m.contains(b));
            if let Some(e) = book.workbook.shared_edge(sheet_index, c, false) {
                if !inside(c, Cell::new(row + 1, col)) {
                    edge(p, sx - 1, sy + geom.row_h as i32 - 1, w + 1, true, e);
                }
            }
            if let Some(e) = book.workbook.shared_edge(sheet_index, c, true) {
                if !inside(c, Cell::new(row, col + 1)) {
                    edge(p, sx + w as i32 - 1, sy - 1, geom.row_h + 1, false, e);
                }
            }
            // The first visible row and column also show their own top and left.
            let own = sheet.cells.get(&c).map(|d| d.style.borders);
            if Some(&(row, y)) == rows.first() || row == 0 {
                let above = row.checked_sub(1).and_then(|r| {
                    book.workbook
                        .shared_edge(sheet_index, Cell::new(r, col), false)
                });
                if let Some(e) = above.or(own.and_then(|b| b.top)) {
                    edge(p, sx - 1, sy - 1, w + 1, true, e);
                }
            }
            if Some(&(col, x, w)) == cols.first() || col == 0 {
                let before = col.checked_sub(1).and_then(|k| {
                    book.workbook
                        .shared_edge(sheet_index, Cell::new(row, k), true)
                });
                if let Some(e) = before.or(own.and_then(|b| b.left)) {
                    edge(p, sx - 1, sy - 1, geom.row_h + 1, false, e);
                }
            }
        }
    }
    clip_from(p, mark, cells);
    let font = (u32::from(pal.font) * geom.scale / 100).clamp(6, 40) as u16;
    for &(row, y) in &rows {
        let stored: Vec<(Cell, &cw_sheet::workbook::CellData)> =
            visible_cells(row).map(|(c, d)| (*c, d)).collect();
        for (i, (c, d)) in stored.iter().enumerate() {
            if covered(*c) {
                continue;
            }
            let Some((x, w)) = col_span(c.col) else {
                continue;
            };
            if book.editing.is_some()
                && *c == book.active
                && !book.editing.as_ref().is_some_and(|e| e.in_bar)
            {
                continue;
            }
            let effect = effects.get(c);
            let shown = cw_sheet::format::format(&d.value, &d.style.format);
            let mut text = shown.text;
            if effect.is_some_and(|e| e.hide_value) {
                text.clear();
            }
            let numeric = matches!(d.value, Value::Number(_));
            let mut cell_rect = Rect::new(cells.x + x, cells.y + y, w, geom.row_h);
            let merged = merged_rect(*c);
            if let Some(r) = merged {
                cell_rect = Rect::new(
                    r.x + 1,
                    r.y + 1,
                    r.width.saturating_sub(1),
                    r.height.saturating_sub(1),
                );
            }
            // An icon sits at the cell's left edge; the text keeps clear of it.
            let icon_w = if let Some((set, idx, n)) = effect.and_then(|e| e.icon.as_ref()) {
                let size = (u32::from(font)).clamp(8, 16);
                icon(
                    p,
                    set,
                    *idx,
                    *n,
                    cell_rect.x + 2,
                    cell_rect.y + (cell_rect.height as i32 - size as i32) / 2,
                    size,
                );
                clip_last(p, cells);
                size + 3
            } else {
                0
            };
            if text.is_empty() {
                continue;
            }
            let mut align = match d.style.align {
                Align::Left => TextAlign::Left,
                Align::Center | Align::CenterAcross => TextAlign::Center,
                Align::Right => TextAlign::Right,
                Align::General => match d.value {
                    Value::Number(_) => TextAlign::Right,
                    Value::Bool(_) | Value::Error(_) => TextAlign::Center,
                    _ => TextAlign::Left,
                },
            };
            // Center Across Selection spreads over the empty cells to the right that
            // share the alignment.
            let mut span = if merged.is_some() { cell_rect.width } else { w };
            if d.style.align == Align::CenterAcross && merged.is_none() {
                for &(col, _, cw) in cols.iter().filter(|(col, _, _)| *col > c.col) {
                    let next = sheet.cells.get(&Cell::new(row, col));
                    let empty = next.is_none_or(|n| n.value.is_empty());
                    let same = next.is_some_and(|n| n.style.align == Align::CenterAcross);
                    if !(empty && same) {
                        break;
                    }
                    span += cw;
                }
                cell_rect.width = span;
            }
            // Left-aligned text runs on into empty neighbours; numbers that do not fit
            // show as ####, as they do in every spreadsheet.
            if !numeric && align == TextAlign::Left && merged.is_none() {
                let next_filled = stored.get(i + 1).map(|(n, _)| n.col);
                for &(col, _, cw) in cols.iter().filter(|(col, _, _)| *col > c.col) {
                    if next_filled.is_some_and(|n| n <= col)
                        || span > 2000
                        || covered(Cell::new(row, col))
                        || merged_rect(Cell::new(row, col)).is_some()
                    {
                        break;
                    }
                    span += cw;
                }
            }
            let style = effect.map(|e| e.style).unwrap_or_default();
            let bold = d.style.bold || style.bold;
            let width = p.measure(&text, font, bold);
            let room = cell_rect.width.saturating_sub(icon_w);
            if numeric && width + 6 > room {
                let hashes = (room.saturating_sub(4) / p.measure("#", font, false).max(1)).max(1);
                text = "#".repeat(hashes as usize);
                align = TextAlign::Right;
            }
            let color = style
                .color
                .map(rgb)
                .or(shown.color.map(rgb))
                .or(d.style.color.map(rgb))
                .unwrap_or(pal.text);
            let ty = cell_rect.y
                + (cell_rect.height as i32 - i32::from(font) - i32::from(font) / 2) / 2
                + 1;
            let (lx, lw) = match align {
                TextAlign::Left => (
                    cell_rect.x + 3 + icon_w as i32,
                    span.saturating_sub(4 + icon_w).max(1),
                ),
                _ => (
                    cell_rect.x + 2 + icon_w as i32,
                    cell_rect.width.saturating_sub(5 + icon_w).max(1),
                ),
            };
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
                if align == TextAlign::Left {
                    span
                } else {
                    cell_rect.width
                },
                cell_rect.height,
            );
            clip_last(p, clip.intersection(cells).unwrap_or(clip));
            if d.style.underline || style.underline {
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
    let active_range = sheet
        .merge_at(book.active)
        .unwrap_or(Range::single(book.active));
    if let Some(r) = sel_rect {
        let clipped = r.intersection(cells).unwrap_or(r);
        if sel != active_range {
            p.box_(clipped, pal.sel_fill, 0);
            if let Some(a) = range_rect(&rows, &cols, geom, active_range, cells) {
                p.box_(a.intersection(cells).unwrap_or(a), Color::WHITE, 0);
                // The active cell's own text stays readable over the clearing.
                if let Some(d) = book.workbook.cell(sheet_index, book.active) {
                    let text = cw_sheet::format::format(&d.value, &d.style.format).text;
                    let right = matches!(d.value, Value::Number(_));
                    p.label(
                        a.x + 3,
                        a.y + (a.height as i32 - i32::from(font) - i32::from(font) / 2) / 2 + 1,
                        a.width.saturating_sub(6).max(1),
                        &text,
                        font,
                        pal.text,
                        d.style.bold,
                        if d.style.align == Align::Center || d.style.align == Align::CenterAcross {
                            TextAlign::Center
                        } else if right {
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
        if cells.contains(handle.x, handle.y) && book.editing.is_none() && book.draw.is_none() {
            p.box_(handle, Color::WHITE, 0);
            p.box_(
                Rect::new(handle.x + 1, handle.y + 1, 5, 5),
                pal.sel_border,
                0,
            );
            p.region(handle, &geom.target("fill"), "Fill handle");
        }
    }
    // A fill in progress outlines where it will land; so does a border being drawn.
    if let Some(Drag::Fill { source, to }) = &book.drag {
        let target = super::fill_target(*source, *to);
        if let Some(r) = range_rect(&rows, &cols, geom, target, cells) {
            dashed(p, r, Color::rgb(120, 120, 120));
        }
    }
    if let Some(Drag::Border { from, to }) = &book.drag {
        if let Some(r) = range_rect(&rows, &cols, geom, Range::new(*from, *to), cells) {
            dashed(p, r, rgb(book.pen.color));
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
    // The references of a formula being typed, each outlined in its colour.
    if let Some(e) = book.editing.as_ref().filter(|e| e.text.starts_with('=')) {
        let spans = cw_sheet::parser::reference_spans(&e.text);
        let order = ref_order(&spans);
        for (k, sp) in spans.iter().enumerate() {
            let here = sp
                .sheet
                .as_ref()
                .is_none_or(|n| n.eq_ignore_ascii_case(&sheet.name));
            if !here || pal.ref_colors.is_empty() {
                continue;
            }
            let color = pal.ref_colors[order[k] % pal.ref_colors.len()];
            let range = sheet.expand_merges(sp.range);
            let Some(r) = range_rect(&rows, &cols, geom, range, cells) else {
                continue;
            };
            let mark = p.scene.nodes.len();
            p.box_(r, Color(color.0, color.1, color.2, 28), 0);
            for (dx, dy, w, h) in [
                (0, 0, r.width, 2),
                (0, r.height as i32 - 2, r.width, 2),
                (0, 0, 2, r.height),
                (r.width as i32 - 2, 0, 2, r.height),
            ] {
                p.box_(Rect::new(r.x + dx, r.y + dy, w, h), color, 0);
            }
            // Excel's corner handles.
            for (hx, hy) in [
                (r.x, r.y),
                (r.x + r.width as i32, r.y),
                (r.x, r.y + r.height as i32),
                (r.x + r.width as i32, r.y + r.height as i32),
            ] {
                p.box_(Rect::new(hx - 2, hy - 2, 5, 5), color, 0);
            }
            clip_from(p, mark, cells);
        }
    }
    // Charts float over the grid at their anchor; a selected one can be dragged by
    // its body and resized by its handles.
    for (i, chart) in sheet.charts.iter().enumerate() {
        let b = book.chart_box(chart);
        let rel = chart_screen(book, geom, b);
        let r = Rect::new(cells.x + rel.x, cells.y + rel.y, rel.width, rel.height);
        if r.x > cells.x + cells.width as i32
            || r.y > cells.y + cells.height as i32
            || r.x + r.width as i32 <= cells.x
            || r.y + r.height as i32 <= cells.y
        {
            continue;
        }
        let mark = p.scene.nodes.len();
        super::chart::paint(p, &book.workbook, sheet_index, chart, r, pal);
        p.region(
            r,
            &format!("sheet:chartmove:{i}:{}:{}", geom.row_h, geom.scale),
            &format!("Chart {}", chart.title),
        );
        if book.chart == Some(i) {
            p.border(r, Color::TRANSPARENT, 0, pal.sel_border);
            let (w, h) = (r.width as i32, r.height as i32);
            let handles = [
                (0, 0),
                (w / 2, 0),
                (w, 0),
                (w, h / 2),
                (w, h),
                (w / 2, h),
                (0, h),
                (0, h / 2),
            ];
            for (k, (hx, hy)) in handles.iter().enumerate() {
                let hr = Rect::new(r.x + hx - 4, r.y + hy - 4, 9, 9);
                p.box_(hr, Color::WHITE, 1);
                p.border(hr, Color::TRANSPARENT, 1, pal.sel_border);
                p.region(
                    hr,
                    &format!("sheet:chartsize:{i}:{k}:{}:{}", geom.row_h, geom.scale),
                    "Chart sizing handle",
                );
            }
        }
        clip_from(p, mark, cells);
    }
    // A chart being dragged shows where it will land.
    if let Some((_, b)) = book.dragged_box(geom) {
        let rel = chart_screen(book, geom, b);
        let r = Rect::new(cells.x + rel.x, cells.y + rel.y, rel.width, rel.height);
        let mark = p.scene.nodes.len();
        dashed(p, r, Color::rgb(90, 90, 90));
        clip_from(p, mark, cells);
    }
    // In-cell editing: the editor covers the cell and grows to fit its text.
    if let Some(e) = &book.editing {
        if !e.in_bar {
            if let Some(a) = range_rect(&rows, &cols, geom, active_range, cells) {
                let tw = p.measure(&e.text, font, false) + 12;
                let r = Rect::new(a.x, a.y, a.width.max(tw), a.height);
                p.box_(r, Color::WHITE, 0);
                p.border(r, Color::TRANSPARENT, 0, pal.sel_border);
                formula_text(
                    p,
                    r.x + 3,
                    r.y + (r.height as i32 - i32::from(font) - i32::from(font) / 2) / 2 + 1,
                    r.width.saturating_sub(4),
                    &e.text,
                    font,
                    pal.text,
                    pal.ref_colors,
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
