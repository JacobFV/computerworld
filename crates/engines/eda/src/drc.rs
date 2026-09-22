//! Board connectivity, the ratsnest, and the design rules check.
use crate::footprints::PadKind;
use crate::geom::{in_polygon, polygon_edge_distance, Pt, Rect};
use crate::pcb::{Board, Copper, Owner, Shape};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A connection still to be routed: two points on the same net in different islands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RatLine {
    pub net: usize,
    pub a: Pt,
    pub b: Pt,
}

struct Dsu(Vec<usize>);
impl Dsu {
    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            self.0[hi] = lo;
        }
    }
}

/// Points a ratsnest line may attach to for one copper item.
fn anchors(board: &Board, c: &Copper) -> Vec<Pt> {
    match c.owner {
        Owner::Pad(f, i) => board
            .footprint(f)
            .map(|fp| vec![fp.pad_pos(&fp.pads[i])])
            .unwrap_or_default(),
        Owner::Track(_) | Owner::Via(_) => match c.shape {
            Shape::Seg { a, b, .. } => {
                if a == b {
                    vec![a]
                } else {
                    vec![a, b]
                }
            }
            other => vec![other.bbox().center()],
        },
        Owner::Zone(..) => match c.shape {
            Shape::Seg { a, .. } => vec![a],
            other => vec![other.bbox().center()],
        },
    }
}

/// Group every copper item of each net into electrically connected islands. Items of
/// the same owner (a through-hole pad's two layers, a via) are one island.
pub fn islands(board: &Board) -> (Vec<Copper>, Vec<usize>) {
    let copper = board.copper();
    let n = copper.len();
    let mut dsu = Dsu((0..n).collect());
    let mut owners: BTreeMap<(u8, u64, usize), usize> = BTreeMap::new();
    for (i, c) in copper.iter().enumerate() {
        let key = match c.owner {
            Owner::Pad(f, p) => (0, f, p),
            Owner::Via(v) => (1, v, 0),
            Owner::Track(t) => (2, t, 0),
            Owner::Zone(z, r) => (3, z, r),
        };
        if let Some(&first) = owners.get(&key) {
            dsu.union(first, i);
        } else {
            owners.insert(key, i);
        }
    }
    // Same net, same layer, touching.
    let boxes: Vec<Rect> = copper.iter().map(|c| c.shape.bbox()).collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|i| (copper[*i].net, copper[*i].layer, boxes[*i].min.x));
    let mut start = 0;
    while start < n {
        let (net, layer) = (copper[order[start]].net, copper[order[start]].layer);
        let mut end = start;
        while end < n && copper[order[end]].net == net && copper[order[end]].layer == layer {
            end += 1;
        }
        if net != 0 {
            for x in start..end {
                let i = order[x];
                for &j in &order[x + 1..end] {
                    if boxes[j].min.x > boxes[i].max.x {
                        break;
                    }
                    if boxes[i].inflate(1).overlaps(&boxes[j].inflate(1))
                        && copper[i].shape.distance(&copper[j].shape) <= 1.0
                    {
                        dsu.union(i, j);
                    }
                }
            }
        }
        start = end;
    }
    let roots = (0..n).map(|i| dsu.find(i)).collect();
    (copper, roots)
}

/// The ratsnest: for each net, a minimum spanning tree between its islands, drawn
/// between the closest anchor points.
pub fn ratsnest(board: &Board) -> Vec<RatLine> {
    let (copper, roots) = islands(board);
    let mut by_net: BTreeMap<usize, BTreeMap<usize, Vec<Pt>>> = BTreeMap::new();
    for (i, c) in copper.iter().enumerate() {
        if c.net == 0 {
            continue;
        }
        // Zone fill alone does not make an island worth connecting: a zone island
        // with no pad, track or via on it is not something to route to.
        by_net
            .entry(c.net)
            .or_default()
            .entry(roots[i])
            .or_default()
            .extend(anchors(board, c));
    }
    let mut lines = Vec::new();
    for (net, groups) in by_net {
        // Drop islands that are only zone copper with nothing else in them.
        let groups: Vec<Vec<Pt>> = groups
            .into_iter()
            .filter(|(root, _)| {
                copper
                    .iter()
                    .enumerate()
                    .any(|(i, c)| roots[i] == *root && !matches!(c.owner, Owner::Zone(..)))
            })
            .map(|(_, pts)| pts)
            .collect();
        if groups.len() < 2 {
            continue;
        }
        // Prim's algorithm over islands.
        let mut in_tree = vec![false; groups.len()];
        in_tree[0] = true;
        for _ in 1..groups.len() {
            let mut best: Option<(i128, Pt, Pt, usize)> = None;
            for (gi, g) in groups.iter().enumerate() {
                if !in_tree[gi] {
                    continue;
                }
                for (hi, h) in groups.iter().enumerate() {
                    if in_tree[hi] {
                        continue;
                    }
                    for a in g {
                        for b in h {
                            let d = (a.x - b.x) as i128 * (a.x - b.x) as i128
                                + (a.y - b.y) as i128 * (a.y - b.y) as i128;
                            if best.is_none_or(|x| d < x.0) {
                                best = Some((d, *a, *b, hi));
                            }
                        }
                    }
                }
            }
            let Some((_, a, b, hi)) = best else { break };
            in_tree[hi] = true;
            lines.push(RatLine { net, a, b });
        }
    }
    lines
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    pub severity: Severity,
    pub rule: String,
    pub message: String,
    pub pos: Pt,
    pub items: Vec<String>,
}

fn describe(board: &Board, c: &Copper) -> String {
    let net = board.net_name(c.net);
    let net = if net.is_empty() {
        "<no net>".to_owned()
    } else {
        format!("[{net}]")
    };
    match c.owner {
        Owner::Pad(f, i) => board
            .footprint(f)
            .map(|fp| {
                format!(
                    "Pad {} {net} of {} on {}",
                    fp.pads[i].number,
                    fp.reference,
                    c.layer.name()
                )
            })
            .unwrap_or_default(),
        Owner::Track(_) => format!("Track {net} on {}", c.layer.name()),
        Owner::Via(_) => format!("Via {net} on F.Cu - B.Cu"),
        Owner::Zone(..) => format!("Zone {net} on {}", c.layer.name()),
    }
}
fn mm(v: f64) -> String {
    format!("{:.4} mm", v / 1e6)
}
fn centre(c: &Copper) -> Pt {
    c.shape.bbox().center()
}

/// Run every rule. `schematic_refs` enables the schematic parity checks: references
/// that should be on the board.
pub fn check(board: &Board, schematic_refs: Option<&[(String, String)]>) -> Vec<Violation> {
    let mut out = Vec::new();
    let rules = &board.rules;
    let outline = board.outline();
    if outline.is_none() {
        out.push(Violation {
            severity: Severity::Error,
            rule: "invalid_outline".into(),
            message: "Board has malformed outline (no closed Edge.Cuts outline found)".into(),
            pos: Pt::default(),
            items: vec![],
        });
    }
    let copper = board.copper();
    // Clearance between different nets on the same layer.
    let boxes: Vec<Rect> = copper.iter().map(|c| c.shape.bbox()).collect();
    let mut order: Vec<usize> = (0..copper.len()).collect();
    order.sort_by_key(|i| (copper[*i].layer, boxes[*i].min.x));
    let mut reported = std::collections::BTreeSet::new();
    for x in 0..order.len() {
        let i = order[x];
        for &j in &order[x + 1..] {
            if copper[j].layer != copper[i].layer {
                break;
            }
            if boxes[j].min.x > boxes[i].max.x + rules.clearance {
                break;
            }
            let (a, b) = (&copper[i], &copper[j]);
            if a.owner == b.owner || (a.net == b.net && a.net != 0) {
                continue;
            }
            // Items of one footprint's pad on two layers, or the same via: skip.
            let same_item = match (a.owner, b.owner) {
                (Owner::Pad(f, p), Owner::Pad(g, q)) => f == g && p == q,
                (Owner::Zone(z, _), Owner::Zone(w, _)) => z == w,
                _ => false,
            };
            if same_item {
                continue;
            }
            if !boxes[i].inflate(rules.clearance).overlaps(&boxes[j]) {
                continue;
            }
            let d = a.shape.distance(&b.shape);
            if d < rules.clearance as f64 {
                let key = (a.owner.min(b.owner), a.owner.max(b.owner));
                if !reported.insert(key) {
                    continue;
                }
                let (rule, message) = if d == 0.0 {
                    ("shorting_items", "Items shorting two nets".to_owned())
                } else {
                    (
                        "clearance",
                        format!(
                            "Clearance violation (netclass 'Default' clearance {}; actual {})",
                            mm(rules.clearance as f64),
                            mm(d)
                        ),
                    )
                };
                out.push(Violation {
                    severity: Severity::Error,
                    rule: rule.into(),
                    message,
                    pos: centre(if matches!(a.owner, Owner::Track(_)) {
                        a
                    } else {
                        b
                    }),
                    items: vec![describe(board, a), describe(board, b)],
                });
            }
        }
    }
    // Track widths.
    for t in &board.tracks {
        if t.width < rules.min_track_width {
            out.push(Violation {
                severity: Severity::Error,
                rule: "track_width".into(),
                message: format!(
                    "Track width (board setup constraints min track width {}; actual {})",
                    mm(rules.min_track_width as f64),
                    mm(t.width as f64)
                ),
                pos: Pt::new((t.a.x + t.b.x) / 2, (t.a.y + t.b.y) / 2),
                items: vec![format!(
                    "Track [{}] on {}",
                    board.net_name(t.net),
                    t.layer.name()
                )],
            });
        }
    }
    // Annular rings and hole sizes.
    for v in &board.vias {
        let ring = (v.diameter - v.drill) / 2;
        if ring < rules.min_annular_ring {
            out.push(Violation {
                severity: Severity::Error,
                rule: "annular_width".into(),
                message: format!(
                    "Annular width (board setup constraints min annular width {}; actual {})",
                    mm(rules.min_annular_ring as f64),
                    mm(ring as f64)
                ),
                pos: v.pos,
                items: vec![format!("Via [{}] on F.Cu - B.Cu", board.net_name(v.net))],
            });
        }
        if v.drill < rules.min_through_hole {
            out.push(Violation {
                severity: Severity::Error,
                rule: "drill_out_of_range".into(),
                message: format!(
                    "Hole size out of range (board setup constraints min hole {}; actual {})",
                    mm(rules.min_through_hole as f64),
                    mm(v.drill as f64)
                ),
                pos: v.pos,
                items: vec![format!("Via [{}]", board.net_name(v.net))],
            });
        }
    }
    let mut holes: Vec<(Pt, i64, String)> = board
        .vias
        .iter()
        .map(|v| (v.pos, v.drill, format!("Via [{}]", board.net_name(v.net))))
        .collect();
    for f in &board.footprints {
        for p in &f.pads {
            if p.kind != PadKind::ThroughHole {
                continue;
            }
            let (w, h) = p.size;
            let ring = (w.min(h) - p.drill) / 2;
            if ring < rules.min_annular_ring {
                out.push(Violation {
                    severity: Severity::Error,
                    rule: "annular_width".into(),
                    message: format!(
                        "Annular width (board setup constraints min annular width {}; actual {})",
                        mm(rules.min_annular_ring as f64),
                        mm(ring as f64)
                    ),
                    pos: f.pad_pos(p),
                    items: vec![format!("Pad {} of {}", p.number, f.reference)],
                });
            }
            holes.push((
                f.pad_pos(p),
                p.drill,
                format!("Pad {} of {}", p.number, f.reference),
            ));
        }
    }
    for (i, a) in holes.iter().enumerate() {
        for b in &holes[i + 1..] {
            let gap = a.0.dist(b.0) - (a.1 + b.1) as f64 / 2.0;
            if a.0 != b.0 && gap < rules.hole_to_hole as f64 {
                out.push(Violation {
                    severity: Severity::Error,
                    rule: "hole_to_hole".into(),
                    message: format!(
                        "Drilled holes too close together (board setup constraints hole to hole {}; actual {})",
                        mm(rules.hole_to_hole as f64),
                        mm(gap.max(0.0))
                    ),
                    pos: a.0,
                    items: vec![a.2.clone(), b.2.clone()],
                });
            }
        }
    }
    // Copper against the board edge.
    if let Some(poly) = &outline {
        for c in &copper {
            let b = c.shape.bbox();
            let corners = [
                b.min,
                Pt::new(b.max.x, b.min.y),
                b.max,
                Pt::new(b.min.x, b.max.y),
            ];
            let inside = corners.iter().all(|p| in_polygon(*p, poly));
            let edge_gap = match c.shape {
                Shape::Seg { a, b, r } => {
                    let n = poly.len();
                    (0..n)
                        .map(|k| crate::geom::segment_segment(a, b, poly[k], poly[(k + 1) % n]))
                        .fold(f64::INFINITY, f64::min)
                        - r as f64
                }
                Shape::Rect(r) => corners
                    .iter()
                    .map(|p| polygon_edge_distance(*p, poly))
                    .fold(f64::INFINITY, f64::min)
                    .min(polygon_edge_distance(r.center(), poly)),
                Shape::Poly(q) => {
                    // A turned pad: its own corners against the outline's edges.
                    let n = poly.len();
                    (0..4)
                        .map(|i| {
                            (0..n)
                                .map(|k| {
                                    crate::geom::segment_segment(
                                        q[i],
                                        q[(i + 1) % 4],
                                        poly[k],
                                        poly[(k + 1) % n],
                                    )
                                })
                                .fold(f64::INFINITY, f64::min)
                        })
                        .fold(f64::INFINITY, f64::min)
                }
            };
            if !inside || edge_gap < rules.copper_edge_clearance as f64 {
                if matches!(c.owner, Owner::Zone(..)) && inside {
                    continue;
                }
                let key = (c.owner, c.owner);
                if !reported.insert(key) {
                    continue;
                }
                out.push(Violation {
                    severity: Severity::Error,
                    rule: "copper_edge_clearance".into(),
                    message: format!(
                        "Board edge clearance violation (board setup constraints edge clearance {}; actual {})",
                        mm(rules.copper_edge_clearance as f64),
                        mm(if inside { edge_gap.max(0.0) } else { 0.0 })
                    ),
                    pos: centre(c),
                    items: vec![describe(board, c)],
                });
            }
        }
    }
    // Courtyards, turned with their footprints.
    let yards: Vec<(usize, bool, Rect, [Pt; 4])> = board
        .footprints
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            f.courtyard_poly()
                .map(|p| (i, f.back, crate::pcb::bbox_of(&p), p))
        })
        .collect();
    for (k, (i, back, r, pa)) in yards.iter().enumerate() {
        for (j, back2, s, pb) in &yards[k + 1..] {
            if back == back2 && r.overlaps(s) && crate::pcb::convex_overlap(pa, pb) {
                let (a, b) = (&board.footprints[*i], &board.footprints[*j]);
                out.push(Violation {
                    severity: Severity::Error,
                    rule: "courtyards_overlap".into(),
                    message: "Courtyards overlap".into(),
                    pos: Rect::new(
                        Pt::new(r.min.x.max(s.min.x), r.min.y.max(s.min.y)),
                        Pt::new(r.max.x.min(s.max.x), r.max.y.min(s.max.y)),
                    )
                    .center(),
                    items: vec![
                        format!(
                            "Footprint {} on {}",
                            a.reference,
                            if *back { "B.Courtyard" } else { "F.Courtyard" }
                        ),
                        format!("Footprint {}", b.reference),
                    ],
                });
            }
        }
    }
    // Rule areas: no copper inside a keepout.
    for z in board
        .zones
        .iter()
        .filter(|z| z.keepout && z.outline.len() >= 3)
    {
        for c in copper.iter().filter(|c| c.layer == z.layer) {
            if shape_enters(&c.shape, &z.outline) {
                let key = (c.owner, Owner::Zone(z.id, usize::MAX));
                if !reported.insert(key) {
                    continue;
                }
                out.push(Violation {
                    severity: Severity::Error,
                    rule: "items_not_allowed".into(),
                    message: format!("Items not allowed (keepout area on {})", z.layer.name()),
                    pos: centre(c),
                    items: vec![describe(board, c), "Rule area".into()],
                });
            }
        }
    }
    // Dangling tracks: an end touching nothing else of its net.
    for t in &board.tracks {
        for end in [t.a, t.b] {
            let touching = copper.iter().any(|c| {
                c.layer == t.layer
                    && c.owner != Owner::Track(t.id)
                    && c.net == t.net
                    && c.shape.point_distance(end) <= (t.width / 2) as f64
            });
            if !touching {
                out.push(Violation {
                    severity: Severity::Warning,
                    rule: "track_dangling".into(),
                    message: "Track has unconnected end".into(),
                    pos: end,
                    items: vec![format!(
                        "Track [{}] on {}",
                        board.net_name(t.net),
                        t.layer.name()
                    )],
                });
            }
        }
    }
    // Schematic parity.
    if let Some(refs) = schematic_refs {
        for (reference, fp) in refs {
            if !board.footprints.iter().any(|f| &f.reference == reference) {
                out.push(Violation {
                    severity: Severity::Warning,
                    rule: "missing_footprint".into(),
                    message: format!("Missing footprint {reference} ({fp})"),
                    pos: Pt::default(),
                    items: vec![format!("Symbol {reference}")],
                });
            }
        }
        for f in &board.footprints {
            if !refs.iter().any(|(r, _)| *r == f.reference) {
                out.push(Violation {
                    severity: Severity::Warning,
                    rule: "extra_footprint".into(),
                    message: format!("Footprint {} not found in schematic", f.reference),
                    pos: f.pos,
                    items: vec![format!("Footprint {}", f.reference)],
                });
            }
        }
    }
    out.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then(a.rule.cmp(&b.rule))
            .then(a.pos.cmp(&b.pos))
    });
    out
}

/// Whether a copper shape reaches inside polygon `poly` (touching its edge counts).
pub fn shape_enters(shape: &Shape, poly: &[Pt]) -> bool {
    let n = poly.len();
    let edge_hits = |a: Pt, b: Pt, r: f64| {
        (0..n).any(|k| crate::geom::segment_segment(a, b, poly[k], poly[(k + 1) % n]) < r)
    };
    match shape {
        Shape::Seg { a, b, r } => {
            in_polygon(*a, poly) || in_polygon(*b, poly) || edge_hits(*a, *b, (*r as f64).max(1.0))
        }
        other => {
            let c = other.corners().expect("rectangular shapes have corners");
            c.iter().any(|p| in_polygon(*p, poly))
                || (0..4).any(|i| edge_hits(c[i], c[(i + 1) % 4], 1.0))
                || in_polygon(poly[0], &c)
        }
    }
}

/// "Unconnected items" as DRC lists them: every ratsnest line still to route.
pub fn unconnected(board: &Board) -> Vec<Violation> {
    ratsnest(board)
        .into_iter()
        .map(|l| Violation {
            severity: Severity::Error,
            rule: "unconnected_items".into(),
            message: format!(
                "Missing connection between items [{}]",
                board.net_name(l.net)
            ),
            pos: l.a,
            items: vec![],
        })
        .collect()
}
