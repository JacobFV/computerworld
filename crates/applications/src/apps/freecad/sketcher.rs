//! Sketch edit mode: geometry tools with FreeCAD's automatic constraints, the
//! constraint commands, selection, dragging through the solver, and dimension editing.
use super::commands::{parse_quantity, value_text};
use super::*;
use cw_cad::math;
use cw_cad::sketch::{
    solver, tools, Constraint, ConstraintType as T, Geom, Sketch, GEO_UNDEF, H_AXIS, V_AXIS,
};
use cw_cad::Frame;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Point,
    Line,
    Polyline,
    ArcCenter,
    Arc3,
    Circle,
    Circle3,
    Rectangle,
    Slot,
    Fillet,
    Trim,
    Extend,
}
impl Tool {
    pub fn from_command(id: &str) -> Option<Tool> {
        Some(match id {
            "Sketcher_CreatePoint" => Tool::Point,
            "Sketcher_CreateLine" => Tool::Line,
            "Sketcher_CreatePolyline" => Tool::Polyline,
            "Sketcher_CreateArc" => Tool::ArcCenter,
            "Sketcher_Create3PointArc" => Tool::Arc3,
            "Sketcher_CreateCircle" => Tool::Circle,
            "Sketcher_Create3PointCircle" => Tool::Circle3,
            "Sketcher_CreateRectangle" => Tool::Rectangle,
            "Sketcher_CreateSlot" => Tool::Slot,
            "Sketcher_CreateFillet" => Tool::Fillet,
            "Sketcher_Trimming" => Tool::Trim,
            "Sketcher_Extend" => Tool::Extend,
            _ => return None,
        })
    }
    pub fn command(self) -> &'static str {
        match self {
            Tool::Point => "Sketcher_CreatePoint",
            Tool::Line => "Sketcher_CreateLine",
            Tool::Polyline => "Sketcher_CreatePolyline",
            Tool::ArcCenter => "Sketcher_CreateArc",
            Tool::Arc3 => "Sketcher_Create3PointArc",
            Tool::Circle => "Sketcher_CreateCircle",
            Tool::Circle3 => "Sketcher_Create3PointCircle",
            Tool::Rectangle => "Sketcher_CreateRectangle",
            Tool::Slot => "Sketcher_CreateSlot",
            Tool::Fillet => "Sketcher_CreateFillet",
            Tool::Trim => "Sketcher_Trimming",
            Tool::Extend => "Sketcher_Extend",
        }
    }
    /// What the status bar asks for next, given how many clicks are in.
    pub fn prompt(self, clicks: usize) -> &'static str {
        match (self, clicks) {
            (Tool::Point, _) => "Click to place a point",
            (Tool::Line, 0) => "Click the start point of the line",
            (Tool::Line, _) => "Click the end point of the line",
            (Tool::Polyline, 0) => "Click the first point of the polyline",
            (Tool::Polyline, _) => {
                "Click the next point; click the first point to close, Enter to finish"
            }
            (Tool::ArcCenter, 0) => "Click the centre of the arc",
            (Tool::ArcCenter, 1) => "Click the start of the arc",
            (Tool::ArcCenter, _) => "Click the end of the arc (counter-clockwise)",
            (Tool::Arc3, 0) => "Click the start point of the arc",
            (Tool::Arc3, 1) => "Click the end point of the arc",
            (Tool::Arc3, _) => "Click a point on the arc",
            (Tool::Circle, 0) => "Click the centre of the circle",
            (Tool::Circle, _) => "Click a point on the rim",
            (Tool::Circle3, 0) => "Click the first point on the circle",
            (Tool::Circle3, 1) => "Click the second point on the circle",
            (Tool::Circle3, _) => "Click the third point on the circle",
            (Tool::Rectangle, 0) => "Click the first corner of the rectangle",
            (Tool::Rectangle, _) => "Click the opposite corner",
            (Tool::Slot, 0) => "Click the centre of the first end",
            (Tool::Slot, 1) => "Click the centre of the second end",
            (Tool::Slot, _) => "Click to set the slot's radius",
            (Tool::Fillet, _) => "Click the corner where two lines meet",
            (Tool::Trim, _) => "Click the part of an edge to trim away",
            (Tool::Extend, 0) => "Click the edge to extend, near the end to extend",
            (Tool::Extend, _) => "Click the edge to extend it to",
        }
    }
    fn clicks_needed(self) -> usize {
        match self {
            Tool::Point | Tool::Fillet | Tool::Trim => 1,
            Tool::Line | Tool::Circle | Tool::Rectangle | Tool::Extend => 2,
            Tool::ArcCenter | Tool::Arc3 | Tool::Circle3 | Tool::Slot => 3,
            Tool::Polyline => usize::MAX,
        }
    }
}

/// Pick tolerance in pixels, as FreeCAD's default "pick radius".
pub const PICK_PX: f64 = 8.0;

impl Cad {
    pub fn sketch(&self) -> Option<&Sketch> {
        let name = &self.sketch_edit()?.name;
        match &self.doc.get(name)?.feature {
            Feature::Sketch { sketch, .. } => Some(sketch),
            _ => None,
        }
    }
    fn sketch_mut(&mut self) -> Option<&mut Sketch> {
        let name = self.sketch_edit()?.name.clone();
        match &mut self.doc.get_mut(&name)?.feature {
            Feature::Sketch { sketch, .. } => Some(sketch),
            _ => None,
        }
    }
    /// Where the edited sketch sits in the world.
    pub fn sketch_frame(&self) -> Option<Frame> {
        let name = &self.sketch_edit()?.name;
        self.model().frames.get(name).copied()
    }
    pub(crate) fn open_sketch(&mut self, name: &str) -> Result<(), String> {
        let Some(Feature::Sketch { sketch, .. }) = self.doc.get(name).map(|o| &o.feature) else {
            return Err("not a sketch".into());
        };
        let mut probe = sketch.clone();
        let report = solver::solve(&mut probe, None);
        self.task = Some(Task::Sketch(Box::new(SketchEdit {
            name: name.to_owned(),
            tool: None,
            clicks: vec![],
            snaps: vec![],
            picked: vec![],
            constraints: vec![],
            report,
            construction: false,
            fillet_radius: 2.0,
            pending: None,
            folded: BTreeSet::new(),
        })));
        self.combo = ComboTab::Tasks;
        self.selection.clear();
        if let Some(o) = self.doc.get_mut(name) {
            o.visible = true;
        }
        self.view_sketch();
        self.status = format!("Editing {}", self.label_of(name));
        Ok(())
    }
    /// Look straight at the sketch plane and frame its geometry (Sketcher_ViewSketch).
    pub(crate) fn view_sketch(&mut self) {
        let Some(frame) = self.sketch_frame() else {
            return;
        };
        self.camera.look_from(frame.z, frame.y);
        let (w, h) = self.view_px();
        let bounds = self.sketch().and_then(|s| s.bounds());
        match bounds {
            Some((lo, hi)) => {
                let (a, b) = (frame.to_world(lo), frame.to_world(hi));
                self.camera.fit(a.min(b), a.max(b), w, h);
                self.camera.target = frame.to_world(lo.lerp(hi, 0.5));
            }
            None => {
                self.camera.target = frame.origin;
                self.camera.half_height = self.camera.half_height.clamp(20.0, 200.0);
            }
        }
    }
    pub(crate) fn leave_sketch(&mut self) {
        let name = self.sketch_edit().map(|s| s.name.clone());
        self.task = None;
        self.combo = ComboTab::Model;
        self.field = None;
        if matches!(self.dialog, Some(Dialog::Dimension { .. })) {
            self.dialog = None;
        }
        // Leaving the sketch recomputes everything that uses it.
        self.recompute();
        if let Some(name) = name {
            self.selection = vec![Sel {
                object: name.clone(),
                sub: String::new(),
                point: V3::ZERO,
            }];
            // A sketch used by a feature hides again, as FreeCAD does.
            let used = self
                .doc
                .objects
                .iter()
                .any(|o| o.feature.profile() == Some(name.as_str()));
            if used {
                if let Some(o) = self.doc.get_mut(&name) {
                    o.visible = false;
                }
            }
        }
        self.status = String::new();
    }
    pub(crate) fn refresh_sketch_report(&mut self) {
        let Some(mut s) = self.sketch().cloned() else {
            return;
        };
        let report = solver::solve(&mut s, None);
        if let Some(e) = self.sketch_edit_mut() {
            e.report = report;
        }
    }
    /// Solve the edited sketch in place, record the verdict and show it.
    fn solve_sketch(&mut self) {
        let report = match self.sketch_mut() {
            Some(s) => solver::solve(s, None),
            None => return,
        };
        self.status = report.message();
        if matches!(
            report.status,
            cw_cad::sketch::SolveStatus::Conflicting | cw_cad::sketch::SolveStatus::Redundant
        ) {
            self.log(
                ReportKind::Warning,
                &format!(
                    "{}: {}",
                    self.sketch_edit()
                        .map(|s| s.name.clone())
                        .unwrap_or_default(),
                    report.message()
                ),
            );
        }
        if let Some(e) = self.sketch_edit_mut() {
            e.report = report;
        }
        self.modified = true;
    }

    pub(crate) fn start_tool(&mut self, tool: Tool) -> Result<(), String> {
        let s = self
            .sketch_edit_mut()
            .ok_or("Open a sketch for editing first")?;
        s.tool = if s.tool == Some(tool) {
            None
        } else {
            Some(tool)
        };
        s.clicks.clear();
        s.snaps.clear();
        s.pending = None;
        self.status = tool.prompt(0).into();
        Ok(())
    }

    /// The sketch point under a view pixel, and what it snaps to.
    pub(crate) fn sketch_point(&self, x: f64, y: f64) -> Option<(V2, Option<(i32, Pos)>)> {
        let frame = self.sketch_frame()?;
        let (w, h) = self.view_px();
        let (o, d) = self.camera.ray(x, y, w, h);
        let denom = d.dot(frame.z);
        if denom.abs() < 1e-12 {
            return None;
        }
        let t = (frame.origin - o).dot(frame.z) / denom;
        let p = frame.to_local2(o + d * t);
        let tol = PICK_PX * 2.0 * self.camera.half_height / f64::from(h);
        let snap = self.sketch().and_then(|s| tools::pick(s, p, tol));
        // A point snap lands exactly on the point.
        let at = match snap {
            Some((g, pos)) if pos != Pos::None => {
                self.sketch().and_then(|s| s.point(g, pos)).unwrap_or(p)
            }
            _ => p,
        };
        Some((at, snap))
    }

    /// A click in the 3D view while a sketch is open.
    pub(crate) fn sketch_click(&mut self, x: f64, y: f64) -> Result<Vec<AppEffect>, String> {
        let (p, snap) = self
            .sketch_point(x, y)
            .ok_or("the view is edge-on to the sketch")?;
        let edit = self.sketch_edit().ok_or("no sketch")?.clone();
        if let Some(tool) = edit.tool {
            return self.tool_click(tool, p, snap);
        }
        // Selection: a click on geometry adds it (FreeCAD accumulates in the Sketcher);
        // on a constraint's label selects the constraint; on nothing clears.
        match snap {
            Some(pick) if pick.0 >= 0 || pick.0 == H_AXIS || pick.0 == V_AXIS => {
                let e = self.sketch_edit_mut().unwrap();
                if let Some(i) = e.picked.iter().position(|q| *q == pick) {
                    e.picked.remove(i);
                } else {
                    e.picked.push(pick);
                }
                if let Some(cmd) = e.pending.clone() {
                    return self.constrain(&cmd).map(|_| vec![]).or_else(|err| {
                        if err.starts_with("Select") {
                            Ok(vec![])
                        } else {
                            Err(err)
                        }
                    });
                }
                self.status = self.describe_pick(pick);
            }
            _ => {
                let e = self.sketch_edit_mut().unwrap();
                e.picked.clear();
                e.constraints.clear();
            }
        }
        Ok(vec![])
    }

    fn describe_pick(&self, (g, pos): (i32, Pos)) -> String {
        let what = match g {
            H_AXIS if pos == Pos::Start => "RootPoint".to_owned(),
            H_AXIS => "H_Axis".to_owned(),
            V_AXIS => "V_Axis".to_owned(),
            g => match pos {
                Pos::None => format!("Edge{}", g + 1),
                _ => format!("Vertex{}", self.vertex_number(g, pos).unwrap_or(0)),
            },
        };
        format!(
            "Selected {}.{}",
            self.sketch_edit().map(|s| s.name.as_str()).unwrap_or(""),
            what
        )
    }
    /// FreeCAD numbers sketch vertices across all geometry, in order.
    pub fn vertex_number(&self, geo: i32, pos: Pos) -> Option<usize> {
        let s = self.sketch()?;
        let mut n = 0;
        for (i, g) in s.geos.iter().enumerate() {
            for p in g.geom.points() {
                n += 1;
                if i as i32 == geo && p == pos {
                    return Some(n);
                }
            }
        }
        None
    }

    fn add_auto(&mut self, new: (i32, Pos), snap: Option<(i32, Pos)>) {
        let Some((g, pos)) = snap else { return };
        let Some(s) = self.sketch_mut() else { return };
        if g == new.0 {
            return;
        }
        let c = if pos == Pos::None {
            if new.1 == Pos::Center {
                return;
            }
            Constraint::new(T::PointOnObject, new.0, new.1).with_second(g, Pos::None)
        } else {
            Constraint::new(T::Coincident, new.0, new.1).with_second(g, pos)
        };
        let _ = s.add_constraint(c);
    }

    fn tool_click(
        &mut self,
        tool: Tool,
        p: V2,
        snap: Option<(i32, Pos)>,
    ) -> Result<Vec<AppEffect>, String> {
        let construction = self.sketch_edit().map(|e| e.construction).unwrap_or(false);
        // Editing tools act on what is under the pointer rather than on points.
        match tool {
            Tool::Trim => {
                let (g, _) = snap
                    .filter(|s| s.0 >= 0)
                    .ok_or("Click on an edge to trim")?;
                self.checkpoint("Trim edge");
                let msg = tools::trim(self.sketch_mut().unwrap(), g, p)?;
                self.after_sketch_edit(&msg);
                return Ok(vec![]);
            }
            Tool::Fillet => {
                let (g, pos) = snap
                    .filter(|s| s.0 >= 0 && s.1 != Pos::None)
                    .ok_or("Click on the corner point")?;
                let r = self.sketch_edit().map(|e| e.fillet_radius).unwrap_or(2.0);
                self.checkpoint("Create fillet");
                match tools::fillet(self.sketch_mut().unwrap(), g, pos, r) {
                    Ok(_) => self.after_sketch_edit("Created fillet"),
                    Err(e) => {
                        self.undo.pop();
                        return Err(e);
                    }
                }
                return Ok(vec![]);
            }
            Tool::Extend => {
                let (g, _) = snap.filter(|s| s.0 >= 0).ok_or("Click on an edge")?;
                let e = self.sketch_edit_mut().unwrap();
                if e.clicks.is_empty() {
                    e.clicks.push(p);
                    e.snaps.push(Some((g, Pos::None)));
                    self.status = tool.prompt(1).into();
                    return Ok(vec![]);
                }
                let (first, near) = (e.snaps[0].unwrap().0, e.clicks[0]);
                e.clicks.clear();
                e.snaps.clear();
                self.checkpoint("Extend edge");
                match tools::extend(self.sketch_mut().unwrap(), first, near, g) {
                    Ok(msg) => self.after_sketch_edit(&msg),
                    Err(err) => {
                        self.undo.pop();
                        return Err(err);
                    }
                }
                return Ok(vec![]);
            }
            _ => {}
        }
        let e = self.sketch_edit_mut().unwrap();
        // Polyline: a click on its own first point closes it.
        if tool == Tool::Polyline && e.clicks.len() >= 2 && e.clicks[0].dist(p) < 1e-9 {
            return self.finish_polyline(true);
        }
        e.clicks.push(p);
        e.snaps.push(snap);
        let n = e.clicks.len();
        if tool == Tool::Polyline {
            self.status = tool.prompt(n).into();
            return Ok(vec![]);
        }
        if n < tool.clicks_needed() {
            self.status = tool.prompt(n).into();
            return Ok(vec![]);
        }
        let clicks = std::mem::take(&mut e.clicks);
        let snaps = std::mem::take(&mut e.snaps);
        let label = super::commands::command(tool.command())
            .map(|c| c.label)
            .unwrap_or("Create geometry");
        self.checkpoint(label);
        let s = self.sketch_mut().unwrap();
        let before = s.geos.len() as i32;
        let made: Result<Vec<(i32, Pos, usize)>, String> = (|| {
            Ok(match tool {
                Tool::Point => {
                    let id = tools::point(s, clicks[0], construction);
                    vec![(id, Pos::Start, 0)]
                }
                Tool::Line => {
                    let id = tools::line(s, clicks[0], clicks[1], construction, true)?;
                    vec![(id, Pos::Start, 0), (id, Pos::End, 1)]
                }
                Tool::Circle => {
                    let id = tools::circle(s, clicks[0], clicks[0].dist(clicks[1]), construction)?;
                    vec![(id, Pos::Center, 0)]
                }
                Tool::Circle3 => {
                    tools::circle_3pt(s, clicks[0], clicks[1], clicks[2], construction)?;
                    vec![]
                }
                Tool::ArcCenter => {
                    let id = tools::arc_center(s, clicks[0], clicks[1], clicks[2], construction)?;
                    vec![(id, Pos::Center, 0), (id, Pos::Start, 1), (id, Pos::End, 2)]
                }
                Tool::Arc3 => {
                    let id = tools::arc_3pt(s, clicks[0], clicks[2], clicks[1], construction)?;
                    // Whichever end the arc starts from, snap the clicked points to it.
                    let start = s.point(id, Pos::Start).unwrap();
                    if start.dist(clicks[0]) < start.dist(clicks[1]) {
                        vec![(id, Pos::Start, 0), (id, Pos::End, 1)]
                    } else {
                        vec![(id, Pos::End, 0), (id, Pos::Start, 1)]
                    }
                }
                Tool::Rectangle => {
                    let ids = tools::rectangle(s, clicks[0], clicks[1], construction)?;
                    // The first click is whichever corner it was.
                    let corner = |p: V2| -> (i32, Pos) {
                        let pts = [
                            (ids[0], Pos::Start),
                            (ids[1], Pos::Start),
                            (ids[2], Pos::Start),
                            (ids[3], Pos::Start),
                        ];
                        *pts.iter()
                            .min_by(|a, b| {
                                let da = s.point(a.0, a.1).unwrap().dist(p);
                                let db = s.point(b.0, b.1).unwrap().dist(p);
                                da.total_cmp(&db)
                            })
                            .unwrap()
                    };
                    let (a, b) = (corner(clicks[0]), corner(clicks[1]));
                    vec![(a.0, a.1, 0), (b.0, b.1, 1)]
                }
                Tool::Slot => {
                    let (c1, c2) = (clicks[0], clicks[1]);
                    let d = (c2 - c1).norm();
                    let r = (clicks[2] - c1).cross(d).abs().max(1e-3);
                    let ids = tools::slot(s, c1, c2, r, construction)?;
                    vec![(ids[0], Pos::Center, 0), (ids[1], Pos::Center, 1)]
                }
                _ => vec![],
            })
        })();
        let made = match made {
            Ok(m) => m,
            Err(err) => {
                self.undo.pop();
                return Err(err);
            }
        };
        for (g, pos, click) in made {
            if let Some(snap) = snaps.get(click).copied().flatten() {
                if snap.0 < before {
                    self.add_auto((g, pos), Some(snap));
                }
            }
        }
        self.after_sketch_edit(&format!("{label} done"));
        // FreeCAD's tools stay active for the next one ("continuous mode").
        if let Some(e) = self.sketch_edit_mut() {
            e.tool = Some(tool);
        }
        self.status = tool.prompt(0).into();
        Ok(vec![])
    }

    fn after_sketch_edit(&mut self, message: &str) {
        self.solve_sketch();
        if let Some(e) = self.sketch_edit_mut() {
            e.picked.clear();
            e.constraints.clear();
        }
        let solver = self.status.clone();
        self.status = format!("{message}. {solver}");
    }

    fn finish_polyline(&mut self, close: bool) -> Result<Vec<AppEffect>, String> {
        let construction = self.sketch_edit().map(|e| e.construction).unwrap_or(false);
        let e = self.sketch_edit_mut().ok_or("no sketch")?;
        let clicks = std::mem::take(&mut e.clicks);
        let snaps = std::mem::take(&mut e.snaps);
        if clicks.len() < 2 {
            return Ok(vec![]);
        }
        self.checkpoint("Create polyline");
        let s = self.sketch_mut().unwrap();
        let before = s.geos.len() as i32;
        let ids = tools::polyline(s, &clicks, close, construction)?;
        for (k, snap) in snaps.iter().enumerate() {
            let Some(snap) = snap.filter(|s| s.0 < before) else {
                continue;
            };
            let at = if k < ids.len() {
                (ids[k], Pos::Start)
            } else {
                (ids[k - 1], Pos::End)
            };
            self.add_auto(at, Some(snap));
        }
        self.after_sketch_edit("Created polyline");
        Ok(vec![])
    }

    /// Enter: finish a polyline in progress.
    pub(crate) fn finish_tool(&mut self) -> Result<Vec<AppEffect>, String> {
        match self.sketch_edit().and_then(|e| e.tool) {
            Some(Tool::Polyline) => self.finish_polyline(false),
            _ => Ok(vec![]),
        }
    }

    pub(crate) fn toggle_construction(&mut self) -> Result<(), String> {
        let picked: Vec<i32> = self
            .sketch_edit()
            .map(|e| {
                e.picked
                    .iter()
                    .filter(|p| p.0 >= 0 && p.1 == Pos::None)
                    .map(|p| p.0)
                    .collect()
            })
            .unwrap_or_default();
        if picked.is_empty() {
            // With nothing selected it switches the creation mode, as FreeCAD does.
            let e = self.sketch_edit_mut().unwrap();
            e.construction = !e.construction;
            self.status = if e.construction {
                "Construction mode on"
            } else {
                "Construction mode off"
            }
            .into();
            return Ok(());
        }
        self.checkpoint("Toggle construction geometry");
        let s = self.sketch_mut().unwrap();
        for g in picked {
            if let Some(geo) = s.geos.get_mut(g as usize) {
                geo.construction = !geo.construction;
            }
        }
        self.after_sketch_edit("Toggled construction geometry");
        Ok(())
    }

    pub(crate) fn toggle_driving(&mut self) -> Result<(), String> {
        let chosen = self
            .sketch_edit()
            .map(|e| e.constraints.clone())
            .unwrap_or_default();
        let s = self.sketch().ok_or("no sketch")?;
        if chosen.iter().any(|i| {
            !s.constraints
                .get(*i)
                .is_some_and(|c| c.kind.is_dimensional())
        }) {
            return Err("Only dimensional constraints can be reference constraints".into());
        }
        self.checkpoint("Toggle driving/reference constraint");
        let s = self.sketch_mut().unwrap();
        for i in &chosen {
            let c = &mut s.constraints[*i];
            c.driving = !c.driving;
        }
        // Reference dimensions track the geometry; one turned driving again keeps the
        // value the geometry has, so switching back never moves anything.
        let snapshot = s.clone();
        for c in s.constraints.iter_mut().filter(|c| !c.driving) {
            if let Ok(v) = solver::measure(&snapshot, c) {
                c.value = v;
            }
        }
        self.after_sketch_edit("Toggled driving/reference");
        Ok(())
    }

    pub(crate) fn sketch_delete(&mut self) -> Result<(), String> {
        let e = self.sketch_edit().ok_or("no sketch")?.clone();
        let geos: Vec<i32> = e
            .picked
            .iter()
            .filter(|p| p.0 >= 0 && p.1 == Pos::None)
            .map(|p| p.0)
            .collect();
        // A selected point on its own takes the constraints that pin it.
        let points: Vec<(i32, Pos)> = e
            .picked
            .iter()
            .filter(|p| p.0 >= 0 && p.1 != Pos::None)
            .copied()
            .collect();
        let s = self.sketch().ok_or("no sketch")?;
        let mut cons: Vec<usize> = e.constraints.clone();
        for (g, pos) in &points {
            for (i, c) in s.constraints.iter().enumerate() {
                if (c.first == *g && c.first_pos == *pos)
                    || (c.second == *g && c.second_pos == *pos)
                    || (c.third == *g && c.third_pos == *pos)
                {
                    cons.push(i);
                }
            }
        }
        // A point geometry selected by its point is deleted as geometry.
        let mut geos = geos;
        for (g, _) in &points {
            if matches!(s.geo(*g).map(|x| &x.geom), Some(Geom::Point { .. })) {
                geos.push(*g);
            }
        }
        if geos.is_empty() && cons.is_empty() {
            return Err("Select sketch geometry or constraints to delete".into());
        }
        self.checkpoint("Delete");
        let s = self.sketch_mut().unwrap();
        s.delete_constraints(&cons);
        s.delete_geos(&geos);
        self.after_sketch_edit("Deleted the selection");
        Ok(())
    }

    /// Apply a constraint command to the sketch selection. Without enough selected, the
    /// command waits for picks (FreeCAD's continuous constraint mode).
    pub(crate) fn constrain(&mut self, cmd: &str) -> Result<(), String> {
        let e = self
            .sketch_edit()
            .ok_or("Open a sketch for editing first")?
            .clone();
        let s = self.sketch().ok_or("no sketch")?.clone();
        let picks = e.picked.clone();
        let result = build_constraints(&s, cmd, &picks);
        let list = match result {
            Ok(list) => list,
            Err(err) => {
                // Not enough (or the wrong things) selected: wait for picks.
                if picks.is_empty() || err.starts_with("Select") {
                    if let Some(e) = self.sketch_edit_mut() {
                        e.pending = Some(cmd.to_owned());
                        e.tool = None;
                    }
                    self.status = err.clone();
                    if picks.is_empty() {
                        return Ok(());
                    }
                }
                return Err(err);
            }
        };
        let label = super::commands::command(cmd)
            .map(|c| c.label)
            .unwrap_or("Add constraint");
        self.checkpoint(label);
        let mut dims = Vec::new();
        {
            let s = self.sketch_mut().unwrap();
            for c in list {
                let dimensional = c.kind.is_dimensional();
                let i = s.add_constraint(c)?;
                if dimensional {
                    dims.push(i);
                }
            }
        }
        if let Some(e) = self.sketch_edit_mut() {
            e.pending = None;
        }
        self.after_sketch_edit(label);
        // A new dimension asks for its value, seeded with what the geometry measures.
        if let Some(&i) = dims.last() {
            if cmd != "Sketcher_ConstrainLock" {
                self.edit_constraint_value(i)?;
                let title = match self.sketch().map(|s| s.constraints[i].kind) {
                    Some(T::Angle) => "Insert angle",
                    Some(T::Radius) => "Insert radius",
                    Some(T::Diameter) => "Insert diameter",
                    _ => "Insert length",
                };
                self.dialog = Some(Dialog::Dimension {
                    index: i,
                    title: title.into(),
                });
            }
        }
        Ok(())
    }

    /// Focus the value of constraint `i` for editing.
    pub(crate) fn edit_constraint_value(&mut self, i: usize) -> Result<Vec<AppEffect>, String> {
        let s = self.sketch().ok_or("Open a sketch for editing first")?;
        let c = s.constraints.get(i).ok_or("no such constraint")?;
        if !c.kind.is_dimensional() {
            return Err("Only dimensional constraints have a value".into());
        }
        let text = value_text(c);
        self.field = Some(Field {
            target: FieldTarget::Constraint { index: i },
            text,
            replace: true,
        });
        if let Some(e) = self.sketch_edit_mut() {
            e.constraints = vec![i];
        }
        Ok(vec![])
    }

    pub(crate) fn set_constraint_value(&mut self, i: usize, text: &str) -> Result<(), String> {
        let s = self.sketch().ok_or("no sketch")?;
        let c = s.constraints.get(i).ok_or("no such constraint")?.clone();
        let angle = c.kind == T::Angle;
        let v = parse_quantity(text, angle)?;
        let value = if angle { math::radians(v) } else { v };
        if matches!(c.kind, T::Radius | T::Diameter) && value <= 0.0 {
            return Err("A radius must be positive".into());
        }
        if matches!(c.kind, T::Distance) && value < 0.0 {
            return Err("A distance cannot be negative".into());
        }
        self.checkpoint("Change constraint value");
        let s = self.sketch_mut().unwrap();
        s.constraints[i].value = value;
        self.after_sketch_edit(&format!("Constraint{} = {}", i + 1, text.trim()));
        Ok(())
    }

    /// Clicks in the sketch task panel.
    pub(crate) fn sketch_command(&mut self, rest: &str) -> Result<Vec<AppEffect>, String> {
        if let Some(i) = rest
            .strip_prefix("constraint:")
            .or_else(|| rest.strip_prefix("dim:"))
        {
            let i: usize = i.parse().map_err(|_| "bad constraint")?;
            let n = self.sketch().map(|s| s.constraints.len()).unwrap_or(0);
            if i >= n {
                return Err("no such constraint".into());
            }
            let e = self.sketch_edit_mut().ok_or("no sketch")?;
            if let Some(at) = e.constraints.iter().position(|c| *c == i) {
                e.constraints.remove(at);
            } else {
                e.constraints.push(i);
            }
            return Ok(vec![]);
        }
        if let Some(i) = rest.strip_prefix("element:") {
            let g: i32 = i.parse().map_err(|_| "bad element")?;
            let n = self.sketch().map(|s| s.geos.len()).unwrap_or(0);
            if g < 0 || g as usize >= n {
                return Err("no such element".into());
            }
            let e = self.sketch_edit_mut().ok_or("no sketch")?;
            if let Some(at) = e.picked.iter().position(|p| *p == (g, Pos::None)) {
                e.picked.remove(at);
            } else {
                e.picked.push((g, Pos::None));
            }
            return Ok(vec![]);
        }
        if let Some(section) = rest.strip_prefix("fold:") {
            let e = self.sketch_edit_mut().ok_or("no sketch")?;
            if !e.folded.remove(section) {
                e.folded.insert(section.to_owned());
            }
            return Ok(vec![]);
        }
        match rest {
            "close" => {
                self.leave_sketch();
                Ok(vec![])
            }
            "select-free" => {
                // Under constrained: select the geometry that can still move.
                let e = self.sketch_edit().ok_or("no sketch")?;
                let fixed = e.report.fully_constrained_geos.clone();
                let n = self.sketch().map(|s| s.geos.len()).unwrap_or(0) as i32;
                let e = self.sketch_edit_mut().unwrap();
                e.picked = (0..n)
                    .filter(|g| !fixed.contains(g))
                    .map(|g| (g, Pos::None))
                    .collect();
                Ok(vec![])
            }
            "fillet-radius" => {
                let r = self.sketch_edit().map(|e| e.fillet_radius).unwrap_or(2.0);
                self.field = Some(Field {
                    target: FieldTarget::Task {
                        name: "fillet_radius".into(),
                    },
                    text: format!("{} mm", math::fmt_num(r, 2)),
                    replace: true,
                });
                Ok(vec![])
            }
            "construction" => {
                let e = self.sketch_edit_mut().ok_or("no sketch")?;
                e.construction = !e.construction;
                Ok(vec![])
            }
            other => Err(format!("unknown sketch command {other}")),
        }
    }

    /// A press on sketch geometry starts a drag through the solver.
    pub(crate) fn sketch_grab(&self, x: f64, y: f64) -> Option<(i32, Pos)> {
        let e = self.sketch_edit()?;
        if e.tool.is_some() || e.pending.is_some() {
            return None;
        }
        let (_, snap) = self.sketch_point(x, y)?;
        snap.filter(|s| s.0 >= 0)
    }
    pub(crate) fn sketch_drag(&mut self, grab: (i32, Pos), x: f64, y: f64) {
        let Some((p, _)) = self.sketch_point(x, y) else {
            return;
        };
        let report = match self.sketch_mut() {
            Some(s) => s.drag(grab.0, grab.1, p),
            None => return,
        };
        if let Some(e) = self.sketch_edit_mut() {
            e.report = report;
        }
    }
}

/// The constraints a command makes from the selected sketch elements.
pub(crate) fn build_constraints(
    s: &Sketch,
    cmd: &str,
    picks: &[(i32, Pos)],
) -> Result<Vec<Constraint>, String> {
    let edges: Vec<i32> = picks
        .iter()
        .filter(|p| p.1 == Pos::None)
        .map(|p| p.0)
        .collect();
    let points: Vec<(i32, Pos)> = picks.iter().filter(|p| p.1 != Pos::None).copied().collect();
    let is_line = |g: i32| matches!(s.geom(g), Some(Geom::Line { .. }));
    let is_round = |g: i32| matches!(s.geom(g), Some(Geom::Circle { .. } | Geom::Arc { .. }));
    let dim = |kind: T, picks: &[(i32, Pos)]| tools::dimension(s, kind, picks);
    let out = match cmd {
        "Sketcher_ConstrainCoincident" => {
            if points.len() < 2 || !edges.is_empty() {
                return Err("Select two or more points".into());
            }
            points[1..]
                .iter()
                .map(|p| {
                    Constraint::new(T::Coincident, points[0].0, points[0].1).with_second(p.0, p.1)
                })
                .collect()
        }
        "Sketcher_ConstrainPointOnObject" => match (points.as_slice(), edges.as_slice()) {
            ([p], [e]) => {
                vec![Constraint::new(T::PointOnObject, p.0, p.1).with_second(*e, Pos::None)]
            }
            _ => return Err("Select one point and one edge".into()),
        },
        "Sketcher_ConstrainHorizontal" | "Sketcher_ConstrainVertical" => {
            let kind = if cmd.ends_with("Horizontal") {
                T::Horizontal
            } else {
                T::Vertical
            };
            if points.len() == 2 && edges.is_empty() {
                vec![Constraint::new(kind, points[0].0, points[0].1)
                    .with_second(points[1].0, points[1].1)]
            } else if !edges.is_empty()
                && points.is_empty()
                && edges.iter().all(|g| is_line(*g) && *g >= 0)
            {
                edges
                    .iter()
                    .map(|g| Constraint::new(kind, *g, Pos::None))
                    .collect()
            } else {
                return Err("Select one or more lines, or two points".into());
            }
        }
        "Sketcher_ConstrainParallel" => {
            if edges.len() < 2 || !points.is_empty() || !edges.iter().all(|g| is_line(*g)) {
                return Err("Select two or more lines".into());
            }
            edges[1..]
                .iter()
                .map(|g| {
                    Constraint::new(T::Parallel, edges[0], Pos::None).with_second(*g, Pos::None)
                })
                .collect()
        }
        "Sketcher_ConstrainPerpendicular" => match edges.as_slice() {
            [a, b] if points.is_empty() => {
                vec![Constraint::new(T::Perpendicular, *a, Pos::None).with_second(*b, Pos::None)]
            }
            _ => return Err("Select two lines, or a line and a circle".into()),
        },
        "Sketcher_ConstrainTangent" => {
            if let ([a, b], []) = (points.as_slice(), edges.as_slice()) {
                vec![Constraint::new(T::Tangent, a.0, a.1).with_second(b.0, b.1)]
            } else if let ([a, b], []) = (edges.as_slice(), points.as_slice()) {
                let mut c = Constraint::new(T::Tangent, *a, Pos::None).with_second(*b, Pos::None);
                if let (
                    Some(Geom::Circle { c: c1, r: r1 } | Geom::Arc { c: c1, r: r1, .. }),
                    Some(Geom::Circle { c: c2, r: r2 } | Geom::Arc { c: c2, r: r2, .. }),
                ) = (s.geom(*a), s.geom(*b))
                {
                    c.internal = c1.dist(c2) < r1.max(r2);
                }
                vec![c]
            } else {
                return Err("Select two edges, or two end points".into());
            }
        }
        "Sketcher_ConstrainEqual" => {
            let ok = edges.len() >= 2
                && points.is_empty()
                && (edges.iter().all(|g| is_line(*g)) || edges.iter().all(|g| is_round(*g)));
            if !ok {
                return Err("Select two or more lines, or two or more circles and arcs".into());
            }
            edges[1..]
                .iter()
                .map(|g| Constraint::new(T::Equal, edges[0], Pos::None).with_second(*g, Pos::None))
                .collect()
        }
        "Sketcher_ConstrainSymmetric" => match (points.as_slice(), edges.as_slice()) {
            ([a, b], [l]) if is_line(*l) => vec![Constraint::new(T::Symmetric, a.0, a.1)
                .with_second(b.0, b.1)
                .with_third(*l, Pos::None)],
            ([a, b, m], []) => vec![Constraint::new(T::Symmetric, a.0, a.1)
                .with_second(b.0, b.1)
                .with_third(m.0, m.1)],
            ([], [l]) if is_line(*l) && *l >= 0 => {
                // A line alone: its end points symmetric about... FreeCAD needs a
                // second reference; ask for it.
                return Err(
                    "Select two points and a symmetry line, or two points and a symmetry point"
                        .into(),
                );
            }
            _ => {
                return Err(
                    "Select two points and a symmetry line, or two points and a symmetry point"
                        .into(),
                )
            }
        },
        "Sketcher_ConstrainBlock" => {
            if edges.is_empty() || !points.is_empty() || edges.iter().any(|g| *g < 0) {
                return Err("Select the geometry to block".into());
            }
            edges
                .iter()
                .map(|g| Constraint::new(T::Block, *g, Pos::None))
                .collect()
        }
        "Sketcher_ConstrainLock" => match (points.as_slice(), edges.as_slice()) {
            ([p], []) if p.0 >= 0 => {
                let q = s.point(p.0, p.1).ok_or("bad point")?;
                vec![
                    Constraint::new(T::DistanceX, p.0, p.1).with_value(q.x),
                    Constraint::new(T::DistanceY, p.0, p.1).with_value(q.y),
                ]
            }
            _ => return Err("Select one point to lock".into()),
        },
        "Sketcher_ConstrainDistanceX" => vec![dim(T::DistanceX, picks).map_err(select_err)?],
        "Sketcher_ConstrainDistanceY" => vec![dim(T::DistanceY, picks).map_err(select_err)?],
        "Sketcher_ConstrainDistance" => vec![dim(T::Distance, picks).map_err(select_err)?],
        "Sketcher_ConstrainRadius" | "Sketcher_ConstrainDiameter" => {
            let kind = if cmd.ends_with("Radius") {
                T::Radius
            } else {
                T::Diameter
            };
            if edges.is_empty() || !points.is_empty() || !edges.iter().all(|g| is_round(*g)) {
                return Err("Select one or more circles or arcs".into());
            }
            let mut out = Vec::new();
            for g in edges {
                out.push(dim(kind, &[(g, Pos::None)])?);
            }
            out
        }
        "Sketcher_ConstrainAngle" => {
            let valid = !edges.is_empty() && edges.len() <= 2 && points.is_empty();
            if !valid {
                return Err("Select one line, two lines, or an arc".into());
            }
            let pairs: Vec<(i32, Pos)> = edges.iter().map(|g| (*g, Pos::None)).collect();
            vec![dim(T::Angle, &pairs).map_err(select_err)?]
        }
        other => return Err(format!("unknown constraint command {other}")),
    };
    for c in &out {
        if c.first == GEO_UNDEF {
            return Err("Select geometry for the constraint".into());
        }
    }
    Ok(out)
}
fn select_err(e: String) -> String {
    if e.starts_with("select") {
        let mut s = e;
        s.replace_range(0..1, "S");
        s
    } else {
        e
    }
}

/// The distance and direction of a sketch point from the view centre in pixels, for
/// the overlay drawing: world → screen through the camera.
pub fn to_screen(
    cam: &cw_cad::view::Camera,
    frame: &Frame,
    p: V2,
    w: u32,
    h: u32,
) -> Option<(f64, f64)> {
    cam.project(frame.to_world(p), w, h).map(|(x, y, _)| (x, y))
}
