//! The PCB Editor: footprints from the schematic, a board outline, tracks on two copper
//! layers with 45° routing and vias, copper zones, the Appearance panel, DRC and the
//! fabrication outputs.
use super::draw::Cv;
use super::widgets::{self as w, chrome, MenuItem};
use super::{act, icons, live_board_items, Dialog, Drag, DrcReport, Kicad, View};
use crate::desktop_scene::{shared::Align, Painter};
use crate::{AppEffect, PointerPhase};
use cw_eda::drc;
use cw_eda::footprints::{PadKind, PadShape, MM};
use cw_eda::geom::{mm, parse_mm, Pt, Rect as WRect};
use cw_eda::gerber;
use cw_eda::pcb::{
    angle_text, parse_angle, posture45, BoardItem, DrawShape, Drawing, Layer, Shape, Track, Via,
    Zone,
};
use cw_eda::router::{via_collision, Router};
use cw_eda::zones;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

pub const PCB_BG: Color = Color::rgb(0, 16, 35);
const PANEL_W: u32 = 220;
pub(super) const MIN_ZOOM: i64 = 2;
pub(super) const MAX_ZOOM: i64 = 800;
const WIDTHS: [i64; 6] = [250_000, 400_000, 500_000, 800_000, 1_000_000, 1_500_000];

/// A track being routed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub net: usize,
    pub layer: Layer,
    pub width: i64,
    /// Committed corners, first is where routing started.
    pub points: Vec<Pt>,
    /// Layer of the segment ending at each point after the first.
    pub layers: Vec<Layer>,
    pub vias: Vec<Pt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PcbUi {
    /// `select`, `route`, `via`, `zone`, `rect`, `line`.
    pub tool: String,
    pub selection: Vec<BoardItem>,
    /// The active layer: routing goes on it, graphics are drawn on it.
    pub layer: Layer,
    pub hidden: Vec<Layer>,
    pub hide_ratsnest: bool,
    pub route: Option<Route>,
    /// Points of a zone, rectangle or line being drawn.
    pub poly: Vec<Pt>,
    pub track_width: i64,
    pub diagonal_first: bool,
    pub grid: i64,
    /// Router mode: highlight collisions instead of walking around obstacles.
    #[serde(default)]
    pub highlight_collisions: bool,
    /// The walkaround path from the route's last corner to the pointer.
    #[serde(default)]
    pub preview: Vec<Pt>,
    /// What the track being drawn would violate (highlight-collisions mode).
    #[serde(default)]
    pub collisions: Vec<(Pt, String)>,
}
impl PcbUi {
    pub fn new() -> Self {
        Self {
            tool: "select".into(),
            selection: vec![],
            layer: Layer::FCu,
            // Mask and paste are hidden by default so the copper reads clearly.
            hidden: vec![Layer::FMask, Layer::BMask, Layer::FPaste, Layer::BPaste],
            hide_ratsnest: false,
            route: None,
            poly: vec![],
            track_width: 250_000,
            diagonal_first: false,
            grid: 250_000,
            highlight_collisions: false,
            preview: vec![],
            collisions: vec![],
        }
    }
}
impl Default for PcbUi {
    fn default() -> Self {
        Self::new()
    }
}

fn canvas(width: u32, height: u32) -> Rect {
    let top = (w::MENU_H + w::TOOL_H) as i32;
    Rect::new(
        w::SIDE_W as i32 + 1,
        top,
        width.saturating_sub(2 * w::SIDE_W + PANEL_W + 3),
        height.saturating_sub(w::MENU_H + w::TOOL_H + w::STATUS_H),
    )
}
fn um(p: Pt) -> Pt {
    Pt::new(p.x / 1000, p.y / 1000)
}
fn mm_text(v: i64) -> String {
    mm(v, 1_000_000)
}
/// Draw order, back to front, with the active layer lifted to the top of the copper.
fn paint_order(active: Layer) -> Vec<Layer> {
    let mut v = vec![
        Layer::BFab,
        Layer::BCrtYd,
        Layer::BSilkS,
        Layer::BPaste,
        Layer::BMask,
        Layer::BCu,
        Layer::FCu,
        Layer::FMask,
        Layer::FPaste,
        Layer::FSilkS,
        Layer::FCrtYd,
        Layer::FFab,
        Layer::EdgeCuts,
    ];
    if active == Layer::BCu {
        v.retain(|l| *l != Layer::BCu);
        let i = v.iter().position(|l| *l == Layer::FCu).unwrap_or(0) + 1;
        v.insert(i, Layer::BCu);
    }
    v
}
fn layer_color(l: Layer) -> Color {
    let (r, g, b) = cw_eda::svg::color(l);
    match l {
        Layer::FMask | Layer::BMask => Color(r, g, b, 90),
        Layer::FPaste | Layer::BPaste => Color(r, g, b, 200),
        _ => Color::rgb(r, g, b),
    }
}

impl Kicad {
    fn pcb_tool(&self) -> &str {
        if self.ui.pcb.tool.is_empty() {
            "select"
        } else {
            &self.ui.pcb.tool
        }
    }
    pub(super) fn pcb_snap(&self, p: Pt, _hover: bool) -> Pt {
        // Pads and track ends of the active layer attract the cursor, as KiCad's magnetic
        // pads do; everything else snaps to the grid.
        if let Some((_, at)) = self.session.board.net_at(p, self.ui.pcb.layer) {
            return at;
        }
        p.snap(self.ui.pcb.grid.max(1))
    }
    pub(super) fn pcb_view(&self, cw: u32, ch: u32) -> View {
        if !self.ui.view.fit && self.ui.view.zoom != 0 {
            return self.ui.view;
        }
        let b = &self.session.board;
        let r = b.outline_bounds().or_else(|| {
            let mut r: Option<WRect> = None;
            for f in &b.footprints {
                let fb = f.bounds();
                r = Some(r.map_or(fb, |x| x.union(&fb)));
            }
            r
        });
        let r = r.unwrap_or(WRect::new(Pt::new(0, 0), Pt::new(100 * MM, 70 * MM)));
        View::fitted(um(r.min), um(r.max), cw, ch)
    }
    fn pcb_edit(&mut self) {
        let before = self.session.board.clone();
        self.session.pcb_history.record(&before);
        self.session.pcb_dirty = true;
        self.session.drc = None;
    }
    fn board_selected(&self) -> Vec<BoardItem> {
        live_board_items(&self.session.board, &self.ui.pcb.selection)
    }
    fn visible(&self, l: Layer) -> bool {
        !self.ui.pcb.hidden.contains(&l)
    }
    fn tol(&self) -> i64 {
        (4_000_000 / self.ui.view.zoom.max(1)).max(100_000)
    }
    fn finish_route(&mut self) {
        let Some(route) = self.ui.pcb.route.take() else {
            return;
        };
        if route.points.len() < 2 {
            return;
        }
        self.pcb_edit();
        let b = &mut self.session.board;
        for (i, pair) in route.points.windows(2).enumerate() {
            if pair[0] == pair[1] {
                continue;
            }
            let id = b.take_id();
            b.tracks.push(Track {
                id,
                a: pair[0],
                b: pair[1],
                width: route.width,
                layer: route.layers.get(i).copied().unwrap_or(route.layer),
                net: route.net,
            });
        }
        for v in &route.vias {
            let id = b.take_id();
            let (diameter, drill) = (b.rules.via_diameter, b.rules.via_drill);
            b.vias.push(Via {
                id,
                pos: *v,
                diameter,
                drill,
                net: route.net,
            });
        }
        let net = b.net_name(route.net).to_owned();
        self.ui.status = format!(
            "Routed {} segment(s) on {}",
            route.points.len() - 1,
            if net.is_empty() {
                "<no net>".into()
            } else {
                net
            }
        );
    }
    /// The router for the route in progress, on its current layer.
    fn router(&self) -> Option<Router> {
        let route = self.ui.pcb.route.as_ref()?;
        Some(Router::new(
            &self.session.board,
            route.net,
            route.layer,
            route.width,
        ))
    }
    /// Recompute what the route in progress would add to reach the pointer: the
    /// walkaround path, or in highlight mode the straight posture and its collisions.
    pub(super) fn update_route_preview(&mut self) {
        self.ui.pcb.preview.clear();
        self.ui.pcb.collisions.clear();
        let (Some(route), Some(hover)) = (self.ui.pcb.route.as_ref(), self.ui.hover) else {
            return;
        };
        let last = *route.points.last().expect("a route has a start");
        let Some(router) = self.router() else { return };
        if self.ui.pcb.highlight_collisions {
            let path = posture45(last, hover, self.ui.pcb.diagonal_first);
            let mut all = route.points.clone();
            all.extend(path.iter().skip(1));
            self.ui.pcb.collisions = router
                .collisions(&all)
                .into_iter()
                .map(|c| (c.at, c.item))
                .collect();
            self.ui.pcb.preview = path;
        } else if let Some(path) = router.walkaround(last, hover, self.ui.pcb.diagonal_first) {
            self.ui.pcb.preview = path;
        }
    }
    fn close_zone(&mut self) {
        let outline = std::mem::take(&mut self.ui.pcb.poly);
        if outline.len() < 3 {
            self.ui.status = "A zone needs at least three corners".into();
            return;
        }
        if self.pcb_tool() == "keepout" {
            self.ui.dialog = Some(Dialog::ZoneProperties {
                outline,
                net: 0,
                layer: if self.ui.pcb.layer.is_copper() {
                    self.ui.pcb.layer
                } else {
                    Layer::FCu
                },
                clearance: "0".into(),
                keepout: true,
                error: String::new(),
            });
            return;
        }
        let net = self
            .session
            .board
            .nets
            .iter()
            .position(|n| n == "GND")
            .unwrap_or(0);
        self.ui.dialog = Some(Dialog::ZoneProperties {
            outline,
            net,
            layer: if self.ui.pcb.layer.is_copper() {
                self.ui.pcb.layer
            } else {
                Layer::FCu
            },
            clearance: "0.5".into(),
            keepout: false,
            error: String::new(),
        });
        self.ui.focus = Some("clearance".into());
    }
    fn drawing_width(layer: Layer) -> i64 {
        match layer {
            Layer::EdgeCuts => 50_000,
            Layer::FSilkS | Layer::BSilkS => 120_000,
            Layer::FCrtYd | Layer::BCrtYd => 50_000,
            _ => 100_000,
        }
    }

    pub(super) fn pcb_pointer(
        &mut self,
        _window: u64,
        phase: PointerPhase,
        at: Pt,
    ) -> Result<Vec<AppEffect>, String> {
        let snapped = self.pcb_snap(at, false);
        self.ui.hover = Some(snapped);
        let layer = self.ui.pcb.layer;
        match self.pcb_tool().to_owned().as_str() {
            "select" => match phase {
                PointerPhase::Down => {
                    let visible = |l: Layer| !self.ui.pcb.hidden.contains(&l);
                    let hits = self.session.board.hit(at, self.tol(), &visible);
                    match hits.first() {
                        Some(item) => {
                            if !self.ui.pcb.selection.contains(item) {
                                self.ui.pcb.selection = vec![*item];
                            }
                            let start = at.snap(self.ui.pcb.grid.max(1));
                            self.ui.drag = Some(Drag::Move { start, at: start });
                        }
                        None => {
                            self.ui.pcb.selection.clear();
                            self.ui.drag = Some(Drag::Box { start: at, at });
                        }
                    }
                    Ok(vec![])
                }
                PointerPhase::Move => {
                    let g = at.snap(self.ui.pcb.grid.max(1));
                    match &mut self.ui.drag {
                        Some(Drag::Move { at: a, .. }) => *a = g,
                        Some(Drag::Box { at: a, .. }) => *a = at,
                        _ => {}
                    }
                    Ok(vec![])
                }
                PointerPhase::Up => {
                    let g = at.snap(self.ui.pcb.grid.max(1));
                    match self.ui.drag.take() {
                        Some(Drag::Move { start, .. }) => {
                            let d = g.sub(start);
                            let items = self.board_selected();
                            if d != Pt::default() && !items.is_empty() {
                                self.pcb_edit();
                                self.session.board.move_items(&items, d);
                                self.ui.status = format!(
                                    "Moved {} item(s) by {} mm, {} mm",
                                    items.len(),
                                    mm_text(d.x),
                                    mm_text(d.y)
                                );
                            } else {
                                self.ui.status = self.describe_board_selection();
                            }
                        }
                        Some(Drag::Box { start, .. }) => {
                            let r = WRect::new(start, at);
                            if r.width() > self.tol() || r.height() > self.tol() {
                                let b = &self.session.board;
                                let mut sel = vec![];
                                for f in &b.footprints {
                                    let fb = f.bounds();
                                    if r.contains(fb.min) && r.contains(fb.max) {
                                        sel.push(BoardItem::Footprint(f.id));
                                    }
                                }
                                for t in &b.tracks {
                                    if r.contains(t.a) && r.contains(t.b) && self.visible(t.layer) {
                                        sel.push(BoardItem::Track(t.id));
                                    }
                                }
                                for v in &b.vias {
                                    if r.contains(v.pos) {
                                        sel.push(BoardItem::Via(v.id));
                                    }
                                }
                                self.ui.pcb.selection = sel;
                            }
                            self.ui.status = self.describe_board_selection();
                        }
                        _ => {}
                    }
                    Ok(vec![])
                }
                PointerPhase::Cancel => {
                    self.ui.drag = None;
                    Ok(vec![])
                }
            },
            "route" => {
                if phase != PointerPhase::Up {
                    return Ok(vec![]);
                }
                if !layer.is_copper() {
                    return Err("select a copper layer (F.Cu or B.Cu) to route on".into());
                }
                let target = self.session.board.net_at(at, layer);
                match &mut self.ui.pcb.route {
                    None => {
                        let (net, start) = target.unwrap_or((0, snapped));
                        self.ui.pcb.route = Some(Route {
                            net,
                            layer,
                            width: self.ui.pcb.track_width,
                            points: vec![start],
                            layers: vec![],
                            vias: vec![],
                        });
                        let name = self.session.board.net_name(net).to_owned();
                        self.ui.status = format!(
                            "Routing {}: click to add corners, click the destination to finish",
                            if name.is_empty() {
                                "<no net>".into()
                            } else {
                                name
                            }
                        );
                    }
                    Some(route) => {
                        let last = *route.points.last().expect("a route has a start");
                        let end = match target {
                            Some((_, pad)) => pad,
                            None => snapped,
                        };
                        if end == last {
                            return Ok(vec![]);
                        }
                        let (net, rlayer, width) = (route.net, route.layer, route.width);
                        let router = Router::new(&self.session.board, net, rlayer, width);
                        let path = if self.ui.pcb.highlight_collisions {
                            posture45(last, end, self.ui.pcb.diagonal_first)
                        } else {
                            // Walk around: the shortest clear 45° path, or refuse.
                            router
                                .walkaround(last, end, self.ui.pcb.diagonal_first)
                                .ok_or(
                                    "Walkaround: no clear path to that point (an obstacle's clearance is in the way)",
                                )?
                        };
                        let route = self.ui.pcb.route.as_mut().expect("routing");
                        for q in &path[1..] {
                            route.points.push(*q);
                            route.layers.push(route.layer);
                        }
                        let hits = router.collisions(&route.points);
                        self.ui.pcb.collisions =
                            hits.iter().map(|c| (c.at, c.item.clone())).collect();
                        if !hits.is_empty() {
                            self.ui.status = format!(
                                "Collides with {}",
                                hits.iter()
                                    .map(|c| c.item.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                        }
                        let route = self.ui.pcb.route.as_ref().expect("routing");
                        let arrived = target
                            .is_some_and(|(net, _)| net == route.net && route.net != 0)
                            && end != route.points[0];
                        if arrived {
                            self.finish_route();
                        }
                    }
                }
                self.update_route_preview();
                Ok(vec![])
            }
            "via" => {
                if phase != PointerPhase::Up {
                    return Ok(vec![]);
                }
                let net = self
                    .session
                    .board
                    .net_at(at, Layer::FCu)
                    .or_else(|| self.session.board.net_at(at, Layer::BCu))
                    .map(|(n, _)| n)
                    .unwrap_or(0);
                self.pcb_edit();
                let b = &mut self.session.board;
                let id = b.take_id();
                let (diameter, drill) = (b.rules.via_diameter, b.rules.via_drill);
                b.vias.push(Via {
                    id,
                    pos: snapped,
                    diameter,
                    drill,
                    net,
                });
                Ok(vec![])
            }
            "zone" | "keepout" => {
                if phase != PointerPhase::Up {
                    return Ok(vec![]);
                }
                let g = at.snap(self.ui.pcb.grid.max(1));
                let first = self.ui.pcb.poly.first().copied();
                if self.ui.pcb.poly.len() >= 3
                    && first.is_some_and(|f| f.dist(g) <= self.tol() as f64)
                {
                    self.close_zone();
                } else if self.ui.pcb.poly.last() != Some(&g) {
                    self.ui.pcb.poly.push(g);
                    self.ui.status = "Click to add zone corners; click the first corner or double-click to close".into();
                }
                Ok(vec![])
            }
            "rect" | "line" => {
                if phase != PointerPhase::Up {
                    return Ok(vec![]);
                }
                let g = at.snap(self.ui.pcb.grid.max(1));
                let Some(start) = self.ui.pcb.poly.first().copied() else {
                    self.ui.pcb.poly = vec![g];
                    return Ok(vec![]);
                };
                if g == start {
                    return Ok(vec![]);
                }
                let rect = self.pcb_tool() == "rect";
                self.pcb_edit();
                let b = &mut self.session.board;
                let id = b.take_id();
                b.drawings.push(Drawing {
                    id,
                    layer,
                    shape: if rect {
                        DrawShape::Rect { a: start, b: g }
                    } else {
                        DrawShape::Line { a: start, b: g }
                    },
                    width: Self::drawing_width(layer),
                });
                // Lines chain on from where the last one ended; a rectangle is done.
                self.ui.pcb.poly = if rect { vec![] } else { vec![g] };
                self.ui.status = format!(
                    "{} added on {}",
                    if rect { "Rectangle" } else { "Line" },
                    layer.name()
                );
                Ok(vec![])
            }
            other => Err(format!("unknown PCB tool {other}")),
        }
    }
    pub(super) fn pcb_activate(&mut self, _window: u64) -> Result<Vec<AppEffect>, String> {
        match self.pcb_tool() {
            "route" => self.finish_route(),
            "zone" | "keepout" => self.close_zone(),
            "line" => self.ui.pcb.poly.clear(),
            "select" => {
                // Double-clicking a footprint opens its properties, as in KiCad.
                if let [BoardItem::Footprint(id)] = self.board_selected().as_slice() {
                    return self.open_fp_properties(*id);
                }
            }
            _ => {}
        }
        Ok(vec![])
    }
    fn open_fp_properties(&mut self, id: u64) -> Result<Vec<AppEffect>, String> {
        let f = self
            .session
            .board
            .footprint(id)
            .ok_or("footprint not found")?;
        self.ui.dialog = Some(Dialog::FootprintProperties {
            id,
            back: f.back,
            locked: f.locked,
            fields: vec![
                ("Position X".into(), mm_text(f.pos.x)),
                ("Position Y".into(), mm_text(f.pos.y)),
                ("Orientation".into(), angle_text(f.angle)),
            ],
            error: String::new(),
        });
        self.ui.focus = Some("Orientation".into());
        Ok(vec![])
    }
    /// Turn the selected footprints by `delta` tenths of a degree counter-clockwise.
    fn rotate_selection(&mut self, delta: i32) -> Result<Vec<AppEffect>, String> {
        let ids: Vec<u64> = self
            .board_selected()
            .iter()
            .filter_map(|i| match i {
                BoardItem::Footprint(id) => Some(*id),
                _ => None,
            })
            .collect();
        if ids.is_empty() {
            return Err("select a footprint first".into());
        }
        self.pcb_edit();
        for id in &ids {
            let f = self
                .session
                .board
                .footprint_mut(*id)
                .expect("selected footprint");
            f.angle = (f.angle + delta).rem_euclid(3600);
        }
        let shown = self
            .session
            .board
            .footprint(ids[0])
            .map(|f| angle_text(f.angle))
            .unwrap_or_default();
        self.ui.status = format!("Rotated to {shown}°");
        Ok(vec![])
    }
    fn describe_board_selection(&self) -> String {
        let b = &self.session.board;
        match self.board_selected().as_slice() {
            [] => String::new(),
            [BoardItem::Footprint(id)] => b
                .footprint(*id)
                .map(|f| {
                    format!(
                        "{} {} ({}) at {} mm, {} mm",
                        f.reference,
                        f.value,
                        f.fp_id,
                        mm_text(f.pos.x),
                        mm_text(f.pos.y)
                    )
                })
                .unwrap_or_default(),
            [BoardItem::Track(id)] => b
                .tracks
                .iter()
                .find(|t| t.id == *id)
                .map(|t| {
                    format!(
                        "Track [{}] on {}, width {} mm",
                        b.net_name(t.net),
                        t.layer.name(),
                        mm_text(t.width)
                    )
                })
                .unwrap_or_default(),
            items => format!("{} items selected", items.len()),
        }
    }
    fn set_pcb_tool(&mut self, tool: &str) {
        self.ui.pcb.tool = tool.into();
        self.ui.pcb.route = None;
        self.ui.pcb.poly.clear();
        self.ui.drag = None;
        if tool != "select" {
            self.ui.pcb.selection.clear();
        }
    }
    fn run_drc(&mut self) {
        // Zones are refilled first, as KiCad's "Refill all zones before performing DRC".
        if !self.session.board.zones.is_empty() {
            zones::fill_all(&mut self.session.board);
        }
        let refs: Vec<(String, String)> = self
            .session
            .schematic
            .symbols
            .iter()
            .filter(|s| s.on_board && !s.is_power() && s.annotated())
            .map(|s| (s.reference().to_owned(), s.field("Footprint").to_owned()))
            .collect();
        let all = drc::check(&self.session.board, Some(&refs));
        let (parity, violations): (Vec<_>, Vec<_>) = all
            .into_iter()
            .partition(|v| v.rule == "missing_footprint" || v.rule == "extra_footprint");
        let unconnected = drc::unconnected(&self.session.board);
        self.ui.status = format!(
            "DRC: {} violations, {} unconnected items, {} schematic parity issues",
            violations.len(),
            unconnected.len(),
            parity.len()
        );
        self.session.drc = Some(DrcReport {
            violations,
            unconnected,
            parity,
        });
    }
    fn pcb_zoom(&mut self, dir: &str, args: &[&str]) -> Result<Vec<AppEffect>, String> {
        let (cw, ch) = match (
            args.get(3).and_then(|v| v.parse().ok()),
            args.get(4).and_then(|v| v.parse().ok()),
        ) {
            (Some(w), Some(h)) => (w, h),
            _ if self.ui.canvas.0 > 0 => self.ui.canvas,
            _ => (800, 600),
        };
        self.ui.canvas = (cw, ch);
        let mut v = View::from_args(args).unwrap_or_else(|| self.pcb_view(cw, ch));
        // Toolbar zooms keep the canvas centre; keys zoom about the pointer.
        let centre = match (args.len() >= 3, self.ui.hover) {
            (false, Some(h)) => um(h),
            _ => v.world(cw as i32 / 2, ch as i32 / 2),
        };
        match dir {
            "fit" => {
                self.ui.view.fit = true;
                return Ok(vec![]);
            }
            "in" => v.zoom_about(centre, 3, 2, MIN_ZOOM, MAX_ZOOM),
            "out" => v.zoom_about(centre, 2, 3, MIN_ZOOM, MAX_ZOOM),
            other => return Err(format!("unknown zoom {other}")),
        }
        self.ui.view = v;
        Ok(vec![])
    }
    pub(super) fn pcb_command(
        &mut self,
        window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let parts: Vec<&str> = rest.split(':').collect();
        match parts[0] {
            "save" => self.save_board(window),
            "undo" => {
                if !self.session.pcb_history.undo(&mut self.session.board) {
                    return Err("nothing to undo".into());
                }
                self.session.pcb_dirty = true;
                Ok(vec![])
            }
            "redo" => {
                if !self.session.pcb_history.redo(&mut self.session.board) {
                    return Err("nothing to redo".into());
                }
                self.session.pcb_dirty = true;
                Ok(vec![])
            }
            "zoom" => self.pcb_zoom(
                parts.get(1).copied().unwrap_or(""),
                &parts[2.min(parts.len())..],
            ),
            "rotate" => self.rotate_selection(900),
            "rotate-cw" => self.rotate_selection(-900),
            "rotate45" => self.rotate_selection(450),
            "properties" => match self.board_selected().as_slice() {
                [BoardItem::Footprint(id)] => self.open_fp_properties(*id),
                _ => Err("select one footprint to edit its properties".into()),
            },
            "flip" => {
                let ids: Vec<u64> = self
                    .board_selected()
                    .iter()
                    .filter_map(|i| match i {
                        BoardItem::Footprint(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
                if ids.is_empty() {
                    return Err("select a footprint first".into());
                }
                self.pcb_edit();
                for id in ids {
                    let f = self
                        .session
                        .board
                        .footprint_mut(id)
                        .expect("selected footprint");
                    f.back = !f.back;
                }
                Ok(vec![])
            }
            "router" => {
                self.ui.dialog = Some(Dialog::RouterSettings {
                    walkaround: !self.ui.pcb.highlight_collisions,
                });
                Ok(vec![])
            }
            "3d" => self.launch_frame(window, "3d"),
            "fped" => self.launch_frame(window, "fped"),
            "delete" => {
                let sel = self.board_selected();
                if sel.is_empty() {
                    return Err("nothing is selected".into());
                }
                self.pcb_edit();
                let n = self.session.board.delete(&sel);
                self.ui.pcb.selection.clear();
                self.ui.status = format!("Deleted {n} item(s)");
                Ok(vec![])
            }
            "tool" => {
                let tool = parts.get(1).copied().unwrap_or("select");
                if !matches!(
                    tool,
                    "select" | "route" | "via" | "zone" | "keepout" | "rect" | "line"
                ) {
                    return Err(format!("unknown tool {tool}"));
                }
                self.set_pcb_tool(tool);
                Ok(vec![])
            }
            "layer" => {
                let layer =
                    Layer::parse(parts.get(1).copied().unwrap_or("")).ok_or("unknown layer")?;
                self.ui.pcb.layer = layer;
                self.ui.pcb.hidden.retain(|l| *l != layer);
                if let Some(route) = &self.ui.pcb.route {
                    if layer.is_copper() && layer != route.layer {
                        // Changing layer mid-route drops a via where the route stands;
                        // the walkaround router will not put one into a clearance.
                        let at = *route.points.last().expect("a route has a start");
                        if !self.ui.pcb.highlight_collisions {
                            let d = self.session.board.rules.via_diameter;
                            if let Some(hit) = via_collision(&self.session.board, route.net, at, d)
                            {
                                return Err(format!("A via here would collide with {hit}"));
                            }
                        }
                        let route = self.ui.pcb.route.as_mut().expect("routing");
                        route.vias.push(at);
                        route.layer = layer;
                    }
                }
                self.update_route_preview();
                Ok(vec![])
            }
            "eye" => {
                let layer =
                    Layer::parse(parts.get(1).copied().unwrap_or("")).ok_or("unknown layer")?;
                if let Some(i) = self.ui.pcb.hidden.iter().position(|l| *l == layer) {
                    self.ui.pcb.hidden.remove(i);
                } else {
                    self.ui.pcb.hidden.push(layer);
                }
                Ok(vec![])
            }
            "ratsnest" => {
                self.ui.pcb.hide_ratsnest = !self.ui.pcb.hide_ratsnest;
                Ok(vec![])
            }
            "width" => {
                let i = WIDTHS
                    .iter()
                    .position(|w| *w == self.ui.pcb.track_width)
                    .map(|i| (i + 1) % WIDTHS.len())
                    .unwrap_or(0);
                self.ui.pcb.track_width = WIDTHS[i];
                if let Some(r) = &mut self.ui.pcb.route {
                    r.width = WIDTHS[i];
                }
                Ok(vec![])
            }
            "grid" => {
                self.ui.pcb.grid = match self.ui.pcb.grid {
                    250_000 => 500_000,
                    500_000 => 1_270_000,
                    1_270_000 => 100_000,
                    _ => 250_000,
                };
                Ok(vec![])
            }
            "posture" => {
                self.ui.pcb.diagonal_first = !self.ui.pcb.diagonal_first;
                Ok(vec![])
            }
            "update" => {
                self.open_update_dialog();
                Ok(vec![])
            }
            "drc" => {
                self.ui.dialog = Some(Dialog::Drc {
                    tab: 0,
                    selected: None,
                });
                Ok(vec![])
            }
            "fill" => {
                if self.session.board.zones.is_empty() {
                    return Err("the board has no zones to fill".into());
                }
                self.pcb_edit();
                zones::fill_all(&mut self.session.board);
                self.ui.status = format!("Filled {} zone(s)", self.session.board.zones.len());
                Ok(vec![])
            }
            "unfill" => {
                if self.session.board.zones.is_empty() {
                    return Err("the board has no zones".into());
                }
                self.pcb_edit();
                for z in &mut self.session.board.zones {
                    z.fill.clear();
                    z.filled = false;
                }
                Ok(vec![])
            }
            "plot" => {
                self.ui.dialog = Some(Dialog::Plot {
                    svg: false,
                    dir: "gerbers/".into(),
                    layers: gerber::DEFAULT_LAYERS.to_vec(),
                    log: vec![],
                });
                Ok(vec![])
            }
            "drill" => {
                self.ui.dialog = Some(Dialog::Drill { log: vec![] });
                Ok(vec![])
            }
            "setup" => {
                let r = &self.session.board.rules;
                self.ui.dialog = Some(Dialog::BoardSetup {
                    fields: vec![
                        ("Minimum clearance".into(), mm_text(r.clearance)),
                        ("Default track width".into(), mm_text(r.track_width)),
                        ("Minimum track width".into(), mm_text(r.min_track_width)),
                        ("Via diameter".into(), mm_text(r.via_diameter)),
                        ("Via hole".into(), mm_text(r.via_drill)),
                        ("Minimum annular width".into(), mm_text(r.min_annular_ring)),
                        (
                            "Copper to edge clearance".into(),
                            mm_text(r.copper_edge_clearance),
                        ),
                        ("Minimum hole to hole".into(), mm_text(r.hole_to_hole)),
                    ],
                    error: String::new(),
                });
                Ok(vec![])
            }
            "schematic" => self.launch_frame(window, "sch"),
            other => Err(format!("unknown PCB command {other}")),
        }
    }
    pub(super) fn pcb_key(&mut self, window: u64, key: &str) -> Result<Vec<AppEffect>, String> {
        let command = match key {
            "r" | "R" => "rotate",
            "Shift+R" | "Shift+r" => "rotate-cw",
            "Ctrl+r" | "Meta+r" => "rotate45",
            "e" | "E" => "properties",
            "Alt+3" => "3d",
            "f" | "F" => "flip",
            "Delete" | "Backspace" => "delete",
            "x" | "X" => "tool:route",
            "Ctrl+z" | "Meta+z" => "undo",
            "Ctrl+y" | "Ctrl+Shift+z" | "Meta+Shift+z" => "redo",
            "b" | "B" => "fill",
            "Ctrl+b" | "Meta+b" => "unfill",
            "w" | "W" => "width",
            "/" => "posture",
            "Home" => "zoom:fit",
            "F1" | "+" | "=" => "zoom:in",
            "F2" | "-" => "zoom:out",
            "F8" => "update",
            "PageUp" => "layer:F.Cu",
            "PageDown" => "layer:B.Cu",
            "v" | "V" => {
                let route = self
                    .ui
                    .pcb
                    .route
                    .as_ref()
                    .ok_or("V places a via while routing")?;
                let other = if route.layer == Layer::FCu {
                    "layer:B.Cu"
                } else {
                    "layer:F.Cu"
                };
                return self.pcb_command(window, other);
            }
            "End" | "Enter" if self.ui.pcb.route.is_some() => {
                self.finish_route();
                return Ok(vec![]);
            }
            "Escape" => {
                if self.ui.pcb.route.is_some() || !self.ui.pcb.poly.is_empty() {
                    self.ui.pcb.route = None;
                    self.ui.pcb.poly.clear();
                    self.ui.pcb.preview.clear();
                    self.ui.pcb.collisions.clear();
                } else if self.pcb_tool() != "select" {
                    self.set_pcb_tool("select");
                } else {
                    self.ui.pcb.selection.clear();
                }
                return Ok(vec![]);
            }
            other => return Err(format!("unsupported PCB key {other}")),
        };
        self.pcb_command(window, command)
    }

    pub(super) fn pcb_dialog(&mut self, window: u64, rest: &str) -> Result<Vec<AppEffect>, String> {
        let dialog = self.ui.dialog.clone().ok_or("no dialog is open")?;
        let (cmd, arg) = rest.split_once(':').unwrap_or((rest, ""));
        match dialog {
            Dialog::UpdatePcb { changes, applied } => match cmd {
                "ok" | "apply" if !applied => {
                    self.pcb_edit();
                    let libs = self.session.project_footprints();
                    let done = self.session.board.update_from_schematic_with(
                        &self.session.schematic,
                        true,
                        &libs,
                    );
                    self.ui.view.fit = true;
                    self.ui.status = format!("Update PCB: {} change(s) applied", done.len());
                    self.ui.dialog = Some(Dialog::UpdatePcb {
                        changes: done,
                        applied: true,
                    });
                    Ok(vec![])
                }
                "ok" | "apply" => {
                    let _ = changes;
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown update command {other}")),
            },
            Dialog::Drc { tab, .. } => match cmd {
                "run" => {
                    self.run_drc();
                    self.ui.dialog = Some(Dialog::Drc {
                        tab,
                        selected: None,
                    });
                    Ok(vec![])
                }
                "tab" => {
                    let t: u8 = arg.parse().map_err(|_| "bad tab")?;
                    self.ui.dialog = Some(Dialog::Drc {
                        tab: t.min(2),
                        selected: None,
                    });
                    Ok(vec![])
                }
                "select" => {
                    let i: usize = arg.parse().map_err(|_| "bad row")?;
                    let report = self.session.drc.as_ref().ok_or("DRC has not been run")?;
                    let list = match tab {
                        0 => &report.violations,
                        1 => &report.unconnected,
                        _ => &report.parity,
                    };
                    let pos = list.get(i).ok_or("row not found")?.pos;
                    let (cw, ch) = if self.ui.canvas.0 > 0 {
                        self.ui.canvas
                    } else {
                        (800, 600)
                    };
                    let mut view = self.pcb_view(cw, ch);
                    let c = um(pos);
                    view.x0 = c.x - (cw as i64 * 1000 / 2 / view.zoom.max(1));
                    view.y0 = c.y - (ch as i64 * 1000 / 2 / view.zoom.max(1));
                    view.fit = false;
                    self.ui.view = view;
                    self.ui.dialog = Some(Dialog::Drc {
                        tab,
                        selected: Some(i),
                    });
                    Ok(vec![])
                }
                "ok" => {
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown DRC command {other}")),
            },
            Dialog::Plot {
                svg,
                dir,
                mut layers,
                mut log,
            } => match cmd {
                "fmt" => {
                    self.ui.dialog = Some(Dialog::Plot {
                        svg: arg == "svg",
                        dir,
                        layers,
                        log,
                    });
                    Ok(vec![])
                }
                "layer" => {
                    let l = Layer::parse(arg).ok_or("unknown layer")?;
                    if let Some(i) = layers.iter().position(|x| *x == l) {
                        layers.remove(i);
                    } else {
                        layers.push(l);
                    }
                    self.ui.dialog = Some(Dialog::Plot {
                        svg,
                        dir,
                        layers,
                        log,
                    });
                    Ok(vec![])
                }
                "drill" => {
                    self.ui.dialog = Some(Dialog::Drill { log: vec![] });
                    Ok(vec![])
                }
                "ok" | "plot" => {
                    let project = self.session.project.clone().ok_or("No project is open")?;
                    if layers.is_empty() {
                        return Err("choose at least one layer to plot".into());
                    }
                    let rel = dir.trim().trim_matches('/');
                    if rel.contains("..") {
                        return Err("the output directory must stay inside the project".into());
                    }
                    let out = if rel.is_empty() {
                        project.dir.clone()
                    } else {
                        format!("{}/{rel}", project.dir)
                    };
                    let mut effects = vec![AppEffect::CreateDirectory {
                        window,
                        path: out.clone(),
                    }];
                    let mut ordered = layers.clone();
                    ordered.sort();
                    for l in ordered {
                        let (path, content) = if svg {
                            (
                                format!(
                                    "{out}/{}-{}.svg",
                                    project.name,
                                    l.name().replace('.', "_")
                                ),
                                cw_eda::svg::plot(&self.session.board, &[l], &project.name),
                            )
                        } else {
                            (
                                format!("{out}/{}", gerber::file_name(&project.name, l)),
                                gerber::layer(&self.session.board, &project.name, l),
                            )
                        };
                        log.push(format!("Plot: '{path}'."));
                        effects.push(AppEffect::WriteFile {
                            window,
                            path,
                            content,
                        });
                    }
                    log.push("Done.".into());
                    self.ui.status = format!("Plotted {} layer(s) to {out}", layers.len());
                    self.ui.dialog = Some(Dialog::Plot {
                        svg,
                        dir,
                        layers,
                        log,
                    });
                    effects.push(AppEffect::ListDirectory {
                        window,
                        tab: 0,
                        path: project.dir,
                    });
                    Ok(effects)
                }
                other => Err(format!("unknown plot command {other}")),
            },
            Dialog::Drill { mut log } => match cmd {
                "ok" | "generate" => {
                    let project = self.session.project.clone().ok_or("No project is open")?;
                    let out = format!("{}/gerbers", project.dir);
                    let path = format!("{out}/{}.drl", project.name);
                    log.push(format!("Creating drill file '{path}'."));
                    log.push("Done.".into());
                    self.ui.status = format!("Drill file written to {path}");
                    self.ui.dialog = Some(Dialog::Drill { log });
                    Ok(vec![
                        AppEffect::CreateDirectory { window, path: out },
                        AppEffect::WriteFile {
                            window,
                            path,
                            content: gerber::drill(&self.session.board, &project.name),
                        },
                        AppEffect::ListDirectory {
                            window,
                            tab: 0,
                            path: project.dir,
                        },
                    ])
                }
                other => Err(format!("unknown drill command {other}")),
            },
            Dialog::BoardSetup { fields, .. } => match cmd {
                "ok" => {
                    let mut values = vec![];
                    for (k, v) in &fields {
                        match parse_mm(v.trim(), 1_000_000).filter(|x| *x > 0) {
                            Some(x) => values.push(x),
                            None => {
                                self.ui.dialog = Some(Dialog::BoardSetup {
                                    error: format!("{k}: '{v}' is not a positive length in mm"),
                                    fields,
                                });
                                return Ok(vec![]);
                            }
                        }
                    }
                    if values[3] <= values[4] {
                        self.ui.dialog = Some(Dialog::BoardSetup {
                            error: "The via diameter must be larger than its hole.".into(),
                            fields,
                        });
                        return Ok(vec![]);
                    }
                    self.pcb_edit();
                    let r = &mut self.session.board.rules;
                    r.clearance = values[0];
                    r.track_width = values[1];
                    r.min_track_width = values[2];
                    r.via_diameter = values[3];
                    r.via_drill = values[4];
                    r.min_annular_ring = values[5];
                    r.copper_edge_clearance = values[6];
                    r.hole_to_hole = values[7];
                    self.ui.pcb.track_width = values[1];
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown board setup command {other}")),
            },
            Dialog::ZoneProperties {
                outline,
                net,
                layer,
                clearance,
                keepout,
                ..
            } => match cmd {
                "net" => {
                    let n: usize = arg.parse().map_err(|_| "bad net")?;
                    if n >= self.session.board.nets.len() {
                        return Err("net not found".into());
                    }
                    self.ui.dialog = Some(Dialog::ZoneProperties {
                        outline,
                        net: n,
                        layer,
                        clearance,
                        keepout,
                        error: String::new(),
                    });
                    Ok(vec![])
                }
                "layer" => {
                    let l = Layer::parse(arg)
                        .filter(|l| l.is_copper())
                        .ok_or("zones go on a copper layer")?;
                    self.ui.dialog = Some(Dialog::ZoneProperties {
                        outline,
                        net,
                        layer: l,
                        clearance,
                        keepout,
                        error: String::new(),
                    });
                    Ok(vec![])
                }
                "keepout" => {
                    self.ui.dialog = Some(Dialog::ZoneProperties {
                        outline,
                        net,
                        layer,
                        clearance,
                        keepout: !keepout,
                        error: String::new(),
                    });
                    Ok(vec![])
                }
                "ok" => {
                    let Some(c) = parse_mm(clearance.trim(), 1_000_000).filter(|c| *c >= 0) else {
                        self.ui.dialog = Some(Dialog::ZoneProperties {
                            error: format!("'{clearance}' is not a clearance in mm"),
                            outline,
                            net,
                            layer,
                            clearance,
                            keepout,
                        });
                        return Ok(vec![]);
                    };
                    self.pcb_edit();
                    let b = &mut self.session.board;
                    let id = b.take_id();
                    b.zones.push(Zone {
                        id,
                        net: if keepout { 0 } else { net },
                        layer,
                        outline,
                        clearance: c,
                        min_width: 250_000,
                        thermal_gap: 500_000,
                        spoke_width: 500_000,
                        fill: vec![],
                        filled: false,
                        keepout,
                    });
                    zones::fill_all(b);
                    self.ui.status = if keepout {
                        format!("Rule area (keepout) added on {}", layer.name())
                    } else {
                        format!("Zone added on {} and filled", layer.name())
                    };
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown zone command {other}")),
            },
            Dialog::FootprintProperties {
                id,
                mut back,
                mut locked,
                fields,
                ..
            } => match cmd {
                "side" | "lock" => {
                    if cmd == "side" {
                        back = !back;
                    } else {
                        locked = !locked;
                    }
                    self.ui.dialog = Some(Dialog::FootprintProperties {
                        id,
                        back,
                        locked,
                        fields,
                        error: String::new(),
                    });
                    Ok(vec![])
                }
                "ok" => {
                    let d = self.ui.dialog.clone().expect("open");
                    let x = parse_mm(&d.value("Position X"), 1_000_000);
                    let y = parse_mm(&d.value("Position Y"), 1_000_000);
                    let angle = parse_angle(d.value("Orientation").trim_end_matches('°'));
                    let (Some(x), Some(y), Some(angle)) = (x, y, angle) else {
                        self.ui.dialog = Some(Dialog::FootprintProperties {
                            id,
                            back,
                            locked,
                            fields,
                            error: "Position is in mm and orientation in degrees (any angle, e.g. 45 or 22.5)."
                                .into(),
                        });
                        return Ok(vec![]);
                    };
                    self.pcb_edit();
                    let f = self
                        .session
                        .board
                        .footprint_mut(id)
                        .ok_or("footprint not found")?;
                    f.pos = Pt::new(x, y);
                    f.angle = angle;
                    f.back = back;
                    f.locked = locked;
                    self.ui.status = format!(
                        "{} at {} mm, {} mm, {}°",
                        f.reference,
                        mm_text(x),
                        mm_text(y),
                        angle_text(angle)
                    );
                    self.close_dialog();
                    Ok(vec![])
                }
                other => Err(format!("unknown footprint command {other}")),
            },
            Dialog::RouterSettings { walkaround } => match cmd {
                "mode" => {
                    self.ui.dialog = Some(Dialog::RouterSettings {
                        walkaround: arg == "walkaround",
                    });
                    Ok(vec![])
                }
                "ok" => {
                    self.ui.pcb.highlight_collisions = !walkaround;
                    self.ui.status = format!(
                        "Router mode: {}",
                        if walkaround {
                            "walk around"
                        } else {
                            "highlight collisions"
                        }
                    );
                    self.close_dialog();
                    self.update_route_preview();
                    Ok(vec![])
                }
                other => Err(format!("unknown router settings command {other}")),
            },
            _ => Err("not a PCB dialog".into()),
        }
    }

    // ---- painting ---------------------------------------------------------------------
    pub(super) fn render_pcb(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (w, h) = (env.width, env.height);
        let c = chrome(env.theme);
        p.scene.background = c.bar;
        let area = canvas(w, h);
        let view = self.pcb_view(area.width, area.height);
        let cv = Cv {
            ox: area.x,
            oy: area.y,
            w: area.width,
            h: area.height,
            view,
        };
        self.paint_board(p, &cv);
        p.region(
            area,
            &format!(
                "kicad:canvas:pcb:{}:{}:{}",
                view.args(),
                area.width,
                area.height
            ),
            "Board canvas",
        );
        let menus = pcb_menus(self);
        let titles: Vec<&str> = menus.iter().map(|m| m.0).collect();
        let top = w::MENU_H as i32;
        p.box_(Rect::new(0, top, w, w::TOOL_H), c.bar, 0);
        p.hline(0, top + w::TOOL_H as i32 - 1, w, w::EDGE);
        let zoom_args = format!("{}:{}:{}", view.args(), area.width, area.height);
        let fp_sel = self
            .board_selected()
            .iter()
            .any(|i| matches!(i, BoardItem::Footprint(_)));
        let need_fp = |t: &str| -> Result<String, &'static str> {
            if fp_sel {
                Ok(t.to_owned())
            } else {
                Err("select a footprint first")
            }
        };
        let proj = |t: &str| -> Result<String, &'static str> {
            if self.session.project.is_some() {
                Ok(t.to_owned())
            } else {
                Err("no project is open")
            }
        };
        let zones_present = |t: &str| -> Result<String, &'static str> {
            if self.session.board.zones.is_empty() {
                Err("the board has no zones")
            } else {
                Ok(t.to_owned())
            }
        };
        let undo = if self.session.pcb_history.past.is_empty() {
            Err("nothing to undo")
        } else {
            Ok("kicad:pcb:undo".to_owned())
        };
        let redo = if self.session.pcb_history.future.is_empty() {
            Err("nothing to redo")
        } else {
            Ok("kicad:pcb:redo".to_owned())
        };
        let groups: Vec<Vec<w::ToolSpec>> = vec![
            vec![
                (icons::save, proj("kicad:pcb:save"), "Save (Ctrl+S)"),
                (icons::settings, Ok("kicad:pcb:setup".into()), "Board setup"),
                (icons::plot, proj("kicad:pcb:plot"), "Plot (Gerber, SVG)"),
            ],
            vec![
                (icons::undo, undo, "Undo (Ctrl+Z)"),
                (icons::redo, redo, "Redo (Ctrl+Y)"),
            ],
            vec![
                (
                    icons::zoom_in,
                    Ok(format!("kicad:pcb:zoom:in:{zoom_args}")),
                    "Zoom in (F1)",
                ),
                (
                    icons::zoom_out,
                    Ok(format!("kicad:pcb:zoom:out:{zoom_args}")),
                    "Zoom out (F2)",
                ),
                (
                    icons::zoom_fit,
                    Ok(format!("kicad:pcb:zoom:fit:{zoom_args}")),
                    "Zoom to fit (Home)",
                ),
            ],
            vec![
                (
                    icons::rotate,
                    need_fp("kicad:pcb:rotate"),
                    "Rotate counterclockwise (R)",
                ),
                (
                    icons::rotate,
                    need_fp("kicad:pcb:rotate45"),
                    "Rotate 45° counterclockwise (Ctrl+R)",
                ),
                (
                    icons::properties,
                    if matches!(self.board_selected().as_slice(), [BoardItem::Footprint(_)]) {
                        Ok("kicad:pcb:properties".into())
                    } else {
                        Err("select one footprint first")
                    },
                    "Footprint properties (E)",
                ),
                (
                    icons::flip,
                    need_fp("kicad:pcb:flip"),
                    "Change side / flip (F)",
                ),
                (
                    icons::delete,
                    if self.board_selected().is_empty() {
                        Err("nothing is selected")
                    } else {
                        Ok("kicad:pcb:delete".into())
                    },
                    "Delete (Del)",
                ),
            ],
            vec![
                (
                    icons::update,
                    Ok("kicad:pcb:update".into()),
                    "Update PCB with changes made to schematic (F8)",
                ),
                (
                    icons::schematic,
                    proj("kicad:pcb:schematic"),
                    "Switch to Schematic Editor",
                ),
                (
                    icons::drc,
                    Ok("kicad:pcb:drc".into()),
                    "Design Rules Checker",
                ),
                (
                    icons::fill,
                    zones_present("kicad:pcb:fill"),
                    "Fill all zones (B)",
                ),
            ],
            vec![
                (
                    icons::settings,
                    Ok("kicad:pcb:router".into()),
                    "Interactive router settings",
                ),
                (icons::viewer3d, proj("kicad:pcb:3d"), "3D Viewer (Alt+3)"),
                (
                    icons::footprint_editor,
                    proj("kicad:pcb:fped"),
                    "Footprint Editor",
                ),
            ],
        ];
        let mut x = 6;
        let ty = top + 3;
        for (gi, group) in groups.into_iter().enumerate() {
            if gi > 0 {
                w::separator_v(p, x + 2, ty);
                x += 8;
            }
            for (icon, target, tip) in group {
                w::tool(p, &c, x, ty, icon, target, tip, false);
                x += 30;
            }
        }
        // Active layer and track width selectors.
        let lr = Rect::new(x + 10, ty + 2, 110, 24);
        let other = if self.ui.pcb.layer == Layer::FCu {
            "B.Cu"
        } else {
            "F.Cu"
        };
        p.button(
            lr,
            w::WHITE,
            c.radius,
            &format!("kicad:pcb:layer:{other}"),
            "Active layer",
        );
        p.border(lr, Color::TRANSPARENT, c.radius, w::EDGE);
        p.box_(
            Rect::new(lr.x + 6, lr.y + 6, 12, 12),
            layer_color(self.ui.pcb.layer),
            2,
        );
        p.label(
            lr.x + 24,
            lr.y + 3,
            80,
            self.ui.pcb.layer.name(),
            13,
            w::INK,
            false,
            Align::Left,
        );
        let wr = Rect::new(lr.x + 118, ty + 2, 150, 24);
        p.button(wr, w::WHITE, c.radius, "kicad:pcb:width", "Track width");
        p.border(wr, Color::TRANSPARENT, c.radius, w::EDGE);
        p.label(
            wr.x + 8,
            wr.y + 3,
            140,
            &format!("Track: {} mm", mm_text(self.ui.pcb.track_width)),
            13,
            w::INK,
            false,
            Align::Left,
        );
        // Left toolbar.
        p.box_(Rect::new(0, area.y, w::SIDE_W, area.height), c.bar, 0);
        p.vline(w::SIDE_W as i32, area.y, area.height, w::EDGE);
        let mut y = area.y + 4;
        for (icon, target, tip, on) in [
            (
                icons::ratsnest as w::Icon,
                "kicad:pcb:ratsnest",
                "Show ratsnest",
                !self.ui.pcb.hide_ratsnest,
            ),
            (icons::grid, "kicad:pcb:grid", "Change grid", false),
            (
                icons::posture,
                "kicad:pcb:posture",
                "Route posture: 45° first",
                self.ui.pcb.diagonal_first,
            ),
        ] {
            w::tool(p, &c, 3, y, icon, Ok(target.into()), tip, on);
            y += 32;
        }
        // Right toolbar.
        let rx = area.x + area.width as i32;
        p.box_(Rect::new(rx, area.y, w::SIDE_W, area.height), c.bar, 0);
        p.vline(rx, area.y, area.height, w::EDGE);
        let mut y = area.y + 4;
        for (icon, tool, tip) in [
            (icons::select as w::Icon, "select", "Select item(s)"),
            (icons::route, "route", "Route tracks (X)"),
            (icons::via, "via", "Add a free-standing via"),
            (icons::zone, "zone", "Add a filled zone"),
            (icons::keepout, "keepout", "Add a rule area (keepout)"),
            (icons::line, "line", "Draw a line"),
            (icons::rect, "rect", "Draw a rectangle"),
        ] {
            w::tool(
                p,
                &c,
                rx + 3,
                y,
                icon,
                Ok(format!("kicad:pcb:tool:{tool}")),
                tip,
                self.pcb_tool() == tool,
            );
            y += 32;
        }
        // Appearance panel.
        let px = rx + w::SIDE_W as i32 + 1;
        p.box_(Rect::new(px, area.y, PANEL_W, area.height), c.panel, 0);
        p.vline(px - 1, area.y, area.height, w::EDGE);
        p.label(
            px + 8,
            area.y + 6,
            PANEL_W - 16,
            "Appearance ▸ Layers",
            12,
            w::MUTED,
            true,
            Align::Left,
        );
        let mut y = area.y + 28;
        for l in Layer::ALL {
            let r = Rect::new(px + 4, y, PANEL_W - 8, 22);
            let active = self.ui.pcb.layer == l;
            p.button(
                r,
                if active {
                    c.selection
                } else {
                    Color::TRANSPARENT
                },
                2,
                &format!("kicad:pcb:layer:{}", l.name()),
                l.user_name(),
            );
            let eye = Rect::new(r.x + 2, y + 2, 18, 18);
            let shown = self.visible(l);
            p.button(
                eye,
                Color::TRANSPARENT,
                3,
                &format!("kicad:pcb:eye:{}", l.name()),
                &format!("Show {}", l.user_name()),
            );
            if let Some(n) = p.scene.nodes.last_mut() {
                n.state = Some(cw_scene::NodeState {
                    checked: Some(shown),
                    ..Default::default()
                });
            }
            p.z += 1;
            if shown {
                p.ring(eye.x + 9, y + 11, 5, 1, w::INK);
                p.circle(eye.x + 9, y + 11, 2, w::INK);
            } else {
                p.line(vec![(eye.x + 3, y + 11), (eye.x + 15, y + 11)], w::FAINT, 1);
            }
            p.box_(Rect::new(r.x + 26, y + 5, 12, 12), layer_color(l), 2);
            p.label(
                r.x + 44,
                y + 2,
                PANEL_W - 56,
                l.user_name(),
                12,
                w::INK,
                active,
                Align::Left,
            );
            p.z -= 1;
            y += 23;
        }
        let rats = cw_eda::drc::ratsnest(&self.session.board).len();
        p.label(
            px + 8,
            y + 8,
            PANEL_W - 16,
            &format!("Unrouted: {rats}"),
            12,
            w::MUTED,
            false,
            Align::Left,
        );
        // Status bar.
        let hv = self.ui.hover.unwrap_or_default();
        w::status_bar(
            p,
            &c,
            w,
            h,
            &[
                format!("Z {:.2}", view.zoom as f64 / 10.0),
                format!("X {} Y {}", mm_text(hv.x), mm_text(hv.y)),
                format!("grid {} mm", mm_text(self.ui.pcb.grid)),
                "mm".into(),
                self.ui.pcb.layer.name().into(),
                self.ui.status.clone(),
            ],
        );
        let xs = w::menubar(p, &c, w, &titles, self.ui.menu.as_deref());
        if let Some(open) = &self.ui.menu {
            if let Some(i) = titles.iter().position(|t| t == open) {
                p.z += 40;
                w::menu_panel(p, &c, xs[i], &menus[i].1);
                p.z -= 40;
            }
        }
    }

    fn paint_board(&self, p: &mut Painter, cv: &Cv) {
        let b = &self.session.board;
        p.box_(Rect::new(cv.ox, cv.oy, cv.w, cv.h), PCB_BG, 0);
        let q = |pt: Pt| um(pt);
        // Grid dots at a spacing that stays readable.
        let mut step = self.ui.pcb.grid.max(1) / 1000;
        while cv.len(step) < 10 {
            step *= 2;
        }
        let tl = cv.view.world(0, 0);
        let br = cv.view.world(cv.w as i32, cv.h as i32);
        if (br.x - tl.x) / step * (br.y - tl.y) / step < 4000 {
            let mut gy = tl.y.div_euclid(step) * step;
            while gy <= br.y {
                let mut gx = tl.x.div_euclid(step) * step;
                while gx <= br.x {
                    let (x, y) = cv.pt(Pt::new(gx, gy));
                    p.box_(Rect::new(x, y, 1, 1), Color::rgb(90, 90, 110), 0);
                    gx += step;
                }
                gy += step;
            }
        }
        let sel = self.board_selected();
        let offset = match &self.ui.drag {
            Some(Drag::Move { start, at }) => at.sub(*start),
            _ => Pt::default(),
        };
        let moved = |item: BoardItem| {
            if sel.contains(&item) {
                offset
            } else {
                Pt::default()
            }
        };
        let active = self.ui.pcb.layer;
        for layer in paint_order(active) {
            if !self.visible(layer) {
                continue;
            }
            let col = layer_color(layer);
            let dim = |c: Color| {
                if layer.is_copper() && layer != active {
                    Color(c.0, c.1, c.2, 150)
                } else {
                    c
                }
            };
            if layer.is_copper() {
                // Rule areas: a hatched outline, as KiCad draws keepouts.
                for z in b.zones.iter().filter(|z| z.layer == layer && z.keepout) {
                    let d = moved(BoardItem::Zone(z.id));
                    let mut outline: Vec<Pt> = z.outline.iter().map(|x| q(x.add(d))).collect();
                    if outline.is_empty() {
                        continue;
                    }
                    outline.push(outline[0]);
                    let red = Color(200, 60, 60, 220);
                    cv.stroke(p, &outline, red, 0, 2);
                    let r = cw_eda::pcb::bbox_of(&outline);
                    let mut k = r.min.x - r.height();
                    let step = 1000.max(r.width().max(r.height()) / 12);
                    while k < r.max.x {
                        // Diagonal hatch clipped to the bounding box of the outline.
                        let a = Pt::new(k.max(r.min.x), r.max.y - (k.max(r.min.x) - k));
                        let e = k + r.height();
                        let bpt = Pt::new(e.min(r.max.x), r.min.y + (e - e.min(r.max.x)));
                        if a.x < bpt.x {
                            cv.stroke(p, &[a, bpt], Color(200, 60, 60, 90), 0, 1);
                        }
                        k += step;
                    }
                }
                for z in b.zones.iter().filter(|z| z.layer == layer && !z.keepout) {
                    let d = moved(BoardItem::Zone(z.id));
                    for r in &z.fill {
                        let (x0, y0) = cv.pt(q(r.min.add(d)));
                        let (x1, y1) = cv.pt(q(r.max.add(d)));
                        p.box_(
                            Rect::new(x0, y0, (x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32),
                            dim(Color(col.0, col.1, col.2, 110)),
                            0,
                        );
                    }
                    let mut outline: Vec<Pt> = z.outline.iter().map(|x| q(x.add(d))).collect();
                    outline.push(outline[0]);
                    cv.stroke(p, &outline, dim(col), 0, 1);
                }
                for t in b.tracks.iter().filter(|t| t.layer == layer) {
                    let d = moved(BoardItem::Track(t.id));
                    cv.stroke(
                        p,
                        &[q(t.a.add(d)), q(t.b.add(d))],
                        dim(col),
                        t.width / 1000,
                        1,
                    );
                }
            }
            for f in &b.footprints {
                let d = moved(BoardItem::Footprint(f.id));
                for l in f.lines.iter().filter(|l| f.layer(l.layer) == layer) {
                    cv.stroke(
                        p,
                        &[q(f.to_board(l.a).add(d)), q(f.to_board(l.b).add(d))],
                        col,
                        l.width / 1000,
                        1,
                    );
                }
                if f.layer(Layer::FSilkS) == layer {
                    let bb = f.bounds();
                    let at = Pt::new(bb.center().x, bb.min.y - 1_000_000).add(d);
                    cv.text(p, q(at), &f.reference, 1000, col, Align::Center);
                }
                if f.layer(Layer::FFab) == layer {
                    cv.text(
                        p,
                        q(f.bounds().center().add(d)),
                        &f.value,
                        800,
                        col,
                        Align::Center,
                    );
                }
                let pad_layer = layer.is_copper()
                    || matches!(
                        layer,
                        Layer::FMask | Layer::BMask | Layer::FPaste | Layer::BPaste
                    );
                if !pad_layer {
                    continue;
                }
                for pad in &f.pads {
                    let on = match layer {
                        Layer::FCu | Layer::BCu => f.pad_layers(pad).contains(&layer),
                        Layer::FMask | Layer::BMask => {
                            pad.kind == PadKind::ThroughHole || f.layer(Layer::FMask) == layer
                        }
                        _ => pad.kind == PadKind::Smd && f.layer(Layer::FPaste) == layer,
                    };
                    if !on {
                        continue;
                    }
                    let pad_color = if layer.is_copper() && pad.kind == PadKind::ThroughHole {
                        Color::rgb(227, 183, 46)
                    } else {
                        col
                    };
                    let c = q(f.pad_pos(pad).add(d));
                    let (pw, ph) = f.pad_size(pad);
                    let (px, py) = cv.pt(c);
                    let (sw, sh) = (cv.len(pw / 1000).max(2), cv.len(ph / 1000).max(2));
                    if !f.orthogonal() {
                        // A pad turned to an arbitrary angle: its true outline.
                        match f.pad_shape(pad) {
                            Shape::Seg { a, b: e, r } => cv.stroke(
                                p,
                                &[q(a.add(d)), q(e.add(d))],
                                pad_color,
                                2 * r / 1000,
                                2,
                            ),
                            shape => {
                                if let Some(corners) = shape.corners() {
                                    let pts: Vec<Pt> =
                                        corners.iter().map(|x| q(x.add(d))).collect();
                                    cv.fill(p, &pts, pad_color);
                                }
                            }
                        }
                        if pad.drill > 0 && layer.is_copper() {
                            p.circle(
                                px,
                                py,
                                (cv.len(pad.drill / 1000) / 2).max(1) as u32,
                                Color::rgb(20, 20, 20),
                            );
                        }
                        continue;
                    }
                    match pad.shape {
                        PadShape::Circle => p.circle(px, py, (sw / 2).max(1) as u32, pad_color),
                        PadShape::Oval => p.box_(
                            Rect::new(px - sw / 2, py - sh / 2, sw as u32, sh as u32),
                            pad_color,
                            (sw.min(sh) / 2) as u32,
                        ),
                        PadShape::Rect => p.box_(
                            Rect::new(px - sw / 2, py - sh / 2, sw as u32, sh as u32),
                            pad_color,
                            0,
                        ),
                        PadShape::RoundRect => p.box_(
                            Rect::new(px - sw / 2, py - sh / 2, sw as u32, sh as u32),
                            pad_color,
                            (sw.min(sh) / 4) as u32,
                        ),
                    }
                    if pad.drill > 0 && layer.is_copper() {
                        p.circle(
                            px,
                            py,
                            (cv.len(pad.drill / 1000) / 2).max(1) as u32,
                            Color::rgb(20, 20, 20),
                        );
                    }
                }
            }
            for g in b.drawings.iter().filter(|g| g.layer == layer) {
                let d = moved(BoardItem::Drawing(g.id));
                match g.shape {
                    DrawShape::Line { a, b: e } => {
                        cv.stroke(p, &[q(a.add(d)), q(e.add(d))], col, g.width / 1000, 1)
                    }
                    DrawShape::Rect { a, b: e } => {
                        let pts = [a, Pt::new(e.x, a.y), e, Pt::new(a.x, e.y), a];
                        let pts: Vec<Pt> = pts.iter().map(|x| q(x.add(d))).collect();
                        cv.stroke(p, &pts, col, g.width / 1000, 1);
                    }
                }
            }
        }
        // Vias go over both copper layers.
        if self.visible(Layer::FCu) || self.visible(Layer::BCu) {
            for v in &b.vias {
                let (x, y) = cv.pt(q(v.pos.add(moved(BoardItem::Via(v.id)))));
                p.circle(
                    x,
                    y,
                    (cv.len(v.diameter / 1000) / 2).max(2) as u32,
                    Color::rgb(236, 236, 236),
                );
                p.circle(
                    x,
                    y,
                    (cv.len(v.drill / 1000) / 2).max(1) as u32,
                    Color::rgb(60, 60, 60),
                );
            }
        }
        if !self.ui.pcb.hide_ratsnest {
            for r in drc::ratsnest(b) {
                cv.stroke(p, &[q(r.a), q(r.b)], Color(245, 255, 213, 200), 0, 1);
            }
        }
        // Selection outlines.
        for item in &sel {
            let r = match item {
                BoardItem::Footprint(id) => b.footprint(*id).map(|f| f.bounds()),
                BoardItem::Track(id) => b
                    .tracks
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| WRect::new(t.a, t.b).inflate(t.width / 2)),
                BoardItem::Via(id) => b
                    .vias
                    .iter()
                    .find(|v| v.id == *id)
                    .map(|v| WRect::around(v.pos, v.diameter / 2, v.diameter / 2)),
                BoardItem::Drawing(id) => b.drawings.iter().find(|g| g.id == *id).map(|g| match g
                    .shape
                {
                    DrawShape::Line { a, b } | DrawShape::Rect { a, b } => WRect::new(a, b),
                }),
                BoardItem::Zone(id) => b.zones.iter().find(|z| z.id == *id).map(|z| {
                    let mut r = WRect::new(z.outline[0], z.outline[0]);
                    for p in &z.outline {
                        r = r.union(&WRect::new(*p, *p));
                    }
                    r
                }),
            };
            if let Some(r) = r {
                let r = WRect::new(r.min.add(offset), r.max.add(offset));
                let (x0, y0) = cv.pt(q(r.min));
                let (x1, y1) = cv.pt(q(r.max));
                p.border(
                    Rect::new(
                        x0 - 2,
                        y0 - 2,
                        (x1 - x0 + 4).max(2) as u32,
                        (y1 - y0 + 4).max(2) as u32,
                    ),
                    Color(255, 255, 255, 30),
                    2,
                    Color::rgb(255, 255, 255),
                );
            }
        }
        // DRC markers.
        if let Some(report) = &self.session.drc {
            for v in report.violations.iter().chain(&report.unconnected) {
                let (x, y) = cv.pt(q(v.pos));
                let color = if v.severity == drc::Severity::Error {
                    Color::rgb(255, 40, 40)
                } else {
                    Color::rgb(255, 200, 0)
                };
                p.path(vec![(x, y), (x + 14, y - 5), (x + 5, y - 14)], color);
            }
        }
        // In-progress tools.
        if let Some(hover) = self.ui.hover {
            if let Some(route) = &self.ui.pcb.route {
                let last = *route.points.last().expect("a route has a start");
                let col = layer_color(route.layer);
                let mut pts: Vec<Pt> = route.points.clone();
                // The router's own path to the pointer when it has one (walkaround), the
                // plain posture otherwise.
                let ahead = if self.ui.pcb.preview.first() == Some(&last) {
                    self.ui.pcb.preview.clone()
                } else {
                    posture45(last, hover, self.ui.pcb.diagonal_first)
                };
                for x in &ahead[1..] {
                    pts.push(*x);
                }
                let pts: Vec<Pt> = pts.iter().map(|x| q(*x)).collect();
                cv.stroke(p, &pts, col, route.width / 1000, 1);
                // Collisions the track would make, ringed in red.
                for (at, _) in &self.ui.pcb.collisions {
                    let (x, y) = cv.pt(q(*at));
                    p.ring(x, y, 10, 2, Color::rgb(255, 40, 40));
                }
                for v in &route.vias {
                    let (x, y) = cv.pt(q(*v));
                    p.circle(
                        x,
                        y,
                        (cv.len(b.rules.via_diameter / 1000) / 2).max(2) as u32,
                        Color::rgb(236, 236, 236),
                    );
                }
            }
            if !self.ui.pcb.poly.is_empty() {
                let col = layer_color(self.ui.pcb.layer);
                let mut pts: Vec<Pt> = self.ui.pcb.poly.iter().map(|x| q(*x)).collect();
                let h = q(hover.snap(self.ui.pcb.grid.max(1)));
                if self.pcb_tool() == "rect" {
                    let a = pts[0];
                    pts = vec![a, Pt::new(h.x, a.y), h, Pt::new(a.x, h.y), a];
                } else {
                    pts.push(h);
                }
                cv.stroke(p, &pts, col, 0, 1);
            }
            if let Some(Drag::Box { start, at }) = &self.ui.drag {
                let (x0, y0) = cv.pt(q(Pt::new(start.x.min(at.x), start.y.min(at.y))));
                let (x1, y1) = cv.pt(q(Pt::new(start.x.max(at.x), start.y.max(at.y))));
                p.border(
                    Rect::new(x0, y0, (x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32),
                    Color(255, 255, 255, 20),
                    0,
                    Color::rgb(200, 200, 200),
                );
            }
        }
    }

    pub(super) fn render_pcb_dialog(
        &self,
        p: &mut Painter,
        env: &crate::AppEnv<'_>,
        dialog: &Dialog,
    ) {
        let c = chrome(env.theme);
        let (w, h) = (env.width, env.height);
        let focus = self.ui.focus.as_deref();
        let close = |p: &mut Painter, body: Rect, label: &str, target: &str| {
            let by = body.y + body.height as i32 - 30;
            w::button(
                p,
                &c,
                Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                label,
                target,
                false,
            );
        };
        match dialog {
            Dialog::UpdatePcb { changes, applied } => {
                let body = w::dialog(p, &c, env.theme, w, h, 620, 440, dialog.title());
                p.label(
                    body.x,
                    body.y,
                    body.width,
                    if *applied {
                        "Changes applied to the board:"
                    } else {
                        "Changes to be applied:"
                    },
                    13,
                    w::INK,
                    true,
                    Align::Left,
                );
                let list = Rect::new(body.x, body.y + 24, body.width, body.height - 66);
                p.border(list, w::WHITE, 2, w::EDGE);
                if changes.is_empty() {
                    p.label(
                        list.x + 8,
                        list.y + 8,
                        list.width - 16,
                        "The board is up to date with the schematic.",
                        13,
                        w::MUTED,
                        false,
                        Align::Left,
                    );
                }
                for (i, ch) in changes.iter().enumerate() {
                    let y = list.y + 4 + i as i32 * 19;
                    if y + 19 > list.y + list.height as i32 {
                        p.label(
                            list.x + 8,
                            y,
                            list.width - 16,
                            &format!("… and {} more", changes.len() - i),
                            12,
                            w::MUTED,
                            false,
                            Align::Left,
                        );
                        break;
                    }
                    let (tag, col) = if ch.warning {
                        ("Warning: ", Color::rgb(190, 120, 0))
                    } else {
                        ("", w::INK)
                    };
                    p.label(
                        list.x + 8,
                        y,
                        list.width - 16,
                        &format!("{tag}{}", ch.message),
                        12,
                        col,
                        false,
                        Align::Left,
                    );
                }
                let by = body.y + body.height as i32 - 30;
                if *applied {
                    close(p, body, "Close", "kicad:dlg:ok");
                } else {
                    w::button(
                        p,
                        &c,
                        Rect::new(body.x + body.width as i32 - 216, by, 120, 28),
                        "Update PCB",
                        "kicad:dlg:apply",
                        true,
                    );
                    close(p, body, "Close", "kicad:dlg:cancel");
                }
            }
            Dialog::Drc { tab, selected } => {
                let body = w::dialog(p, &c, env.theme, w, h, 680, 480, dialog.title());
                let report = self.session.drc.clone().unwrap_or_default();
                let labels = [
                    format!("Violations ({})", report.violations.len()),
                    format!("Unconnected Items ({})", report.unconnected.len()),
                    format!("Schematic Parity ({})", report.parity.len()),
                ];
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                let y0 = w::tabs(
                    p,
                    &c,
                    body.x,
                    body.y,
                    &refs,
                    *tab as usize,
                    "kicad:dlg:tab:",
                );
                let list = Rect::new(
                    body.x,
                    y0,
                    body.width,
                    (body.y + body.height as i32 - 40 - y0).max(20) as u32,
                );
                p.border(list, w::WHITE, 2, w::EDGE);
                let rows = match tab {
                    0 => &report.violations,
                    1 => &report.unconnected,
                    _ => &report.parity,
                };
                if self.session.drc.is_none() {
                    p.label(
                        list.x + 8,
                        list.y + 8,
                        list.width - 16,
                        "Run DRC to check the board.",
                        13,
                        w::MUTED,
                        false,
                        Align::Left,
                    );
                }
                let mut y = list.y + 2;
                for (i, v) in rows.iter().enumerate() {
                    if y + 40 > list.y + list.height as i32 {
                        break;
                    }
                    let r = Rect::new(list.x + 2, y, list.width - 4, 38);
                    p.button(
                        r,
                        if *selected == Some(i) {
                            c.selection
                        } else {
                            Color::TRANSPARENT
                        },
                        2,
                        &format!("kicad:dlg:select:{i}"),
                        &v.message,
                    );
                    let (tag, col) = match v.severity {
                        drc::Severity::Error => ("[Error]", Color::rgb(200, 30, 30)),
                        drc::Severity::Warning => ("[Warning]", Color::rgb(190, 120, 0)),
                    };
                    p.label(r.x + 6, y + 2, 76, tag, 12, col, true, Align::Left);
                    p.label(
                        r.x + 84,
                        y + 2,
                        r.width - 90,
                        &v.message,
                        13,
                        w::INK,
                        false,
                        Align::Left,
                    );
                    p.label(
                        r.x + 84,
                        y + 19,
                        r.width - 90,
                        &v.items.join("; "),
                        12,
                        w::MUTED,
                        false,
                        Align::Left,
                    );
                    y += 40;
                }
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 186, by, 90, 28),
                    "Run DRC",
                    "kicad:dlg:run",
                    true,
                );
                close(p, body, "Close", "kicad:dlg:ok");
            }
            Dialog::Plot {
                svg,
                dir,
                layers,
                log,
            } => {
                let body = w::dialog(p, &c, env.theme, w, h, 640, 470, dialog.title());
                p.label(
                    body.x,
                    body.y + 3,
                    110,
                    "Plot format:",
                    13,
                    w::INK,
                    false,
                    Align::Left,
                );
                w::radio(
                    p,
                    &c,
                    body.x + 110,
                    body.y + 2,
                    "Gerber",
                    !*svg,
                    "kicad:dlg:fmt:gerber",
                );
                w::radio(
                    p,
                    &c,
                    body.x + 210,
                    body.y + 2,
                    "SVG",
                    *svg,
                    "kicad:dlg:fmt:svg",
                );
                w::label_field(
                    p,
                    &c,
                    body.x,
                    body.y + 30,
                    130,
                    body.width - 130,
                    "Output directory:",
                    dir,
                    "dir",
                    focus == Some("dir"),
                );
                p.label(
                    body.x,
                    body.y + 66,
                    200,
                    "Include Layers",
                    13,
                    w::INK,
                    true,
                    Align::Left,
                );
                for (i, l) in Layer::ALL.iter().enumerate() {
                    let (cx, cy) = (
                        body.x + (i as i32 % 2) * 160,
                        body.y + 88 + (i as i32 / 2) * 24,
                    );
                    w::checkbox(
                        p,
                        &c,
                        cx,
                        cy,
                        l.name(),
                        layers.contains(l),
                        &format!("kicad:dlg:layer:{}", l.name()),
                    );
                }
                let lx = body.x + 340;
                let lr = Rect::new(lx, body.y + 66, body.width - 340, body.height - 108);
                p.border(lr, w::WHITE, 2, w::EDGE);
                for (i, line) in log.iter().rev().take(14).rev().enumerate() {
                    p.label(
                        lr.x + 6,
                        lr.y + 4 + i as i32 * 17,
                        lr.width - 12,
                        line,
                        11,
                        w::INK,
                        false,
                        Align::Left,
                    );
                }
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x, by, 170, 28),
                    "Generate Drill Files...",
                    "kicad:dlg:drill",
                    false,
                );
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 186, by, 90, 28),
                    "Plot",
                    "kicad:dlg:plot",
                    true,
                );
                close(p, body, "Close", "kicad:dlg:cancel");
            }
            Dialog::Drill { log } => {
                let body = w::dialog(p, &c, env.theme, w, h, 520, 300, dialog.title());
                let out = self
                    .session
                    .project
                    .as_ref()
                    .map(|p| format!("{}/gerbers/", p.dir))
                    .unwrap_or_default();
                p.label(
                    body.x,
                    body.y,
                    body.width,
                    &format!("Output folder: {out}"),
                    13,
                    w::INK,
                    false,
                    Align::Left,
                );
                p.label(
                    body.x,
                    body.y + 22,
                    body.width,
                    "Drill file format: Excellon   Units: millimeters   Zeros: decimal format",
                    12,
                    w::MUTED,
                    false,
                    Align::Left,
                );
                for (i, line) in log.iter().enumerate() {
                    p.label(
                        body.x,
                        body.y + 52 + i as i32 * 18,
                        body.width,
                        line,
                        12,
                        w::INK,
                        false,
                        Align::Left,
                    );
                }
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 216, by, 120, 28),
                    "Generate Drill File",
                    "kicad:dlg:generate",
                    true,
                );
                close(p, body, "Close", "kicad:dlg:cancel");
            }
            Dialog::BoardSetup { fields, error } => {
                let body = w::dialog(p, &c, env.theme, w, h, 520, 400, dialog.title());
                p.label(
                    body.x,
                    body.y,
                    body.width,
                    "Design Rules ▸ Constraints and Net Classes (mm)",
                    12,
                    w::MUTED,
                    true,
                    Align::Left,
                );
                for (i, (k, v)) in fields.iter().enumerate() {
                    w::label_field(
                        p,
                        &c,
                        body.x,
                        body.y + 22 + i as i32 * 32,
                        220,
                        140,
                        k,
                        v,
                        k,
                        focus == Some(k.as_str()),
                    );
                }
                if !error.is_empty() {
                    p.label(
                        body.x,
                        body.y + body.height as i32 - 58,
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
            Dialog::ZoneProperties {
                net,
                layer,
                clearance,
                keepout,
                error,
                ..
            } => {
                let body = w::dialog(p, &c, env.theme, w, h, 540, 420, dialog.title());
                p.label(body.x, body.y, 200, "Net", 13, w::INK, true, Align::Left);
                let list = Rect::new(body.x, body.y + 22, 250, body.height - 64);
                p.border(list, w::WHITE, 2, w::EDGE);
                for (i, name) in self.session.board.nets.iter().enumerate() {
                    let y = list.y + 2 + i as i32 * 22;
                    if y + 22 > list.y + list.height as i32 {
                        break;
                    }
                    let shown = if name.is_empty() { "<no net>" } else { name };
                    w::row(
                        p,
                        &c,
                        Rect::new(list.x + 2, y, list.width - 4, 22),
                        shown,
                        &format!("kicad:dlg:net:{i}"),
                        *net == i,
                        w::INK,
                    );
                }
                let ox = body.x + 270;
                p.label(ox, body.y, 200, "Layer", 13, w::INK, true, Align::Left);
                w::radio(
                    p,
                    &c,
                    ox,
                    body.y + 22,
                    "F.Cu",
                    *layer == Layer::FCu,
                    "kicad:dlg:layer:F.Cu",
                );
                w::radio(
                    p,
                    &c,
                    ox,
                    body.y + 46,
                    "B.Cu",
                    *layer == Layer::BCu,
                    "kicad:dlg:layer:B.Cu",
                );
                w::label_field(
                    p,
                    &c,
                    ox,
                    body.y + 84,
                    130,
                    100,
                    "Clearance (mm):",
                    clearance,
                    "clearance",
                    focus == Some("clearance"),
                );
                p.label(
                    ox,
                    body.y + 118,
                    240,
                    if *keepout {
                        "Rule area: keeps tracks, vias, pads and fills out"
                    } else {
                        "Pad connections: thermal reliefs"
                    },
                    12,
                    w::MUTED,
                    false,
                    Align::Left,
                );
                w::checkbox(
                    p,
                    &c,
                    ox,
                    body.y + 140,
                    "Rule area (keepout)",
                    *keepout,
                    "kicad:dlg:keepout",
                );
                if !error.is_empty() {
                    p.label(
                        ox,
                        body.y + 170,
                        240,
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
            _ => {}
        }
    }

    pub(super) fn pcb_page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        for (id, label) in [
            ("kicad:pcb:save", "Save"),
            ("kicad:pcb:update", "Update PCB from Schematic"),
            ("kicad:pcb:tool:select", "Select"),
            ("kicad:pcb:tool:route", "Route Tracks"),
            ("kicad:pcb:tool:via", "Add Via"),
            ("kicad:pcb:tool:zone", "Add Filled Zone"),
            ("kicad:pcb:tool:keepout", "Add Rule Area"),
            ("kicad:pcb:tool:rect", "Draw Rectangle"),
            ("kicad:pcb:tool:line", "Draw Line"),
            ("kicad:pcb:drc", "Design Rules Checker"),
            ("kicad:pcb:fill", "Fill All Zones"),
            ("kicad:pcb:plot", "Plot"),
            ("kicad:pcb:setup", "Board Setup"),
            ("kicad:pcb:router", "Interactive Router Settings"),
            ("kicad:pcb:3d", "3D Viewer"),
            ("kicad:pcb:fped", "Footprint Editor"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
                style: None,
            });
        }
        for l in Layer::ALL {
            let id = format!("kicad:pcb:layer:{}", l.name());
            page.elements.push(E::Button {
                id: id.clone(),
                text: format!("Layer {}", l.user_name()),
                action: act(&id),
                style: None,
            });
        }
        let b = &self.session.board;
        for f in &b.footprints {
            page.elements.push(E::Text {
                id: format!("kicad-footprint-{}", f.id),
                text: format!(
                    "{} {} at {} mm, {} mm, {}°{}",
                    f.reference,
                    f.fp_id,
                    mm_text(f.pos.x),
                    mm_text(f.pos.y),
                    angle_text(f.angle),
                    if f.back { " (back)" } else { "" }
                ),
            });
        }
        page.elements.push(E::Text {
            id: "kicad-board".into(),
            text: format!(
                "{} tracks, {} vias, {} zones, {} unrouted connections, active layer {}",
                b.tracks.len(),
                b.vias.len(),
                b.zones.len(),
                drc::ratsnest(b).len(),
                self.ui.pcb.layer.name()
            ),
        });
        if let Some(r) = &self.session.drc {
            for (i, v) in r
                .violations
                .iter()
                .chain(&r.unconnected)
                .chain(&r.parity)
                .enumerate()
            {
                page.elements.push(E::Text {
                    id: format!("kicad-drc-{i}"),
                    text: format!("{:?}: {}", v.severity, v.message),
                });
            }
        }
    }
}

fn pcb_menus(k: &Kicad) -> Vec<(&'static str, Vec<MenuItem>)> {
    let proj = |t: &str| -> Result<String, &'static str> {
        if k.session.project.is_some() {
            Ok(t.to_owned())
        } else {
            Err("no project is open")
        }
    };
    let has_sel = !k.board_selected().is_empty();
    let sel = |t: &str| -> Result<String, &'static str> {
        if has_sel {
            Ok(t.to_owned())
        } else {
            Err("nothing is selected")
        }
    };
    let zones = |t: &str| -> Result<String, &'static str> {
        if k.session.board.zones.is_empty() {
            Err("the board has no zones")
        } else {
            Ok(t.to_owned())
        }
    };
    vec![
        (
            "File",
            vec![
                MenuItem::new("Save", "Ctrl+S", proj("kicad:pcb:save")),
                MenuItem::new("Board Setup...", "", Ok("kicad:pcb:setup".into())).sep(),
                MenuItem::new("Plot...", "", proj("kicad:pcb:plot")).sep(),
                MenuItem::new("Drill Files (.drl)...", "", proj("kicad:pcb:drill")),
            ],
        ),
        (
            "Edit",
            vec![
                MenuItem::new(
                    "Undo",
                    "Ctrl+Z",
                    if k.session.pcb_history.past.is_empty() {
                        Err("nothing to undo")
                    } else {
                        Ok("kicad:pcb:undo".into())
                    },
                ),
                MenuItem::new(
                    "Redo",
                    "Ctrl+Y",
                    if k.session.pcb_history.future.is_empty() {
                        Err("nothing to redo")
                    } else {
                        Ok("kicad:pcb:redo".into())
                    },
                ),
                MenuItem::new("Delete", "Del", sel("kicad:pcb:delete")).sep(),
                MenuItem::new("Rotate", "R", sel("kicad:pcb:rotate")),
                MenuItem::new("Rotate Clockwise", "Shift+R", sel("kicad:pcb:rotate-cw")),
                MenuItem::new("Rotate 45°", "Ctrl+R", sel("kicad:pcb:rotate45")),
                MenuItem::new("Change Side / Flip", "F", sel("kicad:pcb:flip")),
                MenuItem::new("Properties...", "E", sel("kicad:pcb:properties")).sep(),
            ],
        ),
        (
            "View",
            vec![
                MenuItem::new("Zoom In", "F1", Ok("kicad:pcb:zoom:in".into())),
                MenuItem::new("Zoom Out", "F2", Ok("kicad:pcb:zoom:out".into())),
                MenuItem::new("Zoom to Fit", "Home", Ok("kicad:pcb:zoom:fit".into())),
                MenuItem::new("Show Ratsnest", "", Ok("kicad:pcb:ratsnest".into())).sep(),
                MenuItem::new("3D Viewer", "Alt+3", Ok("kicad:pcb:3d".into())).sep(),
            ],
        ),
        (
            "Place",
            vec![
                MenuItem::new("Via", "", Ok("kicad:pcb:tool:via".into())),
                MenuItem::new("Zone", "", Ok("kicad:pcb:tool:zone".into())),
                MenuItem::new("Rule Area", "", Ok("kicad:pcb:tool:keepout".into())),
                MenuItem::new("Line", "", Ok("kicad:pcb:tool:line".into())).sep(),
                MenuItem::new("Rectangle", "", Ok("kicad:pcb:tool:rect".into())),
            ],
        ),
        (
            "Route",
            vec![
                MenuItem::new("Single Track", "X", Ok("kicad:pcb:tool:route".into())),
                MenuItem::new("Switch to F.Cu", "PgUp", Ok("kicad:pcb:layer:F.Cu".into())).sep(),
                MenuItem::new("Switch to B.Cu", "PgDn", Ok("kicad:pcb:layer:B.Cu".into())),
                MenuItem::new(
                    "Interactive Router Settings...",
                    "",
                    Ok("kicad:pcb:router".into()),
                )
                .sep(),
            ],
        ),
        (
            "Inspect",
            vec![MenuItem::new(
                "Design Rules Checker",
                "",
                Ok("kicad:pcb:drc".into()),
            )],
        ),
        (
            "Tools",
            vec![
                MenuItem::new(
                    "Update PCB from Schematic...",
                    "F8",
                    Ok("kicad:pcb:update".into()),
                ),
                MenuItem::new("Fill All Zones", "B", zones("kicad:pcb:fill")).sep(),
                MenuItem::new("Unfill All Zones", "Ctrl+B", zones("kicad:pcb:unfill")),
                MenuItem::new("Footprint Editor", "", Ok("kicad:pcb:fped".into())).sep(),
            ],
        ),
        (
            "Help",
            vec![MenuItem::new("About KiCad", "", Ok("kicad:about".into()))],
        ),
    ]
}
