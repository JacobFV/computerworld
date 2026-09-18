//! Part Design task panels: creating a feature opens its parameters in the Tasks tab
//! with a live preview; OK keeps it, Cancel returns the document to where it was.
use super::commands::parse_quantity;
use super::*;
use cw_cad::document::{AxisRef, BasePlane, Extent, HoleCut, PlaneRef, SubRef, Support};
use cw_cad::math::fmt_num;
use cw_cad::sketch::Sketch;

/// A parameter the task panel shows: label, value text, and how it is edited.
pub enum Param {
    Number {
        name: &'static str,
        label: &'static str,
        value: String,
    },
    Toggle {
        name: &'static str,
        label: &'static str,
        on: bool,
    },
    Choice {
        name: &'static str,
        label: &'static str,
        value: String,
        options: Vec<(String, String)>,
    },
}

impl Cad {
    /// Create Sketch: on the selected planar face, on a selected base plane, or ask.
    pub(crate) fn new_sketch(&mut self) -> Result<(), String> {
        let body = self.body().ok_or("There is no body; create one first")?;
        if let Some(sel) = self
            .selection
            .iter()
            .find(|s| s.sub.starts_with("Face"))
            .cloned()
        {
            let model = self.model();
            let shape = model
                .shapes
                .get(&sel.object)
                .ok_or("the face has no shape")?
                .clone();
            let i = Cad::element_of(&sel.sub).and_then(|e| match e {
                Element::Face(i) => Some(i),
                _ => None,
            });
            let i = i.ok_or("bad face")?;
            if cw_cad::solid::face_frame(&shape.mesh, &shape.topo, i).is_none() {
                return Err("The selected face is not planar".into());
            }
            let face = SubRef::face(&shape.topo, &shape.mesh, i);
            let support = Support::Face {
                feature: sel.object.clone(),
                face,
            };
            return self.create_sketch(&body, support);
        }
        self.task = Some(Task::PickPlane {
            body,
            plane: "XY_Plane".into(),
        });
        self.combo = ComboTab::Tasks;
        self.status = "Select a plane for the sketch".into();
        Ok(())
    }
    fn create_sketch(&mut self, body: &str, support: Support) -> Result<(), String> {
        self.checkpoint("Create sketch");
        let name = self.doc.add_to_body(
            body,
            Feature::Sketch {
                sketch: Sketch::default(),
                support,
                offset: 0.0,
            },
        )?;
        self.recompute();
        if let Some(e) = self.model().error(&name) {
            let e = e.to_owned();
            self.log(ReportKind::Error, &e);
        }
        self.expanded.insert(body.to_owned());
        self.task = None;
        self.open_sketch(&name)
    }

    /// The sketch a new sketch-based feature uses: the selected one, else the last
    /// sketch in the body no feature has taken yet.
    pub(crate) fn profile_for(&self, body: &str) -> Result<String, String> {
        let is_sketch = |n: &str| {
            matches!(
                self.doc.get(n).map(|o| &o.feature),
                Some(Feature::Sketch { .. })
            )
        };
        if let Some(s) = self.selection.iter().find(|s| is_sketch(&s.object)) {
            return Ok(s.object.clone());
        }
        let Some(Feature::Body { group, .. }) = self.doc.get(body).map(|o| &o.feature) else {
            return Err("no body".into());
        };
        let used: BTreeSet<&str> = self
            .doc
            .objects
            .iter()
            .filter_map(|o| o.feature.profile())
            .collect();
        group
            .iter()
            .rev()
            .find(|g| is_sketch(g) && !used.contains(g.as_str()))
            .cloned()
            .ok_or_else(|| "Select a sketch, or create one first".into())
    }

    pub(crate) fn new_sketch_feature(&mut self, id: &str) -> Result<(), String> {
        let body = self.body().ok_or("There is no body")?;
        let profile = self.profile_for(&body)?;
        let feature = match id {
            "PartDesign_Pad" => Feature::Pad {
                profile: profile.clone(),
                extent: Extent::Length,
                length: 10.0,
                length2: 0.0,
                midplane: false,
                reversed: false,
            },
            "PartDesign_Pocket" => Feature::Pocket {
                profile: profile.clone(),
                extent: Extent::Length,
                length: 5.0,
                length2: 0.0,
                midplane: false,
                reversed: false,
            },
            "PartDesign_Revolution" => Feature::Revolution {
                profile: profile.clone(),
                axis: AxisRef::SketchV,
                angle: 360.0,
                midplane: false,
                reversed: false,
            },
            "PartDesign_Groove" => Feature::Groove {
                profile: profile.clone(),
                axis: AxisRef::SketchV,
                angle: 360.0,
                midplane: false,
                reversed: false,
            },
            _ => Feature::Hole {
                profile: profile.clone(),
                diameter: 6.0,
                depth: 10.0,
                through_all: false,
                cut: HoleCut::None,
                drill_point: Some(118.0),
            },
        };
        let label = super::commands::command(id)
            .map(|c| c.label)
            .unwrap_or("Feature");
        self.begin_feature(&body, feature, label)?;
        // The profile hides once a feature consumes it.
        if let Some(o) = self.doc.get_mut(&profile) {
            o.visible = false;
        }
        Ok(())
    }

    pub(crate) fn new_dressup(&mut self, id: &str) -> Result<(), String> {
        let body = self.body().ok_or("There is no body")?;
        let model = self.model();
        let mut edges = Vec::new();
        for s in self.selection.iter().filter(|s| s.sub.starts_with("Edge")) {
            let shape = model.shapes.get(&s.object).ok_or("the edge has no shape")?;
            if let Some(Element::Edge(i)) = Cad::element_of(&s.sub) {
                if i < shape.topo.edges.len() {
                    edges.push(SubRef::edge(&shape.topo, &shape.mesh, i));
                }
            }
        }
        if edges.is_empty() {
            return Err("Select one or more edges of the body first".into());
        }
        let feature = if id == "PartDesign_Fillet" {
            Feature::Fillet { edges, radius: 1.0 }
        } else {
            Feature::Chamfer { edges, size: 1.0 }
        };
        let label = if id == "PartDesign_Fillet" {
            "Fillet"
        } else {
            "Chamfer"
        };
        self.begin_feature(&body, feature, label)
    }

    /// Solid features the selection names that a pattern can repeat.
    pub(crate) fn pattern_originals(&self) -> Vec<String> {
        let model = self.model();
        let mut out: Vec<String> = self
            .selection
            .iter()
            .map(|s| s.object.clone())
            .filter(|o| model.tools.contains_key(o))
            .collect();
        out.dedup();
        out
    }

    pub(crate) fn new_transform(&mut self, id: &str) -> Result<(), String> {
        let body = self.body().ok_or("There is no body")?;
        let originals = self.pattern_originals();
        if originals.is_empty() {
            return Err("Select a pad, pocket, revolution, groove or hole to transform".into());
        }
        let (feature, label) = match id {
            "PartDesign_Mirrored" => (
                Feature::Mirrored {
                    originals,
                    plane: PlaneRef::SketchV,
                },
                "Mirror",
            ),
            "PartDesign_LinearPattern" => (
                Feature::LinearPattern {
                    originals,
                    direction: AxisRef::SketchH,
                    length: 100.0,
                    occurrences: 2,
                    reversed: false,
                },
                "Linear Pattern",
            ),
            _ => (
                Feature::PolarPattern {
                    originals,
                    axis: AxisRef::SketchNormal,
                    angle: 360.0,
                    occurrences: 3,
                    reversed: false,
                },
                "Polar Pattern",
            ),
        };
        self.begin_feature(&body, feature, label)
    }

    fn begin_feature(&mut self, body: &str, feature: Feature, label: &str) -> Result<(), String> {
        let before = self.doc.clone();
        self.checkpoint(label);
        let name = self.doc.add_to_body(body, feature)?;
        self.recompute();
        self.task = Some(Task::Feature(Box::new(FeatureEdit {
            name,
            before,
            select_mode: None,
        })));
        self.combo = ComboTab::Tasks;
        // The new feature previews as it is; nothing stays selected over it.
        self.selection.clear();
        self.status = format!("{label} parameters");
        Ok(())
    }

    pub(crate) fn open_feature_task(&mut self, name: &str) {
        let before = self.doc.clone();
        let label = self
            .doc
            .get(name)
            .map(|o| o.feature.base_name())
            .unwrap_or("Feature");
        self.checkpoint(&format!("Edit {label}"));
        self.task = Some(Task::Feature(Box::new(FeatureEdit {
            name: name.to_owned(),
            before,
            select_mode: None,
        })));
        self.combo = ComboTab::Tasks;
    }

    fn feature_mut(&mut self) -> Result<(&mut Feature, String), String> {
        let name = match &self.task {
            Some(Task::Feature(f)) => f.name.clone(),
            _ => return Err("no feature is being edited".into()),
        };
        let o = self.doc.get_mut(&name).ok_or("the feature is gone")?;
        Ok((&mut o.feature, name))
    }
    fn feature(&self) -> Option<(&Feature, &str)> {
        match &self.task {
            Some(Task::Feature(f)) => self.doc.get(&f.name).map(|o| (&o.feature, f.name.as_str())),
            _ => None,
        }
    }

    /// The parameters the open feature task shows, in FreeCAD's order.
    pub fn task_params(&self) -> Vec<Param> {
        let Some((f, _)) = self.feature() else {
            return vec![];
        };
        let mm = |v: f64| format!("{} mm", fmt_num(v, 2));
        let deg = |v: f64| format!("{} °", fmt_num(v, 2));
        let axes = |sketch_based: bool| -> Vec<(String, String)> {
            let mut v = vec![];
            if sketch_based {
                v.push(("V_Axis".into(), "Vertical sketch axis".into()));
                v.push(("H_Axis".into(), "Horizontal sketch axis".into()));
            }
            v.extend([
                ("X_Axis".into(), "X axis".into()),
                ("Y_Axis".into(), "Y axis".into()),
                ("Z_Axis".into(), "Z axis".into()),
            ]);
            v
        };
        let extent_name = |e: Extent| match e {
            Extent::Length => "Dimension",
            Extent::TwoLengths => "Two dimensions",
            Extent::ThroughAll => "Through all",
        };
        match f {
            Feature::Pad {
                extent,
                length,
                length2,
                midplane,
                reversed,
                ..
            }
            | Feature::Pocket {
                extent,
                length,
                length2,
                midplane,
                reversed,
                ..
            } => {
                let mut v = vec![Param::Choice {
                    name: "Type",
                    label: "Type",
                    value: extent_name(*extent).into(),
                    options: [Extent::Length, Extent::TwoLengths, Extent::ThroughAll]
                        .into_iter()
                        .map(|e| (extent_name(e).to_owned(), extent_name(e).to_owned()))
                        .collect(),
                }];
                if *extent != Extent::ThroughAll {
                    v.push(Param::Number {
                        name: "Length",
                        label: "Length",
                        value: mm(*length),
                    });
                }
                if *extent == Extent::TwoLengths {
                    v.push(Param::Number {
                        name: "Length2",
                        label: "2nd length",
                        value: mm(*length2),
                    });
                }
                v.push(Param::Toggle {
                    name: "Midplane",
                    label: "Symmetric to plane",
                    on: *midplane,
                });
                v.push(Param::Toggle {
                    name: "Reversed",
                    label: "Reversed",
                    on: *reversed,
                });
                v
            }
            Feature::Revolution {
                axis,
                angle,
                midplane,
                reversed,
                ..
            }
            | Feature::Groove {
                axis,
                angle,
                midplane,
                reversed,
                ..
            } => vec![
                Param::Choice {
                    name: "Axis",
                    label: "Axis",
                    value: axis.label(),
                    options: axes(true),
                },
                Param::Number {
                    name: "Angle",
                    label: "Angle",
                    value: deg(*angle),
                },
                Param::Toggle {
                    name: "Midplane",
                    label: "Symmetric to plane",
                    on: *midplane,
                },
                Param::Toggle {
                    name: "Reversed",
                    label: "Reversed",
                    on: *reversed,
                },
            ],
            Feature::Fillet { radius, .. } => vec![Param::Number {
                name: "Radius",
                label: "Radius",
                value: mm(*radius),
            }],
            Feature::Chamfer { size, .. } => vec![Param::Number {
                name: "Size",
                label: "Size",
                value: mm(*size),
            }],
            Feature::Hole {
                diameter,
                depth,
                through_all,
                cut,
                drill_point,
                ..
            } => {
                let cut_name = match cut {
                    HoleCut::None => "None",
                    HoleCut::Counterbore { .. } => "Counterbore",
                    HoleCut::Countersink { .. } => "Countersink",
                };
                let mut v = vec![Param::Number {
                    name: "Diameter",
                    label: "Diameter",
                    value: mm(*diameter),
                }];
                v.push(Param::Choice {
                    name: "DepthType",
                    label: "Depth type",
                    value: if *through_all {
                        "Through all"
                    } else {
                        "Dimension"
                    }
                    .into(),
                    options: vec![
                        ("Dimension".into(), "Dimension".into()),
                        ("Through all".into(), "Through all".into()),
                    ],
                });
                if !*through_all {
                    v.push(Param::Number {
                        name: "Depth",
                        label: "Depth",
                        value: mm(*depth),
                    });
                }
                v.push(Param::Choice {
                    name: "CutType",
                    label: "Hole cut type",
                    value: cut_name.into(),
                    options: ["None", "Counterbore", "Countersink"]
                        .iter()
                        .map(|s| ((*s).into(), (*s).into()))
                        .collect(),
                });
                match cut {
                    HoleCut::Counterbore { diameter, depth } => {
                        v.push(Param::Number {
                            name: "CutDiameter",
                            label: "Hole cut diameter",
                            value: mm(*diameter),
                        });
                        v.push(Param::Number {
                            name: "CutDepth",
                            label: "Hole cut depth",
                            value: mm(*depth),
                        });
                    }
                    HoleCut::Countersink { diameter, angle } => {
                        v.push(Param::Number {
                            name: "CutDiameter",
                            label: "Hole cut diameter",
                            value: mm(*diameter),
                        });
                        v.push(Param::Number {
                            name: "CutAngle",
                            label: "Countersink angle",
                            value: deg(*angle),
                        });
                    }
                    HoleCut::None => {}
                }
                if !*through_all {
                    v.push(Param::Choice {
                        name: "DrillPoint",
                        label: "Drill point",
                        value: if drill_point.is_some() {
                            "Angled"
                        } else {
                            "Flat"
                        }
                        .into(),
                        options: vec![
                            ("Flat".into(), "Flat".into()),
                            ("Angled".into(), "Angled".into()),
                        ],
                    });
                    if let Some(a) = drill_point {
                        v.push(Param::Number {
                            name: "DrillAngle",
                            label: "Drill point angle",
                            value: deg(*a),
                        });
                    }
                }
                v
            }
            Feature::Mirrored { plane, .. } => vec![Param::Choice {
                name: "Plane",
                label: "Plane",
                value: plane.label(),
                options: vec![
                    ("V_Axis".into(), "Vertical sketch axis".into()),
                    ("H_Axis".into(), "Horizontal sketch axis".into()),
                    ("XY_Plane".into(), "XY_Plane".into()),
                    ("XZ_Plane".into(), "XZ_Plane".into()),
                    ("YZ_Plane".into(), "YZ_Plane".into()),
                ],
            }],
            Feature::LinearPattern {
                direction,
                length,
                occurrences,
                reversed,
                ..
            } => vec![
                Param::Choice {
                    name: "Direction",
                    label: "Direction",
                    value: direction.label(),
                    options: axes(true),
                },
                Param::Toggle {
                    name: "Reversed",
                    label: "Reverse direction",
                    on: *reversed,
                },
                Param::Number {
                    name: "Length",
                    label: "Length",
                    value: mm(*length),
                },
                Param::Number {
                    name: "Occurrences",
                    label: "Occurrences",
                    value: occurrences.to_string(),
                },
            ],
            Feature::PolarPattern {
                axis,
                angle,
                occurrences,
                reversed,
                ..
            } => {
                let mut options = vec![("N_Axis".to_owned(), "Normal sketch axis".to_owned())];
                options.extend(axes(true));
                vec![
                    Param::Choice {
                        name: "Axis",
                        label: "Axis",
                        value: axis.label(),
                        options,
                    },
                    Param::Toggle {
                        name: "Reversed",
                        label: "Reverse direction",
                        on: *reversed,
                    },
                    Param::Number {
                        name: "Angle",
                        label: "Angle",
                        value: deg(*angle),
                    },
                    Param::Number {
                        name: "Occurrences",
                        label: "Occurrences",
                        value: occurrences.to_string(),
                    },
                ]
            }
            _ => vec![],
        }
    }

    pub(crate) fn task_value(&self, name: &str) -> Result<String, String> {
        if name == "fillet_radius" {
            let r = self
                .sketch_edit()
                .map(|e| e.fillet_radius)
                .ok_or("no sketch")?;
            return Ok(format!("{} mm", fmt_num(r, 2)));
        }
        for p in self.task_params() {
            if let Param::Number { name: n, value, .. } = p {
                if n == name {
                    return Ok(value);
                }
            }
        }
        Err(format!("the task has no {name} field"))
    }

    pub(crate) fn set_task_value(&mut self, name: &str, text: &str) -> Result<(), String> {
        if name == "fillet_radius" {
            let v = parse_quantity(text, false)?;
            if v <= 0.0 {
                return Err("The radius must be positive".into());
            }
            self.sketch_edit_mut().ok_or("no sketch")?.fillet_radius = v;
            return Ok(());
        }
        let angle = matches!(name, "Angle" | "CutAngle" | "DrillAngle");
        let count = name == "Occurrences";
        let v = if count {
            let n: u32 = text
                .trim()
                .parse()
                .map_err(|_| format!("\"{}\" is not a whole number", text.trim()))?;
            if !(2..=100).contains(&n) {
                return Err("Occurrences must be between 2 and 100".into());
            }
            f64::from(n)
        } else {
            parse_quantity(text, angle)?
        };
        if !count && !angle && v <= 0.0 && name != "Length2" {
            return Err(format!("{name} must be positive"));
        }
        let (f, _) = self.feature_mut()?;
        match (f, name) {
            (Feature::Pad { length, .. } | Feature::Pocket { length, .. }, "Length") => *length = v,
            (Feature::Pad { length2, .. } | Feature::Pocket { length2, .. }, "Length2") => {
                *length2 = v
            }
            (Feature::Revolution { angle, .. } | Feature::Groove { angle, .. }, "Angle") => {
                if v.abs() > 360.0 || v == 0.0 {
                    return Err("The angle must be between 0 and 360°".into());
                }
                *angle = v
            }
            (Feature::Fillet { radius, .. }, "Radius") => *radius = v,
            (Feature::Chamfer { size, .. }, "Size") => *size = v,
            (Feature::Hole { diameter, .. }, "Diameter") => *diameter = v,
            (Feature::Hole { depth, .. }, "Depth") => *depth = v,
            (
                Feature::Hole {
                    cut:
                        HoleCut::Counterbore { diameter, .. } | HoleCut::Countersink { diameter, .. },
                    ..
                },
                "CutDiameter",
            ) => *diameter = v,
            (
                Feature::Hole {
                    cut: HoleCut::Counterbore { depth, .. },
                    ..
                },
                "CutDepth",
            ) => *depth = v,
            (
                Feature::Hole {
                    cut: HoleCut::Countersink { angle, .. },
                    ..
                },
                "CutAngle",
            ) => *angle = v,
            (
                Feature::Hole {
                    drill_point: Some(a),
                    ..
                },
                "DrillAngle",
            ) => *a = v,
            (Feature::LinearPattern { length, .. }, "Length") => *length = v,
            (
                Feature::LinearPattern { occurrences, .. }
                | Feature::PolarPattern { occurrences, .. },
                "Occurrences",
            ) => *occurrences = v as u32,
            (Feature::PolarPattern { angle, .. }, "Angle") => {
                if v.abs() > 360.0 || v == 0.0 {
                    return Err("The angle must be between 0 and 360°".into());
                }
                *angle = v
            }
            _ => return Err(format!("the task has no {name} field")),
        }
        self.recompute();
        Ok(())
    }

    fn set_task_toggle(&mut self, name: &str) -> Result<(), String> {
        let (f, _) = self.feature_mut()?;
        match (f, name) {
            (
                Feature::Pad { midplane, .. }
                | Feature::Pocket { midplane, .. }
                | Feature::Revolution { midplane, .. }
                | Feature::Groove { midplane, .. },
                "Midplane",
            ) => *midplane = !*midplane,
            (
                Feature::Pad { reversed, .. }
                | Feature::Pocket { reversed, .. }
                | Feature::Revolution { reversed, .. }
                | Feature::Groove { reversed, .. }
                | Feature::LinearPattern { reversed, .. }
                | Feature::PolarPattern { reversed, .. },
                "Reversed",
            ) => *reversed = !*reversed,
            _ => return Err(format!("the task has no {name} option")),
        }
        self.recompute();
        Ok(())
    }

    fn set_task_choice(&mut self, name: &str, value: &str) -> Result<(), String> {
        let axis = |v: &str| -> Option<AxisRef> {
            Some(match v {
                "V_Axis" => AxisRef::SketchV,
                "H_Axis" => AxisRef::SketchH,
                "N_Axis" => AxisRef::SketchNormal,
                "X_Axis" => AxisRef::X,
                "Y_Axis" => AxisRef::Y,
                "Z_Axis" => AxisRef::Z,
                _ => return None,
            })
        };
        let (f, _) = self.feature_mut()?;
        match (f, name) {
            (Feature::Pad { extent, .. } | Feature::Pocket { extent, .. }, "Type") => {
                *extent = match value {
                    "Dimension" => Extent::Length,
                    "Two dimensions" => Extent::TwoLengths,
                    "Through all" => Extent::ThroughAll,
                    _ => return Err("unknown type".into()),
                }
            }
            (
                Feature::Revolution { axis: a, .. }
                | Feature::Groove { axis: a, .. }
                | Feature::PolarPattern { axis: a, .. },
                "Axis",
            ) => *a = axis(value).ok_or("unknown axis")?,
            (Feature::LinearPattern { direction, .. }, "Direction") => {
                *direction = axis(value).ok_or("unknown direction")?
            }
            (Feature::Mirrored { plane, .. }, "Plane") => {
                *plane = match value {
                    "V_Axis" => PlaneRef::SketchV,
                    "H_Axis" => PlaneRef::SketchH,
                    other => PlaneRef::Base(
                        cw_cad::document::plane_by_name(other).ok_or("unknown plane")?,
                    ),
                }
            }
            (Feature::Hole { through_all, .. }, "DepthType") => {
                *through_all = value == "Through all"
            }
            (Feature::Hole { cut, diameter, .. }, "CutType") => {
                *cut = match value {
                    "None" => HoleCut::None,
                    "Counterbore" => HoleCut::Counterbore {
                        diameter: *diameter * 1.8,
                        depth: 2.0,
                    },
                    "Countersink" => HoleCut::Countersink {
                        diameter: *diameter * 2.0,
                        angle: 90.0,
                    },
                    _ => return Err("unknown cut type".into()),
                }
            }
            (Feature::Hole { drill_point, .. }, "DrillPoint") => {
                *drill_point = if value == "Angled" { Some(118.0) } else { None };
            }
            _ => return Err(format!("the task has no {name} choice")),
        }
        self.recompute();
        Ok(())
    }

    /// Dropdown choices: `task/<param>:<value>` and `prop/<property>:<value>`.
    pub(crate) fn choice(&mut self, window: u64, rest: &str) -> Result<Vec<AppEffect>, String> {
        if let Some(open) = rest.strip_prefix("open:") {
            let id = format!("dd:{open}");
            self.menu = if self.menu.as_deref() == Some(id.as_str()) {
                None
            } else {
                Some(id)
            };
            return Ok(vec![]);
        }
        self.menu = None;
        let (target, value) = rest.split_once(':').ok_or("bad choice")?;
        if let Some(name) = target.strip_prefix("task/") {
            self.set_task_choice(name, value)?;
            return Ok(vec![]);
        }
        if let Some(name) = target.strip_prefix("prop/") {
            let object = self.selected_object().ok_or("Select an object")?.to_owned();
            self.set_property(&object, name, value)?;
            return Ok(vec![]);
        }
        if target == "filetype" {
            return self.file_command(window, &format!("type:{value}"));
        }
        Err("unknown choice".into())
    }

    /// Clicks in the task panel.
    pub(crate) fn task_command(
        &mut self,
        _window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        if let Some(name) = rest.strip_prefix("toggle:") {
            self.set_task_toggle(name)?;
            return Ok(vec![]);
        }
        if let Some(plane) = rest.strip_prefix("plane:") {
            if let Some(Task::PickPlane { plane: p, .. }) = &mut self.task {
                *p = plane.to_owned();
                return Ok(vec![]);
            }
            return Err("no plane is being chosen".into());
        }
        if let Some(mode) = rest.strip_prefix("select:") {
            let Some(Task::Feature(f)) = &mut self.task else {
                return Err("no feature is being edited".into());
            };
            let want = mode == "add";
            f.select_mode = if f.select_mode == Some(want) {
                None
            } else {
                Some(want)
            };
            self.status = match f.select_mode {
                Some(true) => "Click edges to add them",
                Some(false) => "Click edges to remove them",
                None => "",
            }
            .into();
            return Ok(vec![]);
        }
        if let Some(i) = rest.strip_prefix("remove-ref:") {
            let i: usize = i.parse().map_err(|_| "bad reference")?;
            let (f, _) = self.feature_mut()?;
            match f {
                Feature::Fillet { edges, .. } | Feature::Chamfer { edges, .. } => {
                    if edges.len() <= 1 {
                        return Err("A fillet needs at least one edge".into());
                    }
                    if i >= edges.len() {
                        return Err("no such edge".into());
                    }
                    edges.remove(i);
                }
                Feature::Mirrored { originals, .. }
                | Feature::LinearPattern { originals, .. }
                | Feature::PolarPattern { originals, .. } => {
                    if originals.len() <= 1 {
                        return Err("A pattern needs at least one original".into());
                    }
                    if i >= originals.len() {
                        return Err("no such feature".into());
                    }
                    originals.remove(i);
                }
                _ => return Err("this feature has no references".into()),
            }
            self.recompute();
            return Ok(vec![]);
        }
        match rest {
            "ok" => match self.task.clone() {
                Some(Task::Feature(f)) => {
                    if let Some(e) = self.model().error(&f.name) {
                        let e = e.to_owned();
                        // FreeCAD keeps the panel open on a failed feature and says why.
                        self.dialog = Some(Dialog::Message {
                            title: "Input error".into(),
                            text: e,
                        });
                        return Ok(vec![]);
                    }
                    self.task = None;
                    self.combo = ComboTab::Model;
                    self.selection.clear();
                    self.status = format!("{} done", self.label_of(&f.name));
                    self.log(ReportKind::Log, &format!("Recomputed {}", f.name));
                    Ok(vec![])
                }
                Some(Task::PickPlane { body, plane }) => {
                    let p = cw_cad::document::plane_by_name(&plane).ok_or("Select a plane")?;
                    self.create_sketch(&body, Support::Plane { plane: p })?;
                    Ok(vec![])
                }
                Some(Task::Sketch(_)) => {
                    self.leave_sketch();
                    Ok(vec![])
                }
                Some(Task::Measure) | None => {
                    self.task = None;
                    self.combo = ComboTab::Model;
                    Ok(vec![])
                }
            },
            "cancel" => {
                match self.task.clone() {
                    Some(Task::Feature(f)) => {
                        self.doc = f.before.clone();
                        self.undo.pop();
                        self.selection.clear();
                        self.recompute();
                        self.status = "Cancelled".into();
                    }
                    Some(Task::Sketch(_)) => {
                        self.leave_sketch();
                        return Ok(vec![]);
                    }
                    _ => {}
                }
                self.task = None;
                self.combo = ComboTab::Model;
                Ok(vec![])
            }
            "measure-clear" => {
                self.selection.clear();
                Ok(vec![])
            }
            other => Err(format!("unknown task command {other}")),
        }
    }

    /// While a pattern's task is open, a tree click adds or removes an original.
    pub(crate) fn toggle_original(&mut self, feature: &str, clicked: &str) -> Result<bool, String> {
        let is_tool = self.model().tools.contains_key(clicked);
        let Some(Object { feature: f, .. }) = self.doc.get_mut(feature) else {
            return Ok(false);
        };
        let (Feature::Mirrored { originals, .. }
        | Feature::LinearPattern { originals, .. }
        | Feature::PolarPattern { originals, .. }) = f
        else {
            return Ok(false);
        };
        if !is_tool || clicked == feature {
            return Ok(false);
        }
        if let Some(i) = originals.iter().position(|o| o == clicked) {
            if originals.len() > 1 {
                originals.remove(i);
            }
        } else {
            originals.push(clicked.to_owned());
        }
        self.recompute();
        Ok(true)
    }

    /// While a fillet or chamfer task is in add/remove mode, a picked edge changes it.
    pub(crate) fn dressup_pick(&mut self, object: &str, element: Element) -> Result<bool, String> {
        let (name, mode) = match &self.task {
            Some(Task::Feature(f)) => match f.select_mode {
                Some(m) => (f.name.clone(), m),
                None => return Ok(false),
            },
            _ => return Ok(false),
        };
        let Element::Edge(i) = element else {
            return Ok(false);
        };
        // Edges belong to the shape the fillet starts from: the previous feature's.
        let base = self
            .doc
            .body_of(&name)
            .and_then(|b| match &self.doc.get(b)?.feature {
                Feature::Body { group, .. } => {
                    let at = group.iter().position(|g| g == &name)?;
                    group[..at]
                        .iter()
                        .rev()
                        .find(|g| self.model().shapes.contains_key(*g))
                        .cloned()
                }
                _ => None,
            });
        let base = base.ok_or("the feature has no base shape")?;
        let model = self.model();
        // The view shows the result; map the picked edge back onto the base by position.
        let shape = model.shapes.get(object).ok_or("no shape")?;
        let picked = SubRef::edge(&shape.topo, &shape.mesh, i);
        let base_shape = model.shapes.get(&base).ok_or("no base shape")?;
        let j = cw_cad::document::resolve(&picked, &base_shape.mesh, &base_shape.topo)
            .ok_or("That edge is not on the base shape")?;
        let r = SubRef::edge(&base_shape.topo, &base_shape.mesh, j);
        let (f, _) = self.feature_mut()?;
        let (Feature::Fillet { edges, .. } | Feature::Chamfer { edges, .. }) = f else {
            return Ok(false);
        };
        let at = edges.iter().position(|e| e.center.dist(r.center) < 1e-6);
        match (mode, at) {
            (true, None) => edges.push(r),
            (false, Some(k)) if edges.len() > 1 => {
                edges.remove(k);
            }
            _ => return Ok(true),
        }
        self.recompute();
        Ok(true)
    }

    /// Everything the Measure panel reports about the current selection.
    pub fn measurements(&self) -> Vec<(String, String)> {
        let model = self.model();
        let mut out = Vec::new();
        let mm = |v: f64| format!("{} mm", fmt_num(v, 2));
        let whole: Vec<&Sel> = self.selection.iter().filter(|s| s.sub.is_empty()).collect();
        for s in &whole {
            let shape = match self.doc.get(&s.object).map(|o| &o.feature) {
                Some(Feature::Body { .. }) => model.body_shape.get(&s.object),
                _ => model.shapes.get(&s.object),
            };
            if let Some(shape) = shape {
                let m = &shape.mesh;
                out.push((
                    format!("{} volume", self.label_of(&s.object)),
                    format!("{} mm³", fmt_num(m.volume(), 2)),
                ));
                out.push((
                    "Surface area".into(),
                    format!("{} mm²", fmt_num(m.area(), 2)),
                ));
                if let Some(c) = m.center_of_mass() {
                    out.push((
                        "Center of mass".into(),
                        format!(
                            "({}, {}, {}) mm",
                            fmt_num(c.x, 2),
                            fmt_num(c.y, 2),
                            fmt_num(c.z, 2)
                        ),
                    ));
                }
                if let Some(b) = m.bounds() {
                    let d = b.size();
                    out.push((
                        "Bounding box".into(),
                        format!(
                            "{} × {} × {} mm",
                            fmt_num(d.x, 2),
                            fmt_num(d.y, 2),
                            fmt_num(d.z, 2)
                        ),
                    ));
                }
            }
        }
        let subs: Vec<(&Sel, Element, Arc<cw_cad::document::Shape>)> = self
            .selection
            .iter()
            .filter_map(|s| {
                let e = Cad::element_of(&s.sub)?;
                let shape = model.shapes.get(&s.object)?.clone();
                Some((s, e, shape))
            })
            .collect();
        let point_of = |e: Element, sh: &cw_cad::document::Shape, fallback: V3| -> V3 {
            match e {
                Element::Vertex(i) => sh.mesh.verts[sh.topo.vertices[i] as usize],
                _ => fallback,
            }
        };
        match subs.as_slice() {
            [(_, Element::Face(i), sh)] => {
                out.push((
                    "Area".into(),
                    format!("{} mm²", fmt_num(sh.topo.faces[*i].area, 2)),
                ));
                if let Some(r) = face_radius(sh, *i) {
                    out.push(("Radius".into(), mm(r)));
                }
            }
            [(_, Element::Edge(i), sh)] => {
                let e = &sh.topo.edges[*i];
                out.push(("Length".into(), mm(e.length)));
                if let Some((_, r, _)) = e.circle {
                    out.push(("Radius".into(), mm(r)));
                }
            }
            [(s, Element::Vertex(i), sh)] => {
                let p = point_of(Element::Vertex(*i), sh, s.point);
                out.push((
                    "Position".into(),
                    format!(
                        "({}, {}, {}) mm",
                        fmt_num(p.x, 2),
                        fmt_num(p.y, 2),
                        fmt_num(p.z, 2)
                    ),
                ));
            }
            [(s1, e1, sh1), (s2, e2, sh2)] => {
                let dir = |e: Element, sh: &cw_cad::document::Shape| -> Option<V3> {
                    match e {
                        Element::Edge(i)
                            if sh.topo.edges[i].kind == cw_cad::solid::EdgeKind::Line =>
                        {
                            Some(SubRef::edge(&sh.topo, &sh.mesh, i).dir)
                        }
                        Element::Face(i) => {
                            match &sh.mesh.surfaces[sh.topo.faces[i].surface as usize] {
                                cw_cad::mesh::Surface::Plane { normal, .. } => Some(*normal),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                };
                // Distance: exact between vertices, between parallel planes, else
                // between the picked points.
                let d = match (e1, e2) {
                    (Element::Face(a), Element::Face(b)) => match (dir(*e1, sh1), dir(*e2, sh2)) {
                        (Some(n1), Some(n2)) if n1.cross(n2).len() < 1e-9 => Some(
                            (sh2.topo.faces[*b].centroid - sh1.topo.faces[*a].centroid)
                                .dot(n1)
                                .abs(),
                        ),
                        _ => None,
                    },
                    _ => None,
                };
                let d = d.unwrap_or_else(|| {
                    point_of(*e1, sh1, s1.point).dist(point_of(*e2, sh2, s2.point))
                });
                out.push(("Distance".into(), mm(d)));
                if let (Some(a), Some(b)) = (dir(*e1, sh1), dir(*e2, sh2)) {
                    let ang = cw_cad::math::degrees(cw_cad::math::acos(a.dot(b).clamp(-1.0, 1.0)));
                    out.push(("Angle".into(), format!("{} °", fmt_num(ang, 2))));
                }
            }
            _ => {}
        }
        out
    }
}

fn face_radius(sh: &cw_cad::document::Shape, i: usize) -> Option<f64> {
    match &sh.mesh.surfaces[sh.topo.faces[i].surface as usize] {
        cw_cad::mesh::Surface::Cylinder { radius, .. } => Some(*radius),
        _ => None,
    }
}

/// A base plane for the plane chooser, by name.
pub fn base_planes() -> [(BasePlane, &'static str); 3] {
    [
        (BasePlane::XY, "XY_Plane (Base plane)"),
        (BasePlane::XZ, "XZ_Plane (Base plane)"),
        (BasePlane::YZ, "YZ_Plane (Base plane)"),
    ]
}
