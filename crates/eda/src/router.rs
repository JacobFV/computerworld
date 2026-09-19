//! The interactive router's geometry, after KiCad's push-and-shove router (PNS): every
//! obstacle a new track must keep clear of — pads, tracks and vias of other nets on the
//! layer, rule areas, the board edge — is grown by the clearance plus half the track
//! width into an octagonal hull (the Minkowski sum with an octagon, as PNS's
//! `OctagonalHull` does). A track centreline that stays outside every hull keeps the
//! clearance, and one that runs along hull edges is made of 0°, 45° and 90° segments.
//!
//! *Walkaround* finds the shortest such path from the start to the cursor: a visibility
//! search over the hull corners, joining them with KiCad's two-segment 45° postures.
//! *Highlight collisions* keeps the straight posture and reports what it violates.
//! Everything is integer geometry but the search's lengths, which are ordinary IEEE
//! arithmetic, so a route is the same on every host.
use crate::geom::{in_polygon, segment_segment, Pt, Rect};
use crate::pcb::{posture45, Board, Layer, Owner, Shape};

/// A convex polygon the centreline must stay out of, and what it stands for.
#[derive(Clone, Debug)]
struct Hull {
    pts: Vec<Pt>,
    /// +1 or -1: the side of each edge the interior is on.
    orient: f64,
    bbox: Rect,
    item: String,
}

/// An obstacle's true outline, for collision reports.
#[derive(Clone, Debug)]
struct Obstacle {
    shape: Shape,
    item: String,
}

/// Where a track on `layer` of net `net`, `width` wide, may go.
#[derive(Clone, Debug)]
pub struct Router {
    pub net: usize,
    pub layer: Layer,
    pub width: i64,
    pub clearance: i64,
    hulls: Vec<Hull>,
    obstacles: Vec<Obstacle>,
    /// The board outline, and how far inside it the centreline must stay.
    outline: Option<(Vec<Pt>, f64)>,
}

/// One clearance violation the highlighter shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collision {
    pub at: Pt,
    pub item: String,
}

/// The octagon of inscribed radius `r` about `c`, its corners rounded outwards.
fn octagon(c: Pt, r: i64) -> [Pt; 8] {
    // tan 22.5° = √2 - 1, taken a hair large so the octagon never cuts the circle.
    let k = (r as i128 * 41_421_357 + 99_999_999) / 100_000_000;
    let k = k as i64;
    [
        Pt::new(c.x + r, c.y - k),
        Pt::new(c.x + r, c.y + k),
        Pt::new(c.x + k, c.y + r),
        Pt::new(c.x - k, c.y + r),
        Pt::new(c.x - r, c.y + k),
        Pt::new(c.x - r, c.y - k),
        Pt::new(c.x - k, c.y - r),
        Pt::new(c.x + k, c.y - r),
    ]
}

fn cross(o: Pt, a: Pt, b: Pt) -> i128 {
    (a.x - o.x) as i128 * (b.y - o.y) as i128 - (a.y - o.y) as i128 * (b.x - o.x) as i128
}

/// Convex hull (Andrew's monotone chain), without collinear points.
fn convex_hull(mut pts: Vec<Pt>) -> Vec<Pt> {
    pts.sort();
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut lower: Vec<Pt> = vec![];
    for p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0 {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<Pt> = vec![];
    for p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

impl Hull {
    fn new(pts: Vec<Pt>, item: String) -> Hull {
        let pts = convex_hull(pts);
        let n = pts.len();
        let area: i128 = (0..n)
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                a.x as i128 * b.y as i128 - b.x as i128 * a.y as i128
            })
            .sum();
        Hull {
            bbox: crate::pcb::bbox_of(&pts),
            orient: if area >= 0 { 1.0 } else { -1.0 },
            pts,
            item,
        }
    }
    /// Signed depth test of each edge line: positive strictly inside (by `eps`).
    fn edge_terms(&self, i: usize, p: Pt, d: (f64, f64)) -> (f64, f64, f64) {
        let n = self.pts.len();
        let (a, b) = (self.pts[i], self.pts[(i + 1) % n]);
        let e = ((b.x - a.x) as f64, (b.y - a.y) as f64);
        let len = (e.0 * e.0 + e.1 * e.1).sqrt();
        let num = self.orient * (e.0 * (p.y - a.y) as f64 - e.1 * (p.x - a.x) as f64);
        let den = self.orient * (e.0 * d.1 - e.1 * d.0);
        // Two nanometres of slack: running along an edge is touching, not entering.
        (num, den, 2.0 * len)
    }
    fn contains(&self, p: Pt) -> bool {
        if self.pts.len() < 3 || !self.bbox.contains(p) {
            return false;
        }
        (0..self.pts.len()).all(|i| {
            let (num, _, eps) = self.edge_terms(i, p, (0.0, 0.0));
            num > eps
        })
    }
    /// Whether segment p→q passes through the hull's interior.
    fn blocks(&self, p: Pt, q: Pt) -> bool {
        if self.pts.len() < 3 {
            return false;
        }
        let sb = Rect::new(p, q);
        if !sb.inflate(1).overlaps(&self.bbox) {
            return false;
        }
        let d = ((q.x - p.x) as f64, (q.y - p.y) as f64);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for i in 0..self.pts.len() {
            let (num, den, eps) = self.edge_terms(i, p, d);
            // Inside along the segment where num + t·den > eps.
            if den == 0.0 {
                if num <= eps {
                    return false;
                }
            } else {
                let t = (eps - num) / den;
                if den > 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
            if t0 >= t1 {
                return false;
            }
        }
        t1 - t0 > 1e-9
    }
}

fn describe(board: &Board, owner: Owner, net: usize) -> String {
    let name = board.net_name(net);
    let net = if name.is_empty() {
        "<no net>".to_owned()
    } else {
        format!("[{name}]")
    };
    match owner {
        Owner::Pad(f, i) => board
            .footprint(f)
            .map(|fp| format!("Pad {} {net} of {}", fp.pads[i].number, fp.reference))
            .unwrap_or_default(),
        Owner::Track(_) => format!("Track {net}"),
        Owner::Via(_) => format!("Via {net}"),
        Owner::Zone(..) => format!("Zone {net}"),
    }
}

/// Octilinear length: the straight part plus √2 times the diagonal part.
fn octi_len(a: Pt, b: Pt) -> f64 {
    let (dx, dy) = ((b.x - a.x).abs() as f64, (b.y - a.y).abs() as f64);
    let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
    (hi - lo) + lo * std::f64::consts::SQRT_2
}

/// Drop repeated and collinear corners.
fn simplify(pts: Vec<Pt>) -> Vec<Pt> {
    let mut out: Vec<Pt> = vec![];
    for p in pts {
        if out.last() == Some(&p) {
            continue;
        }
        if out.len() >= 2 && cross(out[out.len() - 2], out[out.len() - 1], p) == 0 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            // Collinear and continuing the same way: the middle corner goes.
            let same_way = (b.x - a.x) as i128 * (p.x - b.x) as i128
                + (b.y - a.y) as i128 * (p.y - b.y) as i128
                >= 0;
            if same_way {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

impl Router {
    pub fn new(board: &Board, net: usize, layer: Layer, width: i64) -> Router {
        let clearance = board.rules.clearance;
        let reach = clearance + width / 2 + 1;
        let mut hulls = vec![];
        let mut obstacles = vec![];
        for c in board.copper() {
            let foreign = c.net != net || net == 0;
            if !foreign || matches!(c.owner, Owner::Zone(..)) {
                // Zones are refilled round the new track, as KiCad's router assumes.
                continue;
            }
            let on_layer = c.layer == layer || matches!(c.owner, Owner::Via(_));
            if !on_layer {
                continue;
            }
            let item = describe(board, c.owner, c.net);
            let pts: Vec<Pt> = match c.shape {
                Shape::Seg { a, b, r } => octagon(a, r + reach)
                    .into_iter()
                    .chain(octagon(b, r + reach))
                    .collect(),
                other => other
                    .corners()
                    .expect("rectangular shapes have corners")
                    .iter()
                    .flat_map(|p| octagon(*p, reach))
                    .collect(),
            };
            if !hulls
                .iter()
                .any(|h: &Hull| h.item == item && h.pts == convex_hull(pts.clone()))
            {
                hulls.push(Hull::new(pts, item.clone()));
            }
            obstacles.push(Obstacle {
                shape: c.shape,
                item,
            });
        }
        for z in board.zones.iter().filter(|z| z.keepout && z.layer == layer) {
            if z.outline.len() < 3 {
                continue;
            }
            // The track edge may not enter the rule area.
            let pts: Vec<Pt> = z
                .outline
                .iter()
                .flat_map(|p| octagon(*p, width / 2 + 1))
                .collect();
            hulls.push(Hull::new(pts, "Rule area (keepout)".into()));
        }
        let outline = board
            .outline()
            .map(|poly| (poly, (board.rules.copper_edge_clearance + width / 2) as f64));
        Router {
            net,
            layer,
            width,
            clearance,
            hulls,
            obstacles,
            outline,
        }
    }

    fn on_board(&self, p: Pt) -> bool {
        match &self.outline {
            Some((poly, keep)) => {
                in_polygon(p, poly) && crate::geom::polygon_edge_distance(p, poly) >= *keep
            }
            None => true,
        }
    }
    fn segment_ok(&self, a: Pt, b: Pt, exempt: &[usize]) -> bool {
        if let Some((poly, keep)) = &self.outline {
            let n = poly.len();
            if (0..n).any(|k| segment_segment(a, b, poly[k], poly[(k + 1) % n]) < *keep) {
                return false;
            }
        }
        !self
            .hulls
            .iter()
            .enumerate()
            .any(|(i, h)| !exempt.contains(&i) && h.blocks(a, b))
    }
    /// The hulls a point is inside: a route may leave them from there but not cross
    /// another.
    fn hulls_at(&self, p: Pt) -> Vec<usize> {
        (0..self.hulls.len())
            .filter(|i| self.hulls[*i].contains(p))
            .collect()
    }
    /// A 45° posture from a to b that keeps clear, trying `diagonal_first` first.
    fn posture(&self, a: Pt, b: Pt, diagonal_first: bool, exempt: &[usize]) -> Option<Vec<Pt>> {
        for diag in [diagonal_first, !diagonal_first] {
            let path = posture45(a, b, diag);
            if path.windows(2).all(|w| self.segment_ok(w[0], w[1], exempt)) {
                return Some(path);
            }
        }
        None
    }

    /// Walkaround: the shortest clear 45° path from `from` to `to`, or `None` when the
    /// target is inside an obstacle's clearance or walled off.
    pub fn walkaround(&self, from: Pt, to: Pt, diagonal_first: bool) -> Option<Vec<Pt>> {
        if from == to {
            return Some(vec![from]);
        }
        // Hulls the ends sit in (a neighbouring pad's clearance at the start pad) do not
        // stop the first and last segments leaving and reaching them.
        let mut exempt = self.hulls_at(from);
        let at_to = self.hulls_at(to);
        exempt.extend(at_to.iter().copied());
        if let Some(p) = self.posture(from, to, diagonal_first, &exempt) {
            return Some(p);
        }
        // Visibility search over hull corners near the two ends, widening until a path
        // is found or the whole board has been tried.
        let span = Rect::new(from, to);
        let reach = span.width().max(span.height()).max(1);
        for grow in [reach / 2 + 3_000_000, 2 * reach + 10_000_000, i64::MAX / 4] {
            let region = span.inflate(grow.min(1_000_000_000_000));
            if let Some(p) = self.search(from, to, diagonal_first, &region, &exempt) {
                return Some(simplify(p));
            }
            if grow >= i64::MAX / 4 {
                break;
            }
        }
        None
    }

    fn search(
        &self,
        from: Pt,
        to: Pt,
        diagonal_first: bool,
        region: &Rect,
        exempt: &[usize],
    ) -> Option<Vec<Pt>> {
        let mut nodes = vec![from, to];
        for (i, h) in self.hulls.iter().enumerate() {
            if !h.bbox.inflate(1).overlaps(region) {
                continue;
            }
            for p in &h.pts {
                if !self.on_board(*p) {
                    continue;
                }
                if self
                    .hulls
                    .iter()
                    .enumerate()
                    .any(|(j, o)| j != i && !exempt.contains(&j) && o.contains(*p))
                {
                    continue;
                }
                if !nodes.contains(p) {
                    nodes.push(*p);
                }
            }
        }
        if nodes.len() > 4000 {
            return None;
        }
        // Dijkstra with edges found as nodes are settled; ties go to the lower index.
        let n = nodes.len();
        let mut dist = vec![f64::INFINITY; n];
        let mut prev: Vec<Option<(usize, Vec<Pt>)>> = vec![None; n];
        let mut done = vec![false; n];
        dist[0] = 0.0;
        loop {
            let mut u = None;
            for i in 0..n {
                if !done[i] && dist[i].is_finite() && u.is_none_or(|k: usize| dist[i] < dist[k]) {
                    u = Some(i);
                }
            }
            let u = u?;
            if u == 1 {
                break;
            }
            done[u] = true;
            for v in 0..n {
                if done[v] || v == u {
                    continue;
                }
                let cost = dist[u] + octi_len(nodes[u], nodes[v]);
                if cost >= dist[v] {
                    continue;
                }
                // Only the ends may pass the hulls they sit in.
                let ex: &[usize] = if u == 0 || v == 1 { exempt } else { &[] };
                if let Some(path) = self.posture(nodes[u], nodes[v], diagonal_first, ex) {
                    dist[v] = cost;
                    prev[v] = Some((u, path));
                }
            }
        }
        let mut pieces = vec![];
        let mut at = 1;
        while let Some((u, path)) = prev[at].clone() {
            pieces.push(path);
            at = u;
        }
        pieces.reverse();
        let mut out = vec![from];
        for p in pieces {
            out.extend(p.into_iter().skip(1));
        }
        Some(out)
    }

    /// What a track along `pts` would violate: each obstacle closer than the clearance.
    pub fn collisions(&self, pts: &[Pt]) -> Vec<Collision> {
        let mut out: Vec<Collision> = vec![];
        for w in pts.windows(2) {
            let track = Shape::Seg {
                a: w[0],
                b: w[1],
                r: self.width / 2,
            };
            for o in &self.obstacles {
                if !o
                    .shape
                    .bbox()
                    .inflate(self.clearance + self.width)
                    .overlaps(&track.bbox())
                {
                    continue;
                }
                if track.distance(&o.shape) < self.clearance as f64
                    && !out.iter().any(|c| c.item == o.item)
                {
                    out.push(Collision {
                        at: o.shape.bbox().center(),
                        item: o.item.clone(),
                    });
                }
            }
        }
        out
    }
}

/// What a via of `diameter` at `pos` on net `net` would come too close to.
pub fn via_collision(board: &Board, net: usize, pos: Pt, diameter: i64) -> Option<String> {
    let via = Shape::Seg {
        a: pos,
        b: pos,
        r: diameter / 2,
    };
    for c in board.copper() {
        if (c.net == net && net != 0) || matches!(c.owner, Owner::Zone(..)) {
            continue;
        }
        if via.distance(&c.shape) < board.rules.clearance as f64 {
            return Some(describe(board, c.owner, c.net));
        }
    }
    for z in board.zones.iter().filter(|z| z.keepout) {
        if crate::drc::shape_enters(&via, &z.outline) {
            return Some("Rule area (keepout)".into());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn octagons_circumscribe_their_circle() {
        let o = octagon(Pt::new(0, 0), 1000);
        for i in 0..8 {
            let (a, b) = (o[i], o[(i + 1) % 8]);
            let d = crate::geom::point_segment(Pt::new(0, 0), a, b);
            assert!(d >= 1000.0 - 1e-9, "edge {i} at {d}");
        }
    }
    #[test]
    fn hulls_block_what_crosses_and_not_what_touches() {
        let h = Hull::new(
            vec![
                Pt::new(0, 0),
                Pt::new(100, 0),
                Pt::new(100, 100),
                Pt::new(0, 100),
            ],
            "box".into(),
        );
        assert!(h.blocks(Pt::new(-50, 50), Pt::new(150, 50)));
        assert!(!h.blocks(Pt::new(-50, 0), Pt::new(150, 0)), "along an edge");
        assert!(!h.blocks(Pt::new(-50, -1), Pt::new(150, -1)));
        assert!(h.blocks(Pt::new(50, 50), Pt::new(50, 60)), "inside");
        assert!(h.contains(Pt::new(50, 50)) && !h.contains(Pt::new(100, 50)));
    }
}
