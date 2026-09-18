//! Copper zone filling. The zone is rasterised on a 0.1 mm grid: a cell is copper when
//! its whole square lies inside the zone outline and the board outline (less the edge
//! clearance), and at least the clearance away from every copper item of another net.
//! Pads of the zone's own net get thermal reliefs (a gap with four spokes); tracks and
//! vias of the net connect solidly. Islands that touch nothing of the net are removed,
//! as KiCad removes them, and the surviving cells merge into rectangles.
//!
//! Every test is made against the cell's centre with half the cell's diagonal added to
//! the required distance, so a cell that passes is clear along its whole square: the
//! fill can only err on the side of more clearance, never less.
use crate::geom::{in_polygon, polygon_edge_distance, Pt, Rect};
use crate::pcb::{Board, Owner, Shape, Zone};

pub const CELL: i64 = 100_000;
const MAX_CELLS: i64 = 4_000_000;

pub fn fill(board: &Board, zone: &Zone) -> Vec<Rect> {
    if zone.outline.len() < 3 {
        return vec![];
    }
    let mut bbox = Rect::new(zone.outline[0], zone.outline[0]);
    for p in &zone.outline {
        bbox = bbox.union(&Rect::new(*p, *p));
    }
    let board_outline = board.outline();
    let mut cell = CELL;
    while (bbox.width() / cell + 1) * (bbox.height() / cell + 1) > MAX_CELLS {
        cell *= 2;
    }
    let half_diag = cell as f64 * std::f64::consts::FRAC_1_SQRT_2;
    let cols = (bbox.width() / cell).max(0) as usize + 1;
    let rows = (bbox.height() / cell).max(0) as usize + 1;
    let origin = bbox.min;
    let centre = |c: usize, r: usize| {
        Pt::new(
            origin.x + c as i64 * cell + cell / 2,
            origin.y + r as i64 * cell + cell / 2,
        )
    };
    let edge = board.rules.copper_edge_clearance as f64;
    let mut on = vec![false; cols * rows];
    for r in 0..rows {
        for c in 0..cols {
            let p = centre(c, r);
            if !in_polygon(p, &zone.outline) || polygon_edge_distance(p, &zone.outline) < half_diag
            {
                continue;
            }
            if let Some(poly) = &board_outline {
                if !in_polygon(p, poly) || polygon_edge_distance(p, poly) < edge + half_diag {
                    continue;
                }
            }
            on[r * cols + c] = true;
        }
    }
    let clearance = zone.clearance.max(board.rules.clearance) as f64;
    let copper = board.copper();
    // Cells covered by a shape's neighbourhood of radius `reach`.
    let cells_near = |shape: &Shape, reach: f64| -> Vec<(usize, usize, f64)> {
        let b = shape.bbox().inflate(reach as i64 + cell);
        let c0 = ((b.min.x - origin.x) / cell).max(0) as usize;
        let r0 = ((b.min.y - origin.y) / cell).max(0) as usize;
        let c1 = (((b.max.x - origin.x) / cell).max(-1) + 1).min(cols as i64) as usize;
        let r1 = (((b.max.y - origin.y) / cell).max(-1) + 1).min(rows as i64) as usize;
        let mut out = vec![];
        for r in r0..r1 {
            for c in c0..c1 {
                let d = shape.point_distance(centre(c, r));
                if d < reach {
                    out.push((c, r, d));
                }
            }
        }
        out
    };
    let mut seeds: Vec<usize> = Vec::new();
    for item in copper.iter().filter(|i| i.layer == zone.layer) {
        if matches!(item.owner, Owner::Zone(z, _) if z == zone.id) {
            continue;
        }
        if item.net != zone.net || zone.net == 0 {
            for (c, r, _) in cells_near(&item.shape, clearance + half_diag) {
                on[r * cols + c] = false;
            }
            continue;
        }
        let thermal = matches!(item.owner, Owner::Pad(..));
        if thermal {
            let Owner::Pad(f, i) = item.owner else {
                unreachable!()
            };
            let Some(fp) = board.footprint(f) else {
                continue;
            };
            let pad = &fp.pads[i];
            let at = fp.pad_pos(pad);
            let half_spoke = zone.spoke_width as f64 / 2.0;
            let half_cell = cell as f64 / 2.0;
            for (c, r, d) in cells_near(&item.shape, zone.thermal_gap as f64 + half_diag) {
                let p = centre(c, r);
                let in_spoke = ((p.x - at.x).abs() as f64 + half_cell <= half_spoke)
                    || ((p.y - at.y).abs() as f64 + half_cell <= half_spoke);
                if !in_spoke {
                    on[r * cols + c] = false;
                } else if d <= half_diag && on[r * cols + c] {
                    seeds.push(r * cols + c);
                }
            }
        } else {
            for (c, r, _) in cells_near(&item.shape, half_diag) {
                if on[r * cols + c] {
                    seeds.push(r * cols + c);
                }
            }
        }
    }
    // Keep only islands connected to the net.
    let mut keep = vec![false; cols * rows];
    let mut stack = seeds;
    while let Some(i) = stack.pop() {
        if keep[i] || !on[i] {
            continue;
        }
        keep[i] = true;
        let (c, r) = (i % cols, i / cols);
        if c > 0 {
            stack.push(i - 1);
        }
        if c + 1 < cols {
            stack.push(i + 1);
        }
        if r > 0 {
            stack.push(i - cols);
        }
        if r + 1 < rows {
            stack.push(i + cols);
        }
    }
    // Merge: horizontal runs, then stack identical runs from consecutive rows.
    let mut rects: Vec<Rect> = Vec::new();
    let mut open: Vec<(usize, usize, usize)> = Vec::new(); // (c0, c1, first row)
    for r in 0..=rows {
        let mut runs = Vec::new();
        if r < rows {
            let mut c = 0;
            while c < cols {
                if keep[r * cols + c] {
                    let start = c;
                    while c < cols && keep[r * cols + c] {
                        c += 1;
                    }
                    runs.push((start, c));
                } else {
                    c += 1;
                }
            }
        }
        let mut still = Vec::new();
        for (c0, c1, r0) in open.drain(..) {
            if runs.contains(&(c0, c1)) {
                still.push((c0, c1, r0));
            } else {
                rects.push(Rect::new(
                    Pt::new(origin.x + c0 as i64 * cell, origin.y + r0 as i64 * cell),
                    Pt::new(origin.x + c1 as i64 * cell, origin.y + r as i64 * cell),
                ));
            }
        }
        for run in runs {
            if !still.iter().any(|(a, b, _)| (*a, *b) == run) {
                still.push((run.0, run.1, r));
            }
        }
        open = still;
    }
    rects.sort_by_key(|r| (r.min.y, r.min.x));
    rects
}

/// Refill every zone on the board.
pub fn fill_all(board: &mut Board) {
    // Zones fill in order; each sees the ones filled before it as obstacles, as KiCad's
    // zone priorities do for equal priorities.
    for i in 0..board.zones.len() {
        board.zones[i].fill.clear();
        board.zones[i].filled = false;
    }
    for i in 0..board.zones.len() {
        let zone = board.zones[i].clone();
        let fill = fill(board, &zone);
        board.zones[i].fill = fill;
        board.zones[i].filled = true;
    }
}
