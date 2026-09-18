//! Charts drawn from their worksheet ranges: clustered columns and bars, lines with
//! markers, and pies, with titles, value axes, gridlines and legends.
use super::grid::Palette;
use crate::desktop_scene::shared::{arc_points, Align};
use crate::desktop_scene::Painter;
use cw_scene::{Color, Rect};
use cw_sheet::{Chart, ChartKind, Workbook};

const INK: Color = Color::rgb(64, 64, 64);
const AXIS: Color = Color::rgb(217, 217, 217);

/// A round step for about `ticks` intervals over `span` (1, 2 or 5 × 10^k).
fn nice_step(span: f64, ticks: f64) -> f64 {
    if span <= 0.0 {
        return 1.0;
    }
    let raw = span / ticks;
    let mut magnitude = 1.0;
    while magnitude * 10.0 <= raw {
        magnitude *= 10.0;
    }
    while magnitude > raw {
        magnitude /= 10.0;
    }
    for m in [1.0, 2.0, 5.0, 10.0] {
        if magnitude * m >= raw {
            return magnitude * m;
        }
    }
    magnitude * 10.0
}
fn label_of(x: f64) -> String {
    cw_sheet::value::general(cw_sheet::format::round_decimal(x, 10))
}

pub fn paint(p: &mut Painter, wb: &Workbook, sheet: usize, chart: &Chart, r: Rect, pal: &Palette) {
    p.box_(r, Color::WHITE, 0);
    p.border(r, Color::TRANSPARENT, 0, Color::rgb(217, 217, 217));
    let data = wb.chart_data(sheet, chart);
    let mut top = r.y + 8;
    if !chart.title.is_empty() {
        p.label(
            r.x + 8,
            top,
            r.width.saturating_sub(16),
            &chart.title,
            14,
            INK,
            false,
            Align::Center,
        );
        top += 26;
    }
    let colors = pal.chart_colors;
    let color = |i: usize| colors[i % colors.len()];
    if data.series.is_empty() || data.categories.is_empty() {
        p.center(
            r.x,
            r.y + r.height as i32 / 2 - 8,
            r.width,
            "No data to plot",
            12,
            Color::rgb(140, 140, 140),
        );
        return;
    }
    // Legend: under the plot for series charts, beside it for a pie.
    let pie = chart.kind == ChartKind::Pie;
    let legend: Vec<(String, Color)> = if pie {
        data.categories
            .iter()
            .enumerate()
            .map(|(i, c)| (c.clone(), color(i)))
            .collect()
    } else {
        data.series
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.clone(), color(i)))
            .collect()
    };
    let legend_w = if pie { 110.min(r.width / 3) } else { 0 };
    let legend_h = if pie { 0 } else { 22 };
    let plot = Rect::new(
        r.x + 10,
        top,
        r.width.saturating_sub(20 + legend_w),
        (r.y + r.height as i32 - legend_h - 6 - top).max(20) as u32,
    );
    if pie {
        let mut y = r.y + (r.height as i32 - legend.len() as i32 * 18) / 2;
        for (name, c) in legend.iter().take(12) {
            let x = r.x + r.width as i32 - legend_w as i32;
            p.box_(Rect::new(x, y + 4, 9, 9), *c, 0);
            p.label(
                x + 14,
                y,
                legend_w.saturating_sub(18),
                name,
                11,
                INK,
                false,
                Align::Left,
            );
            y += 18;
        }
        paint_pie(p, &data.series[0].values, plot, &color);
        return;
    }
    let mut lx = r.x + 10;
    let total_w: u32 = legend
        .iter()
        .map(|(n, _)| p.measure(n, 11, false) + 26)
        .sum();
    if total_w < r.width {
        lx = r.x + (r.width as i32 - total_w as i32) / 2;
    }
    for (name, c) in &legend {
        let ly = r.y + r.height as i32 - legend_h - 2;
        if chart.kind == ChartKind::Line {
            p.box_(Rect::new(lx, ly + 8, 14, 2), *c, 0);
        } else {
            p.box_(Rect::new(lx, ly + 4, 9, 9), *c, 0);
        }
        let w = p.label(lx + 17, ly, 200, name, 11, INK, false, Align::Left);
        lx += w as i32 + 26;
    }
    // Value axis from the data (always including zero), in round steps.
    let values: Vec<f64> = data
        .series
        .iter()
        .flat_map(|s| s.values.iter().flatten().copied())
        .collect();
    let lo = values.iter().copied().fold(0.0f64, f64::min);
    let hi = values.iter().copied().fold(0.0f64, f64::max);
    let step = nice_step(hi - lo, 5.0);
    let axis_lo = (lo / step).floor() * step;
    let mut axis_hi = (hi / step).ceil() * step;
    if axis_hi <= axis_lo {
        axis_hi = axis_lo + step;
    }
    let horizontal = chart.kind == ChartKind::Bar;
    let label_w: u32 = if horizontal {
        60.min(plot.width / 3)
    } else {
        44
    };
    let cat_h = 16;
    // Category labels sit left of a bar chart and under a column chart; value labels
    // take the other edge. Either way the plot loses the same strips.
    let (px, py, pw, ph) = (
        plot.x + label_w as i32,
        plot.y,
        plot.width.saturating_sub(label_w),
        plot.height.saturating_sub(cat_h),
    );
    let span = axis_hi - axis_lo;
    let to_px =
        |v: f64, len: u32| -> i32 { ((v - axis_lo) / span * f64::from(len)).round() as i32 };
    let mut t = axis_lo;
    let mut guard = 0;
    while t <= axis_hi + step / 2.0 && guard < 50 {
        guard += 1;
        if horizontal {
            let x = px + to_px(t, pw);
            p.vline(x, py, ph, AXIS);
            p.label(
                x - 30,
                py + ph as i32 + 2,
                60,
                &label_of(t),
                10,
                INK,
                false,
                Align::Center,
            );
        } else {
            let y = py + ph as i32 - to_px(t, ph);
            p.hline(px, y, pw, AXIS);
            p.label(
                plot.x,
                y - 7,
                label_w - 4,
                &label_of(t),
                10,
                INK,
                false,
                Align::Right,
            );
        }
        t += step;
    }
    let n = data.categories.len();
    let s = data.series.len();
    let zero = if horizontal {
        px + to_px(0.0, pw)
    } else {
        py + ph as i32 - to_px(0.0, ph)
    };
    match chart.kind {
        ChartKind::Column | ChartKind::Bar => {
            let len = if horizontal { ph } else { pw };
            let group = f64::from(len) / n as f64;
            let bar = (group * 0.62 / s as f64).max(1.0);
            for (ci, cat) in data.categories.iter().enumerate() {
                let g0 = f64::from(ci as u32) * group + group * 0.19;
                for (si, series) in data.series.iter().enumerate() {
                    let Some(v) = series.values.get(ci).copied().flatten() else {
                        continue;
                    };
                    let off = (g0 + bar * si as f64).round() as i32;
                    let thick = bar.round().max(1.0) as u32;
                    if horizontal {
                        let x = px + to_px(v, pw);
                        let (a, b) = (zero.min(x), zero.max(x));
                        p.box_(
                            Rect::new(a, py + off, (b - a).max(1) as u32, thick),
                            color(si),
                            0,
                        );
                    } else {
                        let y = py + ph as i32 - to_px(v, ph);
                        let (a, b) = (zero.min(y), zero.max(y));
                        p.box_(
                            Rect::new(px + off, a, thick, (b - a).max(1) as u32),
                            color(si),
                            0,
                        );
                    }
                }
                let centre = (f64::from(ci as u32) * group + group / 2.0).round() as i32;
                if horizontal {
                    p.label(
                        plot.x,
                        py + centre - 7,
                        label_w - 4,
                        cat,
                        10,
                        INK,
                        false,
                        Align::Right,
                    );
                } else {
                    p.label(
                        px + centre - (group as i32) / 2,
                        py + ph as i32 + 2,
                        group.max(8.0) as u32,
                        cat,
                        10,
                        INK,
                        false,
                        Align::Center,
                    );
                }
            }
        }
        ChartKind::Line => {
            let group = f64::from(pw) / n as f64;
            for (si, series) in data.series.iter().enumerate() {
                let mut run: Vec<(i32, i32)> = Vec::new();
                for (ci, v) in series.values.iter().enumerate() {
                    let x = px + (f64::from(ci as u32) * group + group / 2.0).round() as i32;
                    match v {
                        Some(v) => {
                            let y = py + ph as i32 - to_px(*v, ph);
                            run.push((x, y));
                            p.circle(x, y, 3, color(si));
                        }
                        // A blank cell is a gap in the line.
                        None => {
                            if run.len() > 1 {
                                p.line(std::mem::take(&mut run), color(si), 2);
                            }
                            run.clear();
                        }
                    }
                }
                if run.len() > 1 {
                    p.line(run, color(si), 2);
                }
            }
            for (ci, cat) in data.categories.iter().enumerate() {
                let centre = (f64::from(ci as u32) * group + group / 2.0).round() as i32;
                p.label(
                    px + centre - (group as i32) / 2,
                    py + ph as i32 + 2,
                    group.max(8.0) as u32,
                    cat,
                    10,
                    INK,
                    false,
                    Align::Center,
                );
            }
        }
        ChartKind::Pie => {}
    }
    if horizontal {
        p.vline(zero, py, ph, Color::rgb(191, 191, 191));
    } else {
        p.hline(px, zero, pw, Color::rgb(191, 191, 191));
    }
}
fn paint_pie(p: &mut Painter, values: &[Option<f64>], plot: Rect, color: &dyn Fn(usize) -> Color) {
    let vals: Vec<f64> = values.iter().map(|v| v.unwrap_or(0.0).max(0.0)).collect();
    let total: f64 = vals.iter().sum();
    let radius = (plot.width.min(plot.height) / 2).saturating_sub(4) as i32;
    let (cx, cy) = (
        plot.x + plot.width as i32 / 2,
        plot.y + plot.height as i32 / 2,
    );
    if total <= 0.0 || radius < 4 {
        p.center(
            plot.x,
            cy - 8,
            plot.width,
            "No data to plot",
            12,
            Color::rgb(140, 140, 140),
        );
        return;
    }
    let mut acc = 0.0;
    for (i, v) in vals.iter().enumerate() {
        if *v <= 0.0 {
            continue;
        }
        let from = (acc / total * 360.0).round() as i32;
        acc += v;
        let to = (acc / total * 360.0).round() as i32;
        if to <= from {
            continue;
        }
        let mut pts = vec![(cx, cy)];
        pts.extend(arc_points(cx, cy, radius, from, to, 3));
        p.path(pts, color(i));
        // White separators between slices, as every product draws them.
        let edge = arc_points(cx, cy, radius, from, from, 1);
        p.line(vec![(cx, cy), edge[0]], Color::WHITE, 1);
    }
}
