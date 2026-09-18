//! Printed circuit board model: footprints with pads, tracks, vias, board outline and
//! copper zones on a two-layer board, in nanometres with Y down as KiCad keeps them.
use crate::connectivity::{analyze, natural};
use crate::footprints::{self, PadKind, PadShape, MM};
use crate::geom::{Pt, Rect, Xf};
use crate::schematic::Schematic;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Layer {
    FCu,
    BCu,
    FPaste,
    BPaste,
    FSilkS,
    BSilkS,
    FMask,
    BMask,
    EdgeCuts,
    FCrtYd,
    BCrtYd,
    FFab,
    BFab,
}
impl Layer {
    pub const ALL: [Layer; 13] = [
        Layer::FCu,
        Layer::BCu,
        Layer::FPaste,
        Layer::BPaste,
        Layer::FSilkS,
        Layer::BSilkS,
        Layer::FMask,
        Layer::BMask,
        Layer::EdgeCuts,
        Layer::FCrtYd,
        Layer::BCrtYd,
        Layer::FFab,
        Layer::BFab,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::FCu => "F.Cu",
            Self::BCu => "B.Cu",
            Self::FPaste => "F.Paste",
            Self::BPaste => "B.Paste",
            Self::FSilkS => "F.SilkS",
            Self::BSilkS => "B.SilkS",
            Self::FMask => "F.Mask",
            Self::BMask => "B.Mask",
            Self::EdgeCuts => "Edge.Cuts",
            Self::FCrtYd => "F.CrtYd",
            Self::BCrtYd => "B.CrtYd",
            Self::FFab => "F.Fab",
            Self::BFab => "B.Fab",
        }
    }
    /// The layer names KiCad's Appearance panel shows.
    pub fn user_name(self) -> &'static str {
        match self {
            Self::FSilkS => "F.Silkscreen",
            Self::BSilkS => "B.Silkscreen",
            Self::FCrtYd => "F.Courtyard",
            Self::BCrtYd => "B.Courtyard",
            other => other.name(),
        }
    }
    pub fn parse(s: &str) -> Option<Layer> {
        Layer::ALL
            .into_iter()
            .find(|l| l.name() == s || l.user_name() == s)
    }
    /// KiCad's layer number in board files.
    pub fn ordinal(self) -> u8 {
        match self {
            Self::FCu => 0,
            Self::BCu => 31,
            Self::BPaste => 35,
            Self::FPaste => 34,
            Self::BSilkS => 36,
            Self::FSilkS => 37,
            Self::BMask => 38,
            Self::FMask => 39,
            Self::EdgeCuts => 44,
            Self::BCrtYd => 46,
            Self::FCrtYd => 47,
            Self::BFab => 48,
            Self::FFab => 49,
        }
    }
    pub fn is_copper(self) -> bool {
        matches!(self, Self::FCu | Self::BCu)
    }
    /// The same layer on the other side of the board.
    pub fn flipped(self) -> Layer {
        match self {
            Self::FCu => Self::BCu,
            Self::BCu => Self::FCu,
            Self::FPaste => Self::BPaste,
            Self::BPaste => Self::FPaste,
            Self::FSilkS => Self::BSilkS,
            Self::BSilkS => Self::FSilkS,
            Self::FMask => Self::BMask,
            Self::BMask => Self::FMask,
            Self::FCrtYd => Self::BCrtYd,
            Self::BCrtYd => Self::FCrtYd,
            Self::FFab => Self::BFab,
            Self::BFab => Self::FFab,
            Self::EdgeCuts => Self::EdgeCuts,
        }
    }
}

/// Design rules (Board Setup ▸ Constraints and the Default net class).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    pub clearance: i64,
    pub track_width: i64,
    pub via_diameter: i64,
    pub via_drill: i64,
    pub min_track_width: i64,
    pub min_annular_ring: i64,
    pub min_via_diameter: i64,
    pub min_through_hole: i64,
    pub hole_to_hole: i64,
    pub copper_edge_clearance: i64,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            clearance: 200_000,
            track_width: 250_000,
            via_diameter: 600_000,
            via_drill: 300_000,
            min_track_width: 200_000,
            min_annular_ring: 100_000,
            min_via_diameter: 500_000,
            min_through_hole: 300_000,
            hole_to_hole: 250_000,
            copper_edge_clearance: 500_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pad {
    pub number: String,
    pub kind: PadKind,
    pub shape: PadShape,
    /// Position in footprint coordinates (unrotated, front side).
    pub at: Pt,
    pub size: (i64, i64),
    pub drill: i64,
    /// Index into `Board::nets`; 0 is "no net".
    pub net: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FpLine {
    pub layer: Layer,
    pub a: Pt,
    pub b: Pt,
    pub width: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footprint {
    pub id: u64,
    pub fp_id: String,
    pub reference: String,
    pub value: String,
    pub pos: Pt,
    /// Rotation in quarter turns counter-clockwise.
    pub rot: u8,
    pub back: bool,
    pub pads: Vec<Pad>,
    /// Silkscreen, fabrication and courtyard strokes in footprint coordinates.
    pub lines: Vec<FpLine>,
    /// Symbol this footprint came from (its schematic id), for Update PCB.
    pub symbol: Option<u64>,
    #[serde(default)]
    pub locked: bool,
}
impl Footprint {
    fn xf(&self) -> Xf {
        let r = Xf::rotation(self.rot);
        if self.back {
            Xf::MIRROR_Y.then(r)
        } else {
            r
        }
    }
    /// Footprint point to board point.
    pub fn to_board(&self, p: Pt) -> Pt {
        self.pos.add(self.xf().apply(p))
    }
    pub fn pad_pos(&self, pad: &Pad) -> Pt {
        self.to_board(pad.at)
    }
    /// Pad size after rotation: width and height on the board.
    pub fn pad_size(&self, pad: &Pad) -> (i64, i64) {
        if self.rot % 2 == 1 {
            (pad.size.1, pad.size.0)
        } else {
            pad.size
        }
    }
    /// Copper layers the pad is on.
    pub fn pad_layers(&self, pad: &Pad) -> Vec<Layer> {
        match pad.kind {
            PadKind::ThroughHole => vec![Layer::FCu, Layer::BCu],
            PadKind::Smd => vec![if self.back { Layer::BCu } else { Layer::FCu }],
        }
    }
    pub fn layer(&self, l: Layer) -> Layer {
        if self.back {
            l.flipped()
        } else {
            l
        }
    }
    pub fn courtyard(&self) -> Option<Rect> {
        let mut r: Option<Rect> = None;
        for l in self.lines.iter().filter(|l| l.layer == Layer::FCrtYd) {
            let seg = Rect::new(self.to_board(l.a), self.to_board(l.b));
            r = Some(match r {
                Some(r) => r.union(&seg),
                None => seg,
            });
        }
        r
    }
    pub fn bounds(&self) -> Rect {
        let mut r = self
            .courtyard()
            .unwrap_or_else(|| Rect::around(self.pos, MM, MM));
        for p in &self.pads {
            let (w, h) = self.pad_size(p);
            r = r.union(&Rect::around(self.pad_pos(p), w / 2, h / 2));
        }
        r
    }
    /// Copper outline of one pad.
    pub fn pad_shape(&self, pad: &Pad) -> Shape {
        let c = self.pad_pos(pad);
        let (w, h) = self.pad_size(pad);
        match pad.shape {
            PadShape::Circle => Shape::Seg {
                a: c,
                b: c,
                r: w / 2,
            },
            PadShape::Oval => {
                if w == h {
                    Shape::Seg {
                        a: c,
                        b: c,
                        r: w / 2,
                    }
                } else if w > h {
                    let d = (w - h) / 2;
                    Shape::Seg {
                        a: Pt::new(c.x - d, c.y),
                        b: Pt::new(c.x + d, c.y),
                        r: h / 2,
                    }
                } else {
                    let d = (h - w) / 2;
                    Shape::Seg {
                        a: Pt::new(c.x, c.y - d),
                        b: Pt::new(c.x, c.y + d),
                        r: w / 2,
                    }
                }
            }
            PadShape::Rect | PadShape::RoundRect => Shape::Rect(Rect::around(c, w / 2, h / 2)),
        }
    }
    pub fn from_library(
        id: u64,
        fp_id: &str,
        reference: &str,
        value: &str,
        pos: Pt,
    ) -> Option<Self> {
        let lib = footprints::find(fp_id)?;
        let pt = |(x, y): (i64, i64)| Pt::new(x, y);
        let mut lines = Vec::new();
        for (a, b) in &lib.silk {
            lines.push(FpLine {
                layer: Layer::FSilkS,
                a: pt(*a),
                b: pt(*b),
                width: 120_000,
            });
        }
        let rect_lines =
            |layer: Layer, (a, b): ((i64, i64), (i64, i64)), width: i64, out: &mut Vec<FpLine>| {
                let (x0, y0, x1, y1) = (a.0, a.1, b.0, b.1);
                for (p, q) in [
                    ((x0, y0), (x1, y0)),
                    ((x1, y0), (x1, y1)),
                    ((x1, y1), (x0, y1)),
                    ((x0, y1), (x0, y0)),
                ] {
                    out.push(FpLine {
                        layer,
                        a: pt(p),
                        b: pt(q),
                        width,
                    });
                }
            };
        rect_lines(Layer::FFab, lib.fab, 100_000, &mut lines);
        rect_lines(Layer::FCrtYd, lib.courtyard, 50_000, &mut lines);
        Some(Self {
            id,
            fp_id: fp_id.into(),
            reference: reference.into(),
            value: value.into(),
            pos,
            rot: 0,
            back: false,
            pads: lib
                .pads
                .iter()
                .map(|p| Pad {
                    number: p.number.into(),
                    kind: p.kind,
                    shape: p.shape,
                    at: pt(p.at),
                    size: p.size,
                    drill: p.drill,
                    net: 0,
                })
                .collect(),
            lines,
            symbol: None,
            locked: false,
        })
    }
}

/// Copper geometry: a stadium (segment swept by a disc; a disc when a == b) or a rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Seg { a: Pt, b: Pt, r: i64 },
    Rect(Rect),
}
impl Shape {
    /// Edge-to-edge distance; 0 when touching or overlapping.
    pub fn distance(&self, o: &Shape) -> f64 {
        use crate::geom::{rect_rect, segment_rect, segment_segment};
        let d = match (self, o) {
            (Shape::Seg { a, b, r }, Shape::Seg { a: c, b: d, r: s }) => {
                segment_segment(*a, *b, *c, *d) - *r as f64 - *s as f64
            }
            (Shape::Seg { a, b, r }, Shape::Rect(q)) | (Shape::Rect(q), Shape::Seg { a, b, r }) => {
                segment_rect(*a, *b, q) - *r as f64
            }
            (Shape::Rect(p), Shape::Rect(q)) => rect_rect(p, q),
        };
        d.max(0.0)
    }
    pub fn bbox(&self) -> Rect {
        match self {
            Shape::Seg { a, b, r } => Rect::new(*a, *b).inflate(*r),
            Shape::Rect(r) => *r,
        }
    }
    /// Distance from a point to the shape's edge (0 inside).
    pub fn point_distance(&self, p: Pt) -> f64 {
        match self {
            Shape::Seg { a, b, r } => (crate::geom::point_segment(p, *a, *b) - *r as f64).max(0.0),
            Shape::Rect(q) => crate::geom::rect_rect(q, &Rect::new(p, p)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    pub a: Pt,
    pub b: Pt,
    pub width: i64,
    pub layer: Layer,
    pub net: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Via {
    pub id: u64,
    pub pos: Pt,
    pub diameter: i64,
    pub drill: i64,
    pub net: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrawShape {
    Line { a: Pt, b: Pt },
    Rect { a: Pt, b: Pt },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drawing {
    pub id: u64,
    pub layer: Layer,
    pub shape: DrawShape,
    pub width: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub id: u64,
    pub net: usize,
    pub layer: Layer,
    pub outline: Vec<Pt>,
    pub clearance: i64,
    pub min_width: i64,
    pub thermal_gap: i64,
    pub spoke_width: i64,
    /// The fill: rectangles of copper, as computed by `zones::fill`.
    pub fill: Vec<Rect>,
    pub filled: bool,
}

/// Board-level items a selection can hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum BoardItem {
    Footprint(u64),
    Track(u64),
    Via(u64),
    Drawing(u64),
    Zone(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    pub next_id: u64,
    pub rules: Rules,
    /// Net names; index 0 is the unnamed "no net".
    pub nets: Vec<String>,
    pub footprints: Vec<Footprint>,
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub drawings: Vec<Drawing>,
    pub zones: Vec<Zone>,
    pub uuid: String,
}
impl Default for Board {
    fn default() -> Self {
        Self::new("")
    }
}

/// One change Update PCB from Schematic makes, as its dialog lists them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub message: String,
    pub warning: bool,
}

impl Board {
    pub fn new(seed: &str) -> Self {
        Self {
            next_id: 1,
            rules: Rules::default(),
            nets: vec![String::new()],
            footprints: vec![],
            tracks: vec![],
            vias: vec![],
            drawings: vec![],
            zones: vec![],
            uuid: crate::schematic::uuid(seed, u64::MAX >> 16),
        }
    }
    pub fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    pub fn net_index(&mut self, name: &str) -> usize {
        if name.is_empty() {
            return 0;
        }
        match self.nets.iter().position(|n| n == name) {
            Some(i) => i,
            None => {
                self.nets.push(name.into());
                self.nets.len() - 1
            }
        }
    }
    pub fn net_name(&self, net: usize) -> &str {
        self.nets.get(net).map(String::as_str).unwrap_or("")
    }
    pub fn footprint(&self, id: u64) -> Option<&Footprint> {
        self.footprints.iter().find(|f| f.id == id)
    }
    pub fn footprint_mut(&mut self, id: u64) -> Option<&mut Footprint> {
        self.footprints.iter_mut().find(|f| f.id == id)
    }
    /// The board outline: every Edge.Cuts drawing, as a polygon when they close one.
    pub fn outline(&self) -> Option<Vec<Pt>> {
        let edges: Vec<&Drawing> = self
            .drawings
            .iter()
            .filter(|d| d.layer == Layer::EdgeCuts)
            .collect();
        if let Some(r) = edges.iter().find_map(|d| match d.shape {
            DrawShape::Rect { a, b } => Some(Rect::new(a, b)),
            _ => None,
        }) {
            return Some(vec![
                r.min,
                Pt::new(r.max.x, r.min.y),
                r.max,
                Pt::new(r.min.x, r.max.y),
            ]);
        }
        // Chain line segments end to end into a closed loop.
        let mut segs: Vec<(Pt, Pt)> = edges
            .iter()
            .filter_map(|d| match d.shape {
                DrawShape::Line { a, b } => Some((a, b)),
                _ => None,
            })
            .collect();
        let (first, rest) = segs.split_first()?;
        let mut poly = vec![first.0, first.1];
        let mut rest: Vec<(Pt, Pt)> = rest.to_vec();
        while let Some(i) = rest
            .iter()
            .position(|(a, b)| *a == *poly.last().unwrap() || *b == *poly.last().unwrap())
        {
            let (a, b) = rest.remove(i);
            let next = if a == *poly.last().unwrap() { b } else { a };
            if next == poly[0] {
                segs.clear();
                return Some(poly);
            }
            poly.push(next);
        }
        None
    }
    pub fn outline_bounds(&self) -> Option<Rect> {
        let poly = self.outline()?;
        let mut r = Rect::new(poly[0], poly[0]);
        for p in &poly {
            r = r.union(&Rect::new(*p, *p));
        }
        Some(r)
    }

    /// Synchronise footprints and nets with the schematic, as Tools ▸ Update PCB from
    /// Schematic does. New footprints are placed in a row to the right of the board (or
    /// of the origin) for the user to position. Returns the changes, in order.
    pub fn update_from_schematic(&mut self, sch: &Schematic, apply: bool) -> Vec<Change> {
        let conn = analyze(sch);
        let mut changes = Vec::new();
        let mut next = self.clone();
        let mut wanted: Vec<_> = sch
            .symbols
            .iter()
            .filter(|s| !s.is_power() && s.on_board)
            .collect();
        wanted.sort_by_key(|s| natural(s.reference()));
        // New nets first, so pads can refer to them.
        for net in &conn.nets {
            let real = net.pins.iter().filter(|p| !p.power).count();
            if real == 0 {
                continue;
            }
            if !next.nets.contains(&net.name) {
                next.net_index(&net.name);
                changes.push(Change {
                    message: format!("Add net {}.", net.name),
                    warning: false,
                });
            }
        }
        let start = next
            .outline_bounds()
            .map(|r| Pt::new(r.max.x + 5 * MM, r.min.y + 3 * MM))
            .unwrap_or(Pt::new(20 * MM, 20 * MM));
        let mut cursor = start;
        for sym in &wanted {
            if !sym.annotated() {
                changes.push(Change {
                    message: format!("{} is not annotated; skipped.", sym.reference()),
                    warning: true,
                });
                continue;
            }
            let fp_id = sym.field("Footprint").to_owned();
            if fp_id.is_empty() {
                changes.push(Change {
                    message: format!("No footprint assigned to {}; skipped.", sym.reference()),
                    warning: true,
                });
                continue;
            }
            if footprints::find(&fp_id).is_none() {
                changes.push(Change {
                    message: format!(
                        "{}: footprint '{fp_id}' not found in any library.",
                        sym.reference()
                    ),
                    warning: true,
                });
                continue;
            }
            let existing = next
                .footprints
                .iter()
                .position(|f| f.symbol == Some(sym.id) || f.reference == sym.reference());
            let index = match existing {
                Some(i) if next.footprints[i].fp_id != fp_id => {
                    let old = next.footprints[i].clone();
                    let mut f = Footprint::from_library(
                        old.id,
                        &fp_id,
                        sym.reference(),
                        sym.value(),
                        old.pos,
                    )
                    .unwrap();
                    f.rot = old.rot;
                    f.back = old.back;
                    f.symbol = Some(sym.id);
                    changes.push(Change {
                        message: format!(
                            "Change {} footprint from '{}' to '{}'.",
                            sym.reference(),
                            old.fp_id,
                            fp_id
                        ),
                        warning: false,
                    });
                    next.footprints[i] = f;
                    i
                }
                Some(i) => {
                    let f = &mut next.footprints[i];
                    if f.reference != sym.reference() {
                        changes.push(Change {
                            message: format!(
                                "Change {} reference designator to {}.",
                                f.reference,
                                sym.reference()
                            ),
                            warning: false,
                        });
                        f.reference = sym.reference().into();
                    }
                    if f.value != sym.value() {
                        changes.push(Change {
                            message: format!(
                                "Change {} value from '{}' to '{}'.",
                                f.reference,
                                f.value,
                                sym.value()
                            ),
                            warning: false,
                        });
                        f.value = sym.value().into();
                    }
                    f.symbol = Some(sym.id);
                    i
                }
                None => {
                    let id = next.take_id();
                    let mut f =
                        Footprint::from_library(id, &fp_id, sym.reference(), sym.value(), cursor)
                            .unwrap();
                    f.symbol = Some(sym.id);
                    // Lay new parts out left to right, spaced by their own size.
                    let b = f.bounds();
                    let shift = Pt::new(cursor.x - b.min.x, cursor.y - b.min.y);
                    f.pos = f.pos.add(shift);
                    cursor.x += b.width() + 2 * MM;
                    if cursor.x > start.x + 60 * MM {
                        cursor = Pt::new(start.x, cursor.y + 12 * MM);
                    }
                    changes.push(Change {
                        message: format!("Add {} ({}).", sym.reference(), fp_id),
                        warning: false,
                    });
                    next.footprints.push(f);
                    next.footprints.len() - 1
                }
            };
            // Pad nets.
            let reference = sym.reference().to_owned();
            let pads: Vec<(usize, String)> = next.footprints[index]
                .pads
                .iter()
                .enumerate()
                .map(|(i, p)| (i, p.number.clone()))
                .collect();
            for (pi, number) in pads {
                let net_name = conn
                    .net_of_pin(sym.id, &number)
                    .filter(|n| n.pins.iter().filter(|p| !p.power).count() > 1 || n.named)
                    .map(|n| n.name.clone())
                    .unwrap_or_default();
                let net = next.net_index(&net_name);
                let pad = &mut next.footprints[index].pads[pi];
                if pad.net != net {
                    if pad.net != 0 || net != 0 {
                        changes.push(Change {
                            message: if net == 0 {
                                format!("Disconnect {reference} pin {number}.")
                            } else {
                                format!("Connect {reference} pin {number} to {net_name}.")
                            },
                            warning: false,
                        });
                    }
                    pad.net = net;
                }
            }
        }
        // Footprints whose symbol is gone.
        let keep: Vec<u64> = wanted.iter().map(|s| s.id).collect();
        let removed: Vec<String> = next
            .footprints
            .iter()
            .filter(|f| f.symbol.is_some_and(|s| !keep.contains(&s)) && !f.locked)
            .map(|f| f.reference.clone())
            .collect();
        for r in &removed {
            changes.push(Change {
                message: format!("Remove {r}."),
                warning: false,
            });
        }
        next.footprints
            .retain(|f| !(f.symbol.is_some_and(|s| !keep.contains(&s)) && !f.locked));
        if apply {
            *self = next;
        }
        changes
    }

    /// Every copper shape, with its net and the item it belongs to.
    pub fn copper(&self) -> Vec<Copper> {
        let mut out = Vec::new();
        for f in &self.footprints {
            for (i, p) in f.pads.iter().enumerate() {
                let shape = f.pad_shape(p);
                for layer in f.pad_layers(p) {
                    out.push(Copper {
                        layer,
                        net: p.net,
                        shape,
                        owner: Owner::Pad(f.id, i),
                    });
                }
            }
        }
        for t in &self.tracks {
            out.push(Copper {
                layer: t.layer,
                net: t.net,
                shape: Shape::Seg {
                    a: t.a,
                    b: t.b,
                    r: t.width / 2,
                },
                owner: Owner::Track(t.id),
            });
        }
        for v in &self.vias {
            for layer in [Layer::FCu, Layer::BCu] {
                out.push(Copper {
                    layer,
                    net: v.net,
                    shape: Shape::Seg {
                        a: v.pos,
                        b: v.pos,
                        r: v.diameter / 2,
                    },
                    owner: Owner::Via(v.id),
                });
            }
        }
        for z in &self.zones {
            for (i, r) in z.fill.iter().enumerate() {
                out.push(Copper {
                    layer: z.layer,
                    net: z.net,
                    shape: Shape::Rect(*r),
                    owner: Owner::Zone(z.id, i),
                });
            }
        }
        out
    }
    /// The net a point on a copper layer belongs to (a pad, track or via under it).
    pub fn net_at(&self, p: Pt, layer: Layer) -> Option<(usize, Pt)> {
        for f in &self.footprints {
            for pad in &f.pads {
                if f.pad_layers(pad).contains(&layer) && f.pad_shape(pad).point_distance(p) == 0.0 {
                    return Some((pad.net, f.pad_pos(pad)));
                }
            }
        }
        for v in &self.vias {
            if v.pos.dist(p) <= (v.diameter / 2) as f64 {
                return Some((v.net, v.pos));
            }
        }
        for t in &self.tracks {
            if t.layer == layer && crate::geom::point_segment(p, t.a, t.b) <= (t.width / 2) as f64 {
                let end = if t.a.dist(p) <= t.b.dist(p) { t.a } else { t.b };
                let snapped = if end.dist(p) <= (t.width / 2) as f64 {
                    end
                } else {
                    p
                };
                return Some((t.net, snapped));
            }
        }
        None
    }
    pub fn exists(&self, item: BoardItem) -> bool {
        match item {
            BoardItem::Footprint(id) => self.footprints.iter().any(|x| x.id == id),
            BoardItem::Track(id) => self.tracks.iter().any(|x| x.id == id),
            BoardItem::Via(id) => self.vias.iter().any(|x| x.id == id),
            BoardItem::Drawing(id) => self.drawings.iter().any(|x| x.id == id),
            BoardItem::Zone(id) => self.zones.iter().any(|x| x.id == id),
        }
    }
    pub fn delete(&mut self, items: &[BoardItem]) -> usize {
        let before = self.footprints.len()
            + self.tracks.len()
            + self.vias.len()
            + self.drawings.len()
            + self.zones.len();
        self.footprints
            .retain(|x| !items.contains(&BoardItem::Footprint(x.id)));
        self.tracks
            .retain(|x| !items.contains(&BoardItem::Track(x.id)));
        self.vias.retain(|x| !items.contains(&BoardItem::Via(x.id)));
        self.drawings
            .retain(|x| !items.contains(&BoardItem::Drawing(x.id)));
        self.zones
            .retain(|x| !items.contains(&BoardItem::Zone(x.id)));
        before
            - (self.footprints.len()
                + self.tracks.len()
                + self.vias.len()
                + self.drawings.len()
                + self.zones.len())
    }
    pub fn move_items(&mut self, items: &[BoardItem], d: Pt) {
        for f in &mut self.footprints {
            if items.contains(&BoardItem::Footprint(f.id)) {
                f.pos = f.pos.add(d);
            }
        }
        for t in &mut self.tracks {
            if items.contains(&BoardItem::Track(t.id)) {
                t.a = t.a.add(d);
                t.b = t.b.add(d);
            }
        }
        for v in &mut self.vias {
            if items.contains(&BoardItem::Via(v.id)) {
                v.pos = v.pos.add(d);
            }
        }
        for g in &mut self.drawings {
            if items.contains(&BoardItem::Drawing(g.id)) {
                g.shape = match g.shape {
                    DrawShape::Line { a, b } => DrawShape::Line {
                        a: a.add(d),
                        b: b.add(d),
                    },
                    DrawShape::Rect { a, b } => DrawShape::Rect {
                        a: a.add(d),
                        b: b.add(d),
                    },
                };
            }
        }
        for z in &mut self.zones {
            if items.contains(&BoardItem::Zone(z.id)) {
                for p in &mut z.outline {
                    *p = p.add(d);
                }
                for r in &mut z.fill {
                    *r = Rect::new(r.min.add(d), r.max.add(d));
                }
            }
        }
    }
    /// Items under a point, topmost first. Footprints are hit by their courtyard.
    pub fn hit(&self, p: Pt, tol: i64, visible: &dyn Fn(Layer) -> bool) -> Vec<BoardItem> {
        let mut out = Vec::new();
        for v in self.vias.iter().rev() {
            if v.pos.dist(p) <= (v.diameter / 2 + tol) as f64 {
                out.push(BoardItem::Via(v.id));
            }
        }
        for t in self.tracks.iter().rev() {
            if visible(t.layer)
                && crate::geom::point_segment(p, t.a, t.b) <= (t.width / 2 + tol) as f64
            {
                out.push(BoardItem::Track(t.id));
            }
        }
        for f in self.footprints.iter().rev() {
            let side = if f.back { Layer::BCu } else { Layer::FCu };
            if visible(side) && f.bounds().inflate(tol).contains(p) {
                out.push(BoardItem::Footprint(f.id));
            }
        }
        for g in self.drawings.iter().rev() {
            if !visible(g.layer) {
                continue;
            }
            let near = match g.shape {
                DrawShape::Line { a, b } => {
                    crate::geom::point_segment(p, a, b) <= (g.width / 2 + tol) as f64
                }
                DrawShape::Rect { a, b } => {
                    let r = Rect::new(a, b);
                    let c = [
                        r.min,
                        Pt::new(r.max.x, r.min.y),
                        r.max,
                        Pt::new(r.min.x, r.max.y),
                    ];
                    (0..4).any(|i| {
                        crate::geom::point_segment(p, c[i], c[(i + 1) % 4])
                            <= (g.width / 2 + tol) as f64
                    })
                }
            };
            if near {
                out.push(BoardItem::Drawing(g.id));
            }
        }
        for z in self.zones.iter().rev() {
            if visible(z.layer) && crate::geom::polygon_edge_distance(p, &z.outline) <= tol as f64 {
                out.push(BoardItem::Zone(z.id));
            }
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Owner {
    Pad(u64, usize),
    Track(u64),
    Via(u64),
    Zone(u64, usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Copper {
    pub layer: Layer,
    pub net: usize,
    pub shape: Shape,
    pub owner: Owner,
}

/// The 45° router's path from `a` towards `b`: a diagonal and a straight run (straight
/// first unless `diagonal_first`), KiCad's two-segment posture.
pub fn posture45(a: Pt, b: Pt, diagonal_first: bool) -> Vec<Pt> {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let diag = dx.abs().min(dy.abs());
    let (sx, sy) = (dx.signum(), dy.signum());
    let diagonal = Pt::new(sx * diag, sy * diag);
    let straight = Pt::new(dx - diagonal.x, dy - diagonal.y);
    let corner = if diagonal_first {
        a.add(diagonal)
    } else {
        a.add(straight)
    };
    let mut pts = vec![a];
    if corner != a && corner != b {
        pts.push(corner);
    }
    if b != a {
        pts.push(b);
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn router_postures_use_only_45_degree_multiples() {
        let a = Pt::new(0, 0);
        let b = Pt::new(10 * MM, 3 * MM);
        for diag in [false, true] {
            let pts = posture45(a, b, diag);
            assert_eq!(pts.len(), 3);
            for w in pts.windows(2) {
                let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
                assert!(dx == 0 || dy == 0 || dx == dy, "{w:?}");
            }
        }
        assert_eq!(posture45(a, Pt::new(5, 5), false).len(), 2);
    }
    #[test]
    fn footprints_rotate_and_flip_their_pads() {
        let mut f = Footprint::from_library(
            1,
            "Package_DIP:DIP-8_W7.62mm",
            "U1",
            "NE555P",
            Pt::new(0, 0),
        )
        .unwrap();
        let p8 = f.pads.iter().find(|p| p.number == "8").unwrap().clone();
        assert_eq!(f.pad_pos(&p8), Pt::new(7_620_000, 0));
        f.rot = 1;
        assert_eq!(f.pad_pos(&p8), Pt::new(0, -7_620_000));
        f.rot = 0;
        f.back = true;
        assert_eq!(f.pad_pos(&p8), Pt::new(-7_620_000, 0));
        assert_eq!(f.layer(Layer::FSilkS), Layer::BSilkS);
    }
}
