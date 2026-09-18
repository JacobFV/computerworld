//! FreeCAD's commands (by their own `Std_…`, `PartDesign_…`, `Sketcher_…` names), the
//! menus and toolbars that show them, and keyboard and text routing.
use super::sketcher::Tool;
use super::*;
use crate::AppEffect;

/// One command: id, menu text, shortcut.
pub struct Cmd {
    pub id: &'static str,
    pub label: &'static str,
    pub keys: &'static str,
}
const fn c(id: &'static str, label: &'static str, keys: &'static str) -> Cmd {
    Cmd { id, label, keys }
}

pub const COMMANDS: &[Cmd] = &[
    c("Std_New", "New", "Ctrl+N"),
    c("Std_Open", "Open...", "Ctrl+O"),
    c("Std_Save", "Save", "Ctrl+S"),
    c("Std_SaveAs", "Save As...", "Ctrl+Shift+S"),
    c("Std_Import", "Import...", "Ctrl+I"),
    c("Std_Export", "Export...", "Ctrl+E"),
    c("Std_Undo", "Undo", "Ctrl+Z"),
    c("Std_Redo", "Redo", "Ctrl+Y"),
    c("Std_Delete", "Delete", "Del"),
    c("Std_Refresh", "Recompute", "Ctrl+R"),
    c("Std_SelectAll", "Select All", "Ctrl+A"),
    c("Std_ViewFitAll", "Fit all", "V, F"),
    c("Std_ViewFitSelection", "Fit selection", "V, S"),
    c("Std_ViewIsometric", "Isometric", "0"),
    c("Std_ViewFront", "Front", "1"),
    c("Std_ViewTop", "Top", "2"),
    c("Std_ViewRight", "Right", "3"),
    c("Std_ViewRear", "Rear", "4"),
    c("Std_ViewBottom", "Bottom", "5"),
    c("Std_ViewLeft", "Left", "6"),
    c("Std_OrthographicCamera", "Orthographic view", "V, O"),
    c("Std_PerspectiveCamera", "Perspective view", "V, P"),
    c("Std_ToggleVisibility", "Toggle visibility", "Space"),
    c("Std_SelBoundingBox", "Bounding box", ""),
    c("Std_AxisCross", "Toggle axis cross", "A, C"),
    c("Std_ReportView", "Report view", ""),
    c("Std_Measure", "Measure", ""),
    c("Std_About", "About FreeCAD", ""),
    c("PartDesign_Body", "Create body", ""),
    c("PartDesign_NewSketch", "Create sketch", ""),
    c("PartDesign_Pad", "Pad", ""),
    c("PartDesign_Revolution", "Revolution", ""),
    c("PartDesign_Pocket", "Pocket", ""),
    c("PartDesign_Hole", "Hole", ""),
    c("PartDesign_Groove", "Groove", ""),
    c("PartDesign_Fillet", "Fillet", ""),
    c("PartDesign_Chamfer", "Chamfer", ""),
    c("PartDesign_Mirrored", "Mirror", ""),
    c("PartDesign_LinearPattern", "Linear Pattern", ""),
    c("PartDesign_PolarPattern", "Polar Pattern", ""),
    c("PartDesign_MoveTip", "Set tip", ""),
    c("Sketcher_EditSketch", "Edit sketch", ""),
    c("Sketcher_LeaveSketch", "Leave sketch", "Esc"),
    c("Sketcher_ViewSketch", "View sketch", "Q, P"),
    c("Sketcher_CreatePoint", "Create point", "G, Y"),
    c("Sketcher_CreateLine", "Create line", "G, L"),
    c("Sketcher_CreateArc", "Create arc by center", "G, A"),
    c(
        "Sketcher_Create3PointArc",
        "Create arc by 3 points",
        "G, 3, A",
    ),
    c("Sketcher_CreateCircle", "Create circle by center", "G, C"),
    c(
        "Sketcher_Create3PointCircle",
        "Create circle by 3 points",
        "G, 3, C",
    ),
    c("Sketcher_CreatePolyline", "Create polyline", "G, M"),
    c("Sketcher_CreateRectangle", "Create rectangle", "G, R"),
    c("Sketcher_CreateSlot", "Create slot", "G, S"),
    c("Sketcher_CreateFillet", "Create fillet", "G, F, F"),
    c("Sketcher_Trimming", "Trim edge", "G, T"),
    c("Sketcher_Extend", "Extend edge", "G, Q"),
    c(
        "Sketcher_ToggleConstruction",
        "Toggle construction geometry",
        "G, N",
    ),
    c("Sketcher_ConstrainCoincident", "Constrain coincident", "C"),
    c(
        "Sketcher_ConstrainPointOnObject",
        "Constrain point on object",
        "O",
    ),
    c("Sketcher_ConstrainHorizontal", "Constrain horizontal", "H"),
    c("Sketcher_ConstrainVertical", "Constrain vertical", "V"),
    c("Sketcher_ConstrainParallel", "Constrain parallel", "P"),
    c(
        "Sketcher_ConstrainPerpendicular",
        "Constrain perpendicular",
        "N",
    ),
    c("Sketcher_ConstrainTangent", "Constrain tangent", "T"),
    c("Sketcher_ConstrainEqual", "Constrain equal", "E"),
    c("Sketcher_ConstrainSymmetric", "Constrain symmetric", "S"),
    c("Sketcher_ConstrainBlock", "Constrain block", "K, B"),
    c("Sketcher_ConstrainLock", "Constrain lock", "K, L"),
    c(
        "Sketcher_ConstrainDistanceX",
        "Constrain horizontal distance",
        "L",
    ),
    c(
        "Sketcher_ConstrainDistanceY",
        "Constrain vertical distance",
        "I",
    ),
    c("Sketcher_ConstrainDistance", "Constrain distance", "K, D"),
    c("Sketcher_ConstrainRadius", "Constrain radius", "K, R"),
    c("Sketcher_ConstrainDiameter", "Constrain diameter", "K, O"),
    c("Sketcher_ConstrainAngle", "Constrain angle", "K, A"),
    c(
        "Sketcher_ToggleDrivingConstraint",
        "Toggle driving/reference constraint",
        "K, X",
    ),
    c(
        "Sketcher_SelectConflictingConstraints",
        "Select conflicting constraints",
        "",
    ),
];
pub fn command(id: &str) -> Option<&'static Cmd> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// The menu bar, in FreeCAD's order: (menu, items). `-` is a separator.
pub const MENUS: &[(&str, &[&str])] = &[
    (
        "File",
        &[
            "Std_New",
            "Std_Open",
            "-",
            "Std_Save",
            "Std_SaveAs",
            "-",
            "Std_Import",
            "Std_Export",
        ],
    ),
    (
        "Edit",
        &[
            "Std_Undo",
            "Std_Redo",
            "-",
            "Std_Delete",
            "Std_SelectAll",
            "-",
            "Std_Refresh",
        ],
    ),
    (
        "View",
        &[
            "Std_ViewFitAll",
            "Std_ViewFitSelection",
            "-",
            "Std_ViewIsometric",
            "Std_ViewFront",
            "Std_ViewTop",
            "Std_ViewRight",
            "Std_ViewRear",
            "Std_ViewBottom",
            "Std_ViewLeft",
            "-",
            "Std_OrthographicCamera",
            "Std_PerspectiveCamera",
            "-",
            "Std_ToggleVisibility",
            "Std_SelBoundingBox",
            "Std_AxisCross",
            "-",
            "Std_ReportView",
        ],
    ),
    ("Tools", &["Std_Measure"]),
    (
        "Part Design",
        &[
            "PartDesign_Body",
            "PartDesign_NewSketch",
            "-",
            "PartDesign_Pad",
            "PartDesign_Revolution",
            "-",
            "PartDesign_Pocket",
            "PartDesign_Hole",
            "PartDesign_Groove",
            "-",
            "PartDesign_Fillet",
            "PartDesign_Chamfer",
            "-",
            "PartDesign_Mirrored",
            "PartDesign_LinearPattern",
            "PartDesign_PolarPattern",
            "-",
            "PartDesign_MoveTip",
        ],
    ),
    (
        "Sketch",
        &[
            "Sketcher_EditSketch",
            "Sketcher_LeaveSketch",
            "Sketcher_ViewSketch",
            "-",
            "Sketcher_CreatePoint",
            "Sketcher_CreateLine",
            "Sketcher_CreateArc",
            "Sketcher_Create3PointArc",
            "Sketcher_CreateCircle",
            "Sketcher_Create3PointCircle",
            "Sketcher_CreatePolyline",
            "Sketcher_CreateRectangle",
            "Sketcher_CreateSlot",
            "-",
            "Sketcher_CreateFillet",
            "Sketcher_Trimming",
            "Sketcher_Extend",
            "Sketcher_ToggleConstruction",
            "-",
            "Sketcher_ConstrainCoincident",
            "Sketcher_ConstrainPointOnObject",
            "Sketcher_ConstrainHorizontal",
            "Sketcher_ConstrainVertical",
            "Sketcher_ConstrainParallel",
            "Sketcher_ConstrainPerpendicular",
            "Sketcher_ConstrainTangent",
            "Sketcher_ConstrainEqual",
            "Sketcher_ConstrainSymmetric",
            "Sketcher_ConstrainBlock",
            "Sketcher_ConstrainLock",
            "Sketcher_ConstrainDistanceX",
            "Sketcher_ConstrainDistanceY",
            "Sketcher_ConstrainDistance",
            "Sketcher_ConstrainRadius",
            "Sketcher_ConstrainDiameter",
            "Sketcher_ConstrainAngle",
            "-",
            "Sketcher_ToggleDrivingConstraint",
            "Sketcher_SelectConflictingConstraints",
        ],
    ),
    ("Help", &["Std_About"]),
];

/// The toolbars on screen, left to right, `|` between toolbars.
pub fn toolbar(sketching: bool) -> Vec<&'static str> {
    let mut t = vec![
        "Std_New",
        "Std_Open",
        "Std_Save",
        "|",
        "Std_Undo",
        "Std_Redo",
        "|",
        "workbench",
        "|",
    ];
    if sketching {
        t.extend([
            "Sketcher_LeaveSketch",
            "Sketcher_ViewSketch",
            "|",
            "Sketcher_CreatePoint",
            "Sketcher_CreateLine",
            "Sketcher_CreateArc",
            "Sketcher_Create3PointArc",
            "Sketcher_CreateCircle",
            "Sketcher_Create3PointCircle",
            "Sketcher_CreatePolyline",
            "Sketcher_CreateRectangle",
            "Sketcher_CreateSlot",
            "|",
            "Sketcher_CreateFillet",
            "Sketcher_Trimming",
            "Sketcher_Extend",
            "Sketcher_ToggleConstruction",
            "|",
            "Sketcher_ConstrainCoincident",
            "Sketcher_ConstrainPointOnObject",
            "Sketcher_ConstrainHorizontal",
            "Sketcher_ConstrainVertical",
            "Sketcher_ConstrainParallel",
            "Sketcher_ConstrainPerpendicular",
            "Sketcher_ConstrainTangent",
            "Sketcher_ConstrainEqual",
            "Sketcher_ConstrainSymmetric",
            "Sketcher_ConstrainBlock",
            "Sketcher_ConstrainLock",
            "|",
            "Sketcher_ConstrainDistanceX",
            "Sketcher_ConstrainDistanceY",
            "Sketcher_ConstrainDistance",
            "Sketcher_ConstrainRadius",
            "Sketcher_ConstrainDiameter",
            "Sketcher_ConstrainAngle",
            "Sketcher_ToggleDrivingConstraint",
        ]);
    } else {
        t.extend([
            "Std_ViewFitAll",
            "Std_ViewIsometric",
            "Std_ViewFront",
            "Std_ViewTop",
            "Std_ViewRight",
            "Std_ViewRear",
            "Std_ViewBottom",
            "Std_ViewLeft",
            "|",
            "Std_Measure",
            "|",
            "PartDesign_Body",
            "PartDesign_NewSketch",
            "|",
            "PartDesign_Pad",
            "PartDesign_Revolution",
            "|",
            "PartDesign_Pocket",
            "PartDesign_Hole",
            "PartDesign_Groove",
            "|",
            "PartDesign_Fillet",
            "PartDesign_Chamfer",
            "|",
            "PartDesign_Mirrored",
            "PartDesign_LinearPattern",
            "PartDesign_PolarPattern",
        ]);
    }
    t
}

impl Cad {
    /// Why a command cannot run right now, or `Ok` when it can.
    pub fn available(&self, id: &str) -> Result<(), String> {
        let sketching = self.sketch_edit().is_some();
        let busy = self.task.is_some() && !sketching;
        let need_body = || {
            self.body()
                .ok_or_else(|| "There is no body; create one first".to_owned())
        };
        let sel_sketch = || -> Option<String> {
            self.selection.iter().map(|s| s.object.clone()).find(|o| {
                matches!(
                    self.doc.get(o).map(|x| &x.feature),
                    Some(Feature::Sketch { .. })
                )
            })
        };
        match id {
            "Std_Undo" => {
                if self.undo.is_empty() {
                    return Err("Nothing to undo".into());
                }
            }
            "Std_Redo" => {
                if self.redo.is_empty() {
                    return Err("Nothing to redo".into());
                }
            }
            "Std_Delete" => {
                if sketching {
                    let s = self.sketch_edit().unwrap();
                    if s.picked.is_empty() && s.constraints.is_empty() {
                        return Err("Select sketch geometry or constraints to delete".into());
                    }
                } else if self.selection.is_empty() {
                    return Err("Select an object to delete".into());
                }
            }
            "Std_ViewFitSelection" | "Std_ToggleVisibility" => {
                if self.selection.is_empty() {
                    return Err("Select an object first".into());
                }
            }
            "Std_OrthographicCamera" => {
                if !self.camera.perspective {
                    return Err("The view is already orthographic".into());
                }
            }
            "Std_PerspectiveCamera" => {
                if self.camera.perspective {
                    return Err("The view is already perspective".into());
                }
            }
            "Std_Measure" => {
                if self.task.is_some() {
                    return Err("Close the open task first".into());
                }
            }
            "Std_Export" => {
                if self.exportable().is_empty() {
                    return Err("There is nothing to export".into());
                }
            }
            "Std_New" | "Std_Open" | "Std_Import" | "PartDesign_Body" => {
                if self.task.is_some() {
                    return Err("Close the open task first".into());
                }
            }
            "PartDesign_NewSketch" => {
                if self.task.is_some() {
                    return Err("Close the open task first".into());
                }
                need_body()?;
            }
            "PartDesign_Pad"
            | "PartDesign_Revolution"
            | "PartDesign_Pocket"
            | "PartDesign_Hole"
            | "PartDesign_Groove" => {
                if self.task.is_some() {
                    return Err("Close the open task first".into());
                }
                let body = need_body()?;
                self.profile_for(&body)?;
                let solid = self
                    .body()
                    .and_then(|b| self.model().body_shape.get(&b).cloned())
                    .is_some();
                if matches!(
                    id,
                    "PartDesign_Pocket" | "PartDesign_Hole" | "PartDesign_Groove"
                ) && !solid
                {
                    return Err("There is no solid to cut; pad a sketch first".into());
                }
            }
            "PartDesign_Fillet" | "PartDesign_Chamfer" => {
                if self.task.is_some() {
                    return Err("Close the open task first".into());
                }
                need_body()?;
                if !self.selection.iter().any(|s| s.sub.starts_with("Edge")) {
                    return Err("Select one or more edges of the body first".into());
                }
            }
            "PartDesign_Mirrored" | "PartDesign_LinearPattern" | "PartDesign_PolarPattern" => {
                if self.task.is_some() {
                    return Err("Close the open task first".into());
                }
                if self.pattern_originals().is_empty() {
                    return Err(
                        "Select a pad, pocket, revolution, groove or hole to transform".into(),
                    );
                }
            }
            "PartDesign_MoveTip" => {
                if busy || sketching {
                    return Err("Close the open task first".into());
                }
                let ok = self
                    .selected_object()
                    .and_then(|o| self.doc.get(o))
                    .is_some_and(|o| o.feature.is_solid_feature());
                if !ok {
                    return Err("Select a feature of the body".into());
                }
            }
            "Sketcher_EditSketch" => {
                if self.task.is_some() {
                    return Err("Close the open task first".into());
                }
                if sel_sketch().is_none() {
                    return Err("Select a sketch".into());
                }
            }
            "Sketcher_LeaveSketch" | "Sketcher_ViewSketch" | "Sketcher_ToggleConstruction" => {
                if !sketching {
                    return Err("Open a sketch for editing first".into());
                }
            }
            "Sketcher_SelectConflictingConstraints" => {
                let s = self
                    .sketch_edit()
                    .ok_or("Open a sketch for editing first")?;
                if s.report.conflicting.is_empty() && s.report.redundant.is_empty() {
                    return Err("The sketch has no conflicting or redundant constraints".into());
                }
            }
            "Sketcher_ToggleDrivingConstraint" => {
                let s = self
                    .sketch_edit()
                    .ok_or("Open a sketch for editing first")?;
                if s.constraints.is_empty() {
                    return Err("Select a dimensional constraint first".into());
                }
            }
            other if other.starts_with("Sketcher_") && !sketching => {
                return Err("Open a sketch for editing first".into());
            }
            _ => {}
        }
        Ok(())
    }

    /// Everything that can be clicked, pressed or typed into, routed by target.
    pub fn command(
        &mut self,
        window: u64,
        cmd: &str,
        at: Option<(i32, i32)>,
    ) -> Result<Vec<AppEffect>, String> {
        // Any click outside an open menu closes it first, like a real pop-up.
        let menu_click =
            cmd.starts_with("menu") || cmd.starts_with("cmd:") || cmd.starts_with("overflow");
        if !menu_click
            && cmd != "nav-menu"
            && !cmd.starts_with("nav:")
            && !cmd.starts_with("choice:")
        {
            self.menu = None;
        }
        if let Some(rest) = cmd.strip_prefix("cmd:") {
            self.menu = None;
            return self.run(window, rest);
        }
        if let Some(name) = cmd.strip_prefix("menu:") {
            self.menu = if self.menu.as_deref() == Some(name) {
                None
            } else {
                Some(name.to_owned())
            };
            return Ok(vec![]);
        }
        match cmd {
            "menu-close" => {
                self.menu = None;
                return Ok(vec![]);
            }
            "tab:model" => {
                self.combo = ComboTab::Model;
                return Ok(vec![]);
            }
            "tab:tasks" => {
                self.combo = ComboTab::Tasks;
                return Ok(vec![]);
            }
            "prop-tab:view" => {
                self.props = PropTab::View;
                return Ok(vec![]);
            }
            "prop-tab:data" => {
                self.props = PropTab::Data;
                return Ok(vec![]);
            }
            "report-close" => {
                self.show_report = false;
                return Ok(vec![]);
            }
            "report-clear" => {
                self.report.clear();
                return Ok(vec![]);
            }
            "nav-menu" => {
                self.menu = if self.menu.as_deref() == Some("nav") {
                    None
                } else {
                    Some("nav".into())
                };
                return Ok(vec![]);
            }
            "workbench" => {
                self.menu = if self.menu.as_deref() == Some("workbench") {
                    None
                } else {
                    Some("workbench".into())
                };
                return Ok(vec![]);
            }
            "overflow" => {
                self.menu = if self.menu.as_deref() == Some("overflow") {
                    None
                } else {
                    Some("overflow".into())
                };
                return Ok(vec![]);
            }
            "view" => return self.view_click(window, at.unwrap_or((0, 0))),
            "navcube" => {
                return self.navcube_click(at.ok_or("the navigation cube needs a position")?)
            }
            "navcube-menu" => {
                self.menu = if self.menu.as_deref() == Some("navcube") {
                    None
                } else {
                    Some("navcube".into())
                };
                return Ok(vec![]);
            }
            "field-cancel" => {
                self.field = None;
                return Ok(vec![]);
            }
            _ => {}
        }
        if let Some(style) = cmd.strip_prefix("nav:") {
            self.nav = NavStyle::ALL
                .into_iter()
                .find(|n| n.label() == style)
                .ok_or("unknown navigation style")?;
            self.menu = None;
            self.status = format!("Navigation style: {style}");
            return Ok(vec![]);
        }
        if let Some(wb) = cmd.strip_prefix("wb:") {
            self.menu = None;
            return match wb {
                "Part Design" => {
                    if self.sketch_edit().is_some() {
                        self.leave_sketch();
                    }
                    Ok(vec![])
                }
                "Sketcher" => {
                    if self.sketch_edit().is_none() {
                        // The Sketcher needs a sketch to work on: open the selected one, or
                        // start a new one in the active body.
                        if self.available("Sketcher_EditSketch").is_ok() {
                            return self.run(window, "Sketcher_EditSketch");
                        }
                        return self.run(window, "PartDesign_NewSketch");
                    }
                    Ok(vec![])
                }
                _ => Err("unknown workbench".into()),
            };
        }
        if let Some(dir) = cmd.strip_prefix("navcube-arrow:") {
            return self.navcube_arrow(dir);
        }
        if let Some(rest) = cmd.strip_prefix("navcube-view:") {
            self.menu = None;
            return self.run(window, rest);
        }
        if let Some(rest) = cmd.strip_prefix("tree:") {
            return self.tree_click(rest);
        }
        if let Some(rest) = cmd.strip_prefix("tree-toggle:") {
            if !self.expanded.remove(rest) {
                self.expanded.insert(rest.to_owned());
            }
            return Ok(vec![]);
        }
        if let Some(rest) = cmd.strip_prefix("tree-eye:") {
            let o = self.doc.get_mut(rest).ok_or("no such object")?;
            o.visible = !o.visible;
            self.rev += 1;
            return Ok(vec![]);
        }
        if let Some(rest) = cmd.strip_prefix("prop:") {
            return self.property_click(rest);
        }
        if let Some(rest) = cmd.strip_prefix("choice:") {
            return self.choice(window, rest);
        }
        if let Some(rest) = cmd.strip_prefix("task:") {
            return self.task_command(window, rest);
        }
        if let Some(rest) = cmd.strip_prefix("sk:") {
            return self.sketch_command(rest);
        }
        if let Some(rest) = cmd.strip_prefix("dialog:") {
            return self.dialog_command(window, rest);
        }
        if let Some(rest) = cmd.strip_prefix("file:") {
            return self.file_command(window, rest);
        }
        if let Some(rest) = cmd.strip_prefix("field:") {
            return self.focus_field(rest);
        }
        Err(format!("unknown FreeCAD command {cmd}"))
    }

    /// Double clicks: edit what was double-clicked.
    pub fn double_click(&mut self, window: u64, cmd: &str) -> Result<Vec<AppEffect>, String> {
        // In a file dialog a double click opens a folder or chooses a file.
        if let Some(entry) = cmd.strip_prefix("file:entry:") {
            return self.file_command(window, &format!("open:{entry}"));
        }
        if let Some(name) = cmd.strip_prefix("tree:") {
            self.tree_click(name)?;
            return self.edit_object(name);
        }
        if let Some(i) = cmd
            .strip_prefix("sk:constraint:")
            .or_else(|| cmd.strip_prefix("sk:dim:"))
        {
            let i: usize = i.parse().map_err(|_| "bad constraint")?;
            return self.edit_constraint_value(i);
        }
        self.command(window, cmd, None)
    }

    /// Open an object for editing: a sketch in the Sketcher, a feature's task panel.
    pub fn edit_object(&mut self, name: &str) -> Result<Vec<AppEffect>, String> {
        if self.task.is_some() {
            return Err("Close the open task first".into());
        }
        match self.doc.get(name).map(|o| o.feature.clone()) {
            Some(Feature::Sketch { .. }) => {
                self.open_sketch(name)?;
                Ok(vec![])
            }
            Some(Feature::Body { .. }) => {
                // Double-clicking a body makes it the active one.
                self.active_body = Some(name.to_owned());
                self.status = format!("{} is the active body", self.label_of(name));
                Ok(vec![])
            }
            Some(f) if f.is_solid_feature() => {
                self.open_feature_task(name);
                Ok(vec![])
            }
            Some(_) => Err("This object has no editing mode".into()),
            None => Err("no such object".into()),
        }
    }

    fn tree_click(&mut self, name: &str) -> Result<Vec<AppEffect>, String> {
        if name == "document" {
            self.selection.clear();
            return Ok(vec![]);
        }
        if let Some(plane) = name.strip_prefix("origin:") {
            // Base planes can be picked for a new sketch while choosing one.
            if let Some(Task::PickPlane { plane: p, .. }) = &mut self.task {
                *p = plane.to_owned();
            }
            self.status = format!("{plane} (Origin feature)");
            return Ok(vec![]);
        }
        if name.ends_with(":Origin") {
            // The Origin group itself: a click opens or closes it.
            if !self.expanded.remove(name) {
                self.expanded.insert(name.to_owned());
            }
            return Ok(vec![]);
        }
        if self.doc.get(name).is_none() {
            return Err("no such object".into());
        }
        // While choosing originals for a pattern or mirror, a tree click toggles one.
        if let Some(Task::Feature(fe)) = &self.task {
            let feature = fe.name.clone();
            if self.toggle_original(&feature, name)? {
                return Ok(vec![]);
            }
        }
        self.selection = vec![Sel {
            object: name.to_owned(),
            sub: String::new(),
            point: V3::ZERO,
        }];
        self.status = format!("Selected {}", self.label_of(name));
        Ok(vec![])
    }

    /// Run a command by its FreeCAD name.
    pub fn run(&mut self, window: u64, id: &str) -> Result<Vec<AppEffect>, String> {
        self.available(id)?;
        self.field = None;
        match id {
            "Std_New" => return self.guarded(window, "new"),
            "Std_Open" => return self.guarded(window, "open"),
            "Std_Save" => return self.save(window),
            "Std_SaveAs" => return Ok(self.show_file_dialog(window, files::Purpose::SaveAs)),
            "Std_Import" => return Ok(self.show_file_dialog(window, files::Purpose::Import)),
            "Std_Export" => return Ok(self.show_file_dialog(window, files::Purpose::Export)),
            "Std_Undo" => self.undo()?,
            "Std_Redo" => self.redo()?,
            "Std_Delete" => self.delete_selection()?,
            "Std_Refresh" => {
                self.recompute();
                self.status = "Recomputed the document".into();
            }
            "Std_SelectAll" => {
                if self.sketch_edit().is_some() {
                    let count = self.sketch().map(|s| s.geos.len()).unwrap_or(0);
                    if let Some(s) = self.sketch_edit_mut() {
                        s.picked = (0..count as i32).map(|g| (g, Pos::None)).collect();
                    }
                } else {
                    self.selection = self
                        .doc
                        .objects
                        .iter()
                        .map(|o| Sel {
                            object: o.name.clone(),
                            sub: String::new(),
                            point: V3::ZERO,
                        })
                        .collect();
                }
            }
            "Std_ViewFitAll" => self.fit_all(),
            "Std_ViewFitSelection" => self.fit_selection(),
            "Std_ViewIsometric" => self.set_view(StdView::Isometric),
            "Std_ViewFront" => self.set_view(StdView::Front),
            "Std_ViewTop" => self.set_view(StdView::Top),
            "Std_ViewRight" => self.set_view(StdView::Right),
            "Std_ViewRear" => self.set_view(StdView::Rear),
            "Std_ViewBottom" => self.set_view(StdView::Bottom),
            "Std_ViewLeft" => self.set_view(StdView::Left),
            "Std_OrthographicCamera" => self.camera.perspective = false,
            "Std_PerspectiveCamera" => self.camera.perspective = true,
            "Std_ToggleVisibility" => {
                let names: Vec<String> = self.selection.iter().map(|s| s.object.clone()).collect();
                for n in names {
                    if let Some(o) = self.doc.get_mut(&n) {
                        o.visible = !o.visible;
                    }
                }
                self.rev += 1;
            }
            "Std_SelBoundingBox" => self.bbox = !self.bbox,
            "Std_AxisCross" => self.axis_cross = !self.axis_cross,
            "Std_ReportView" => self.show_report = !self.show_report,
            "Std_Measure" => {
                self.task = Some(Task::Measure);
                self.combo = ComboTab::Tasks;
            }
            "Std_About" => self.dialog = Some(Dialog::About),
            "PartDesign_Body" => {
                self.checkpoint("Create body");
                let name = self.doc.add(Feature::Body {
                    group: vec![],
                    tip: None,
                });
                self.active_body = Some(name.clone());
                self.expanded.insert(name.clone());
                self.recompute();
                self.selection = vec![Sel {
                    object: name.clone(),
                    sub: String::new(),
                    point: V3::ZERO,
                }];
                self.log(ReportKind::Log, &format!("Created {name}"));
            }
            "PartDesign_NewSketch" => self.new_sketch()?,
            "PartDesign_Pad"
            | "PartDesign_Pocket"
            | "PartDesign_Revolution"
            | "PartDesign_Groove"
            | "PartDesign_Hole" => self.new_sketch_feature(id)?,
            "PartDesign_Fillet" | "PartDesign_Chamfer" => self.new_dressup(id)?,
            "PartDesign_Mirrored" | "PartDesign_LinearPattern" | "PartDesign_PolarPattern" => {
                self.new_transform(id)?
            }
            "PartDesign_MoveTip" => self.move_tip()?,
            "Sketcher_EditSketch" => {
                let name = self
                    .selection
                    .iter()
                    .map(|s| s.object.clone())
                    .find(|o| {
                        matches!(
                            self.doc.get(o).map(|x| &x.feature),
                            Some(Feature::Sketch { .. })
                        )
                    })
                    .ok_or("Select a sketch")?;
                self.open_sketch(&name)?;
            }
            "Sketcher_LeaveSketch" => self.leave_sketch(),
            "Sketcher_ViewSketch" => self.view_sketch(),
            "Sketcher_ToggleConstruction" => self.toggle_construction()?,
            "Sketcher_ToggleDrivingConstraint" => self.toggle_driving()?,
            "Sketcher_SelectConflictingConstraints" => {
                if let Some(s) = self.sketch_edit_mut() {
                    let mut all: Vec<usize> = s
                        .report
                        .conflicting
                        .iter()
                        .chain(&s.report.redundant)
                        .map(|i| i - 1)
                        .collect();
                    all.sort_unstable();
                    all.dedup();
                    s.constraints = all;
                }
            }
            other => {
                if let Some(tool) = Tool::from_command(other) {
                    self.start_tool(tool)?;
                } else if other.starts_with("Sketcher_Constrain") {
                    self.constrain(other)?;
                } else {
                    return Err(format!("unknown command {other}"));
                }
            }
        }
        Ok(vec![])
    }

    pub(crate) fn undo(&mut self) -> Result<(), String> {
        let (label, doc) = self.undo.pop().ok_or("Nothing to undo")?;
        let current = std::mem::replace(&mut self.doc, doc);
        self.redo.push((label.clone(), current));
        self.after_history();
        self.status = format!("Undo {label}");
        Ok(())
    }
    pub(crate) fn redo(&mut self) -> Result<(), String> {
        let (label, doc) = self.redo.pop().ok_or("Nothing to redo")?;
        let current = std::mem::replace(&mut self.doc, doc);
        self.undo.push((label.clone(), current));
        self.after_history();
        self.status = format!("Redo {label}");
        Ok(())
    }
    /// After the document jumps, drop state that pointed into the old one.
    fn after_history(&mut self) {
        self.modified = true;
        self.selection.retain(|s| self.doc.get(&s.object).is_some());
        let task_ok = match &self.task {
            Some(Task::Sketch(s)) => self.doc.get(&s.name).is_some(),
            Some(Task::Feature(f)) => self.doc.get(&f.name).is_some(),
            _ => true,
        };
        if !task_ok {
            self.task = None;
            self.combo = ComboTab::Model;
        }
        if self.sketch_edit().is_some() {
            let geos = self.sketch().map(|s| s.geos.len()).unwrap_or(0);
            let cons = self.sketch().map(|s| s.constraints.len()).unwrap_or(0);
            if let Some(s) = self.sketch_edit_mut() {
                s.picked.retain(|(g, _)| *g < geos as i32);
                s.constraints.retain(|c| *c < cons);
                s.clicks.clear();
                s.snaps.clear();
            }
        }
        if self
            .active_body
            .as_ref()
            .is_some_and(|b| self.doc.get(b).is_none())
        {
            self.active_body = self.doc.bodies().first().map(|b| (*b).to_owned());
        }
        self.recompute();
        self.refresh_sketch_report();
    }

    fn delete_selection(&mut self) -> Result<(), String> {
        if self.sketch_edit().is_some() {
            return self.sketch_delete();
        }
        let names: Vec<String> = self.selection.iter().map(|s| s.object.clone()).collect();
        let mut names: Vec<String> = names
            .into_iter()
            .filter(|n| self.doc.get(n).is_some())
            .collect();
        names.dedup();
        if names.is_empty() {
            return Err("Select an object to delete".into());
        }
        // Deleting a body takes its features with it.
        let mut all = names.clone();
        for n in &names {
            if let Some(Feature::Body { group, .. }) = self.doc.get(n).map(|o| &o.feature) {
                all.extend(group.iter().cloned());
            }
        }
        self.checkpoint("Delete");
        self.doc.remove(&all);
        self.selection.clear();
        self.recompute();
        self.log(ReportKind::Log, &format!("Deleted {}", names.join(", ")));
        Ok(())
    }

    fn move_tip(&mut self) -> Result<(), String> {
        let name = self.selected_object().ok_or("Select a feature")?.to_owned();
        let body = self
            .doc
            .body_of(&name)
            .ok_or("The feature is not in a body")?
            .to_owned();
        self.checkpoint("Set tip");
        if let Some(Object {
            feature: Feature::Body { tip, .. },
            ..
        }) = self.doc.get_mut(&body)
        {
            *tip = Some(name.clone());
        }
        self.recompute();
        self.status = format!(
            "{} is now the tip of {}",
            self.label_of(&name),
            self.label_of(&body)
        );
        Ok(())
    }

    pub fn key(&mut self, window: u64, key: &str) -> Result<Vec<AppEffect>, String> {
        let key = key.replace("Meta+", "Ctrl+");
        // An open dialog takes the keyboard first.
        if self.field.is_some() {
            match key.as_str() {
                "Enter" | "Tab" => return self.commit_field(window),
                "Escape" if matches!(self.dialog, Some(Dialog::File(_))) => {
                    // Escape answers the file dialog (or its New Folder prompt), not
                    // just the box that has the keyboard.
                    return self.file_dialog_key(window, "Escape");
                }
                "Escape" => {
                    self.field = None;
                    if matches!(self.dialog, Some(Dialog::Dimension { .. })) {
                        self.dialog = None;
                    }
                    return Ok(vec![]);
                }
                "Backspace" => {
                    let f = self.field.as_mut().unwrap();
                    if f.replace {
                        f.text.clear();
                        f.replace = false;
                    } else {
                        f.text.pop();
                    }
                    return Ok(vec![]);
                }
                _ => {}
            }
        }
        if self.menu.is_some() && key == "Escape" {
            self.menu = None;
            return Ok(vec![]);
        }
        if matches!(self.dialog, Some(Dialog::File(_))) {
            return self.file_dialog_key(window, &key);
        }
        if let Some(d) = &self.dialog {
            return match (d, key.as_str()) {
                (_, "Escape") => self.dialog_command(window, "cancel"),
                (_, "Enter") => self.dialog_command(window, "ok"),
                _ => Err(format!("unsupported key {key} in a dialog")),
            };
        }
        match key.as_str() {
            "Ctrl+n" | "Ctrl+N" => self.run(window, "Std_New"),
            "Ctrl+o" | "Ctrl+O" => self.run(window, "Std_Open"),
            "Ctrl+s" | "Ctrl+S" => self.run(window, "Std_Save"),
            "Ctrl+Shift+S" | "Ctrl+Shift+s" => self.run(window, "Std_SaveAs"),
            "Ctrl+i" | "Ctrl+I" => self.run(window, "Std_Import"),
            "Ctrl+e" | "Ctrl+E" => self.run(window, "Std_Export"),
            "Ctrl+z" | "Ctrl+Z" => self.run(window, "Std_Undo"),
            "Ctrl+y" | "Ctrl+Y" | "Ctrl+Shift+Z" | "Ctrl+Shift+z" => self.run(window, "Std_Redo"),
            "Ctrl+r" | "Ctrl+R" => self.run(window, "Std_Refresh"),
            "Ctrl+a" | "Ctrl+A" => self.run(window, "Std_SelectAll"),
            "Delete" => self.run(window, "Std_Delete"),
            "Escape" => {
                if let Some(s) = self.sketch_edit_mut() {
                    // Escape cancels the tool in hand first, then leaves the sketch.
                    if s.tool.is_some() || s.pending.is_some() {
                        s.tool = None;
                        s.pending = None;
                        s.clicks.clear();
                        s.snaps.clear();
                        return Ok(vec![]);
                    }
                    if !s.picked.is_empty() || !s.constraints.is_empty() {
                        s.picked.clear();
                        s.constraints.clear();
                        return Ok(vec![]);
                    }
                    self.leave_sketch();
                    return Ok(vec![]);
                }
                if self.task.is_some() {
                    return self.task_command(window, "cancel");
                }
                self.selection.clear();
                Ok(vec![])
            }
            "Enter" => {
                if self.sketch_edit().is_some() {
                    return self.finish_tool();
                }
                if self.task.is_some() {
                    return self.task_command(window, "ok");
                }
                Ok(vec![])
            }
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" => {
                let (_, h) = self.view_px();
                let step = f64::from(h) / 10.0;
                let (dx, dy) = match key.as_str() {
                    "ArrowLeft" => (-step, 0.0),
                    "ArrowRight" => (step, 0.0),
                    "ArrowUp" => (0.0, -step),
                    _ => (0.0, step),
                };
                // Arrow keys move the view the way the scene would be dragged.
                self.camera.pan(-dx, -dy, h);
                Ok(vec![])
            }
            "PageUp" | "PageDown" => {
                let (w, h) = self.view_px();
                let f = if key == "PageUp" { 1.0 / 1.2 } else { 1.2 };
                self.camera
                    .zoom_at(f, f64::from(w) / 2.0, f64::from(h) / 2.0, w, h);
                Ok(vec![])
            }
            other => Err(format!("unsupported FreeCAD key {other}")),
        }
    }

    /// Typed text: into the focused field, or FreeCAD's single-key view shortcuts.
    pub fn type_text(&mut self, text: &str) -> Result<(), String> {
        if let Some(f) = &mut self.field {
            if f.replace {
                f.text.clear();
                f.replace = false;
            }
            crate::apps::push_bounded(&mut f.text, text, 256);
            return Ok(());
        }
        // Behind a modal dialog the view's single-key shortcuts do nothing.
        if self.dialog.is_some() {
            return Err("no text field has the keyboard focus".into());
        }
        let shortcut = match text {
            "0" => Some(StdView::Isometric),
            "1" => Some(StdView::Front),
            "2" => Some(StdView::Top),
            "3" => Some(StdView::Right),
            "4" => Some(StdView::Rear),
            "5" => Some(StdView::Bottom),
            "6" => Some(StdView::Left),
            _ => None,
        };
        match shortcut {
            Some(v) => {
                self.set_view(v);
                Ok(())
            }
            None if text == " " => {
                if self.selection.is_empty() {
                    return Err("Select an object first".into());
                }
                self.run(0, "Std_ToggleVisibility").map(|_| ())
            }
            None => Err("no text field has the keyboard focus".into()),
        }
    }

    /// Focus a field by target.
    fn focus_field(&mut self, rest: &str) -> Result<Vec<AppEffect>, String> {
        if rest == "folder-name" {
            // The New Folder prompt's box keeps what has been typed into it.
            let open = matches!(&self.dialog, Some(Dialog::File(d)) if d.prompt.is_some());
            if !open {
                return Err("no folder is being named".into());
            }
            if !matches!(
                self.field,
                Some(Field {
                    target: FieldTarget::FolderName,
                    ..
                })
            ) {
                self.field = Some(Field {
                    target: FieldTarget::FolderName,
                    text: String::new(),
                    replace: false,
                });
            }
            return Ok(vec![]);
        }
        if rest == "file-name" {
            // Clicking the name box closes a New Folder prompt, as leaving a popover does.
            if let Some(Dialog::File(d)) = &mut self.dialog {
                if d.confirm.is_some() {
                    return Err("Answer whether to replace the file first".into());
                }
                d.prompt = None;
            }
            if matches!(
                self.field,
                Some(Field {
                    target: FieldTarget::FileName,
                    ..
                })
            ) {
                return Ok(vec![]);
            }
        }
        let target = if rest == "file-name" {
            FieldTarget::FileName
        } else if let Some(name) = rest.strip_prefix("task:") {
            FieldTarget::Task {
                name: name.to_owned(),
            }
        } else if let Some(i) = rest.strip_prefix("constraint:") {
            FieldTarget::Constraint {
                index: i.parse().map_err(|_| "bad constraint")?,
            }
        } else {
            return Err(format!("unknown field {rest}"));
        };
        let text = self.field_value(&target)?;
        self.field = Some(Field {
            target,
            text,
            replace: true,
        });
        Ok(vec![])
    }

    /// The text a field starts with: the value it edits, as FreeCAD formats it.
    pub(crate) fn field_value(&self, target: &FieldTarget) -> Result<String, String> {
        Ok(match target {
            FieldTarget::FileName => match &self.dialog {
                Some(Dialog::File(f)) => f.name.clone(),
                _ => return Err("no file dialog is open".into()),
            },
            FieldTarget::FolderName => String::new(),
            FieldTarget::Task { name } => self.task_value(name)?,
            FieldTarget::Constraint { index } => {
                let s = self.sketch().ok_or("no sketch is open")?;
                let c = s.constraints.get(*index).ok_or("no such constraint")?;
                value_text(c)
            }
            FieldTarget::Property { object, name } => self.property_text(object, name)?,
            FieldTarget::Label { object } => self.label_of(object),
        })
    }

    pub(crate) fn commit_field(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        if matches!(
            self.field,
            Some(Field {
                target: FieldTarget::FolderName,
                ..
            })
        ) {
            // The prompt reads its own box, and keeps it when the name is refused.
            return self.file_command(window, "folder-create");
        }
        let f = self.field.take().ok_or("no field is focused")?;
        match &f.target {
            FieldTarget::FolderName => {}
            FieldTarget::FileName => {
                if let Some(Dialog::File(d)) = &mut self.dialog {
                    d.name = f.text.trim().to_owned();
                }
                return self.file_command(window, "ok");
            }
            FieldTarget::Task { name } => {
                let r = self.set_task_value(name, &f.text);
                if let Err(e) = &r {
                    self.field = Some(f.clone());
                    self.status = e.clone();
                }
                r?;
            }
            FieldTarget::Constraint { index } => {
                let r = self.set_constraint_value(*index, &f.text);
                if let Err(e) = &r {
                    self.field = Some(f.clone());
                    self.status = e.clone();
                    return Err(e.clone());
                }
                if matches!(self.dialog, Some(Dialog::Dimension { .. })) {
                    self.dialog = None;
                }
            }
            FieldTarget::Property { object, name } => {
                let (object, name) = (object.clone(), name.clone());
                let r = self.set_property(&object, &name, &f.text);
                if let Err(e) = &r {
                    self.status = e.clone();
                }
                r?;
            }
            FieldTarget::Label { object } => {
                let object = object.clone();
                let text = f.text.trim().to_owned();
                if text.is_empty() {
                    return Err("A label cannot be empty".into());
                }
                self.checkpoint("Rename");
                if let Some(o) = self.doc.get_mut(&object) {
                    o.label = text;
                }
                self.rev += 1;
            }
        }
        Ok(vec![])
    }

    /// The pixel size of the 3D view as last laid out.
    pub(crate) fn view_px(&self) -> (u32, u32) {
        let (w, h) = self.view_size;
        (w.max(100), h.max(100))
    }

    /// Everything the semantic projection shows an agent.
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: &str| cw_protocol::PageAction {
            method: "APP".into(),
            url: url.into(),
            fields: Default::default(),
        };
        page.elements.push(E::Heading {
            id: "freecad-document".into(),
            text: format!(
                "{}{}",
                self.doc.label,
                if self.modified { " *" } else { "" }
            ),
            level: 2,
        });
        let model = self.model();
        for o in &self.doc.objects {
            let status = match model.status.get(&o.name) {
                Some(Status::Error(e)) => format!(" (error: {e})"),
                Some(Status::Inactive) => " (after tip)".into(),
                _ => String::new(),
            };
            page.elements.push(E::Button {
                id: format!("freecad:tree:{}", o.name),
                text: format!("{} [{}]{status}", o.label, o.feature.type_id()),
                action: act(&format!("freecad:tree:{}", o.name)),
            });
        }
        if let Some(s) = self.sketch_edit() {
            page.elements.push(E::Text {
                id: "freecad-solver".into(),
                text: s.report.message(),
            });
        }
        if let Some(body) = self.body() {
            if let Some(shape) = model.body_shape.get(&body) {
                page.elements.push(E::Text {
                    id: "freecad-volume".into(),
                    text: format!(
                        "{} volume {} mm³, area {} mm²",
                        self.label_of(&body),
                        cw_cad::math::fmt_num(shape.mesh.volume(), 3),
                        cw_cad::math::fmt_num(shape.mesh.area(), 3)
                    ),
                });
            }
        }
        for (menu, items) in MENUS {
            for id in items.iter().filter(|i| **i != "-") {
                if self.available(id).is_ok() {
                    let label = command(id).map(|c| c.label).unwrap_or(id);
                    page.elements.push(E::Button {
                        id: format!("freecad:cmd:{id}"),
                        text: format!("{menu} › {label}"),
                        action: act(&format!("freecad:cmd:{id}")),
                    });
                }
            }
        }
        if let Some(f) = &self.field {
            page.elements.push(E::Input {
                id: "freecad-field".into(),
                label: "Value".into(),
                value: f.text.clone(),
                placeholder: String::new(),
            });
        }
        if !self.status.is_empty() {
            page.elements.push(E::Text {
                id: "freecad-status".into(),
                text: self.status.clone(),
            });
        }
    }
}

/// A constraint's value as its edit box shows it.
pub(crate) fn value_text(c: &cw_cad::sketch::Constraint) -> String {
    use cw_cad::sketch::ConstraintType as T;
    match c.kind {
        T::Angle => format!(
            "{} °",
            cw_cad::math::fmt_num(cw_cad::math::degrees(c.value), 2)
        ),
        _ => format!("{} mm", cw_cad::math::fmt_num(c.value, 2)),
    }
}

/// Parse a quantity the way FreeCAD's input fields do: a number with an optional unit.
/// Lengths accept mm, cm, m and in; angles °, deg and rad.
pub(crate) fn parse_quantity(text: &str, angle: bool) -> Result<f64, String> {
    let t = text.trim().replace(',', ".");
    let split = t
        .find(|ch: char| {
            !(ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == '+' || ch == 'e' || ch == 'E')
        })
        .unwrap_or(t.len());
    // "e" may begin a unit only when it is not an exponent: none of ours start with e.
    let (num, unit) = t.split_at(split);
    let v: f64 = num
        .trim()
        .parse()
        .map_err(|_| format!("\"{}\" is not a number", text.trim()))?;
    let unit = unit.trim();
    let scale = if angle {
        match unit {
            "" | "°" | "deg" => 1.0,
            "rad" => cw_cad::math::degrees(1.0),
            u => return Err(format!("Unit mismatch: {u} is not an angle")),
        }
    } else {
        match unit {
            "" | "mm" => 1.0,
            "cm" => 10.0,
            "m" => 1000.0,
            "in" | "\"" => 25.4,
            "µm" | "um" => 0.001,
            u => return Err(format!("Unit mismatch: {u} is not a length")),
        }
    };
    let v = v * scale;
    if !v.is_finite() {
        return Err("The value is not a number".into());
    }
    Ok(v)
}
