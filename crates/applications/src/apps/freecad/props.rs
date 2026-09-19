//! The property editor: an object's Data and View properties by FreeCAD's names, every
//! editable one really editable, recomputing the document as it changes.
use super::commands::parse_quantity;
use super::*;
use cw_cad::document::{AxisRef, BoolType, ChamferType, Extent, HoleCut, PlaneRef, Support};
use cw_cad::math::fmt_num;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Number,
    Bool(bool),
    Enum(Vec<String>),
    Text,
    ReadOnly,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub group: &'static str,
    pub name: String,
    pub value: String,
    pub kind: Kind,
    /// The expression the property is bound to, when it is (FreeCAD shows such a
    /// value in blue italics and edits it through the f(x) button).
    pub expression: Option<String>,
}
fn row(group: &'static str, name: &str, value: String, kind: Kind) -> Row {
    Row {
        group,
        name: name.to_owned(),
        value,
        kind,
        expression: None,
    }
}
/// The key a property's expression is stored under: constraints as FreeCAD's
/// `Constraints.<name>`, everything else by its property name.
pub(crate) fn expression_key(row: &Row) -> String {
    if row.group == "Constraints" {
        format!("Constraints.{}", row.name)
    } else if row.name == "Attachment Offset" {
        "AttachmentOffset".into()
    } else {
        row.name.clone()
    }
}
fn mm(v: f64) -> String {
    format!("{} mm", fmt_num(v, 2))
}
fn deg(v: f64) -> String {
    format!("{} °", fmt_num(v, 2))
}
fn yes_no(b: bool) -> String {
    if b { "true" } else { "false" }.into()
}
const EXTENTS: [&str; 3] = ["Length", "TwoLengths", "ThroughAll"];
fn extent_name(e: Extent) -> &'static str {
    match e {
        Extent::Length => "Length",
        Extent::TwoLengths => "TwoLengths",
        Extent::ThroughAll => "ThroughAll",
    }
}
fn axis_names() -> Vec<String> {
    ["V_Axis", "H_Axis", "N_Axis", "X_Axis", "Y_Axis", "Z_Axis"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}
fn axis_name(a: &AxisRef) -> String {
    match a {
        AxisRef::SketchV => "V_Axis".into(),
        AxisRef::SketchH => "H_Axis".into(),
        AxisRef::SketchNormal => "N_Axis".into(),
        AxisRef::X => "X_Axis".into(),
        AxisRef::Y => "Y_Axis".into(),
        AxisRef::Z => "Z_Axis".into(),
        AxisRef::SketchLine(i) => format!("Edge{}", i + 1),
        AxisRef::Edge(r) => r.name.clone(),
        AxisRef::Datum(d) => d.clone(),
    }
}
fn parse_axis(v: &str) -> Result<AxisRef, String> {
    Ok(match v {
        "V_Axis" => AxisRef::SketchV,
        "H_Axis" => AxisRef::SketchH,
        "N_Axis" => AxisRef::SketchNormal,
        "X_Axis" => AxisRef::X,
        "Y_Axis" => AxisRef::Y,
        "Z_Axis" => AxisRef::Z,
        _ => return Err(format!("unknown axis {v}")),
    })
}

impl Cad {
    /// Rows of the property editor for `object` on the current tab.
    pub fn property_rows(&self, object: &str) -> Vec<Row> {
        let Some(o) = self.doc.get(object) else {
            return vec![];
        };
        if self.props == PropTab::View {
            let vp = self.view_props(object);
            let mut v = vec![row(
                "Display Options",
                "Visibility",
                yes_no(o.visible),
                Kind::Bool(o.visible),
            )];
            if o.feature.is_solid_feature()
                || matches!(
                    o.feature,
                    Feature::Body { .. } | Feature::Mesh { .. } | Feature::Part { .. }
                )
            {
                v.push(row(
                    "Display Options",
                    "Display Mode",
                    vp.display.label().into(),
                    Kind::Enum(
                        DisplayMode::ALL
                            .iter()
                            .map(|d| d.label().to_owned())
                            .collect(),
                    ),
                ));
                v.push(row(
                    "Object Style",
                    "Transparency",
                    vp.transparency.to_string(),
                    Kind::Number,
                ));
            }
            v.push(row(
                "Object Style",
                "Line Width",
                format!("{} px", fmt_num(vp.line_width, 1)),
                Kind::Number,
            ));
            return v;
        }
        let mut v = vec![row("Base", "Label", o.label.clone(), Kind::Text)];
        match &o.feature {
            Feature::Body { group, tip } => {
                v.push(row(
                    "Base",
                    "Tip",
                    tip.as_ref().map(|t| self.label_of(t)).unwrap_or_default(),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Base",
                    "Group",
                    format!("[{} objects]", group.len()),
                    Kind::ReadOnly,
                ));
            }
            Feature::Primitive { shape, offset, .. } => {
                for (name, value) in shape.dimensions() {
                    v.push(row(
                        shape.name(),
                        name,
                        if name == "Angle" {
                            deg(value)
                        } else {
                            mm(value)
                        },
                        Kind::Number,
                    ));
                }
                v.push(row(
                    "Attachment",
                    "Support",
                    self.support_text(&o.feature),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Attachment",
                    "Attachment Offset",
                    mm(*offset),
                    Kind::Number,
                ));
            }
            Feature::Boolean { kind, bodies } => {
                v.push(row(
                    "Boolean",
                    "Type",
                    kind.label().into(),
                    Kind::Enum(BoolType::ALL.iter().map(|b| b.label().to_owned()).collect()),
                ));
                v.push(row(
                    "Boolean",
                    "Group",
                    bodies
                        .iter()
                        .map(|b| self.label_of(b))
                        .collect::<Vec<_>>()
                        .join(", "),
                    Kind::ReadOnly,
                ));
            }
            Feature::DatumPlane { offset, angle, .. }
            | Feature::DatumLine { offset, angle, .. } => {
                v.push(row(
                    "Attachment",
                    "Support",
                    self.support_text(&o.feature),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Attachment",
                    "Attachment Offset",
                    mm(*offset),
                    Kind::Number,
                ));
                v.push(row("Attachment", "Angle", deg(*angle), Kind::Number));
            }
            Feature::DatumPoint { offset, x, y, .. } => {
                v.push(row(
                    "Attachment",
                    "Support",
                    self.support_text(&o.feature),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Attachment",
                    "Attachment Offset",
                    mm(*offset),
                    Kind::Number,
                ));
                v.push(row("Attachment", "X", mm(*x), Kind::Number));
                v.push(row("Attachment", "Y", mm(*y), Kind::Number));
            }
            Feature::Sketch {
                sketch,
                support: _,
                offset,
            } => {
                let s = self.support_text(&o.feature);
                v.push(row("Attachment", "Support", s, Kind::ReadOnly));
                v.push(row(
                    "Attachment",
                    "Attachment Offset",
                    mm(*offset),
                    Kind::Number,
                ));
                for (i, c) in sketch.constraints.iter().enumerate() {
                    if c.kind.is_dimensional() {
                        let name = if c.name.is_empty() {
                            format!("Constraint{}", i + 1)
                        } else {
                            c.name.clone()
                        };
                        v.push(row(
                            "Constraints",
                            &name,
                            super::commands::value_text(c),
                            if c.driving {
                                Kind::Number
                            } else {
                                Kind::ReadOnly
                            },
                        ));
                    }
                }
            }
            Feature::Pad {
                profile,
                extent,
                length,
                length2,
                midplane,
                reversed,
            }
            | Feature::Pocket {
                profile,
                extent,
                length,
                length2,
                midplane,
                reversed,
            } => {
                v.push(row(
                    "Base",
                    "Profile",
                    self.label_of(profile),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Base",
                    "Type",
                    extent_name(*extent).into(),
                    Kind::Enum(EXTENTS.iter().map(|s| (*s).into()).collect()),
                ));
                v.push(row("Base", "Length", mm(*length), Kind::Number));
                v.push(row("Base", "Length2", mm(*length2), Kind::Number));
                v.push(row(
                    "Base",
                    "Midplane",
                    yes_no(*midplane),
                    Kind::Bool(*midplane),
                ));
                v.push(row(
                    "Base",
                    "Reversed",
                    yes_no(*reversed),
                    Kind::Bool(*reversed),
                ));
            }
            Feature::Revolution {
                profile,
                axis,
                angle,
                midplane,
                reversed,
            }
            | Feature::Groove {
                profile,
                axis,
                angle,
                midplane,
                reversed,
            } => {
                v.push(row(
                    "Base",
                    "Profile",
                    self.label_of(profile),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Base",
                    "ReferenceAxis",
                    axis_name(axis),
                    Kind::Enum(axis_names()),
                ));
                v.push(row("Base", "Angle", deg(*angle), Kind::Number));
                v.push(row(
                    "Base",
                    "Midplane",
                    yes_no(*midplane),
                    Kind::Bool(*midplane),
                ));
                v.push(row(
                    "Base",
                    "Reversed",
                    yes_no(*reversed),
                    Kind::Bool(*reversed),
                ));
            }
            Feature::Fillet {
                edges,
                radius,
                all_edges,
            } => {
                v.push(row(
                    "Base",
                    "Base",
                    edges
                        .iter()
                        .map(|e| e.name.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                    Kind::ReadOnly,
                ));
                v.push(row("Fillet", "Radius", mm(*radius), Kind::Number));
                v.push(row(
                    "Base",
                    "UseAllEdges",
                    yes_no(*all_edges),
                    Kind::Bool(*all_edges),
                ));
            }
            Feature::Chamfer {
                edges,
                size,
                kind,
                size2,
                angle,
                flip,
                all_edges,
            } => {
                v.push(row(
                    "Base",
                    "Base",
                    edges
                        .iter()
                        .map(|e| e.name.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Chamfer",
                    "ChamferType",
                    kind.label().into(),
                    Kind::Enum(
                        ChamferType::ALL
                            .iter()
                            .map(|c| c.label().to_owned())
                            .collect(),
                    ),
                ));
                v.push(row("Chamfer", "Size", mm(*size), Kind::Number));
                if *kind == ChamferType::TwoDistances {
                    v.push(row("Chamfer", "Size2", mm(*size2), Kind::Number));
                }
                if *kind == ChamferType::DistanceAngle {
                    v.push(row("Chamfer", "Angle", deg(*angle), Kind::Number));
                }
                if *kind != ChamferType::Equal {
                    v.push(row(
                        "Chamfer",
                        "FlipDirection",
                        yes_no(*flip),
                        Kind::Bool(*flip),
                    ));
                }
                v.push(row(
                    "Base",
                    "UseAllEdges",
                    yes_no(*all_edges),
                    Kind::Bool(*all_edges),
                ));
            }
            Feature::Hole {
                profile,
                diameter,
                depth,
                through_all,
                cut,
                drill_point,
            } => {
                v.push(row(
                    "Base",
                    "Profile",
                    self.label_of(profile),
                    Kind::ReadOnly,
                ));
                v.push(row("Hole", "Diameter", mm(*diameter), Kind::Number));
                v.push(row(
                    "Hole",
                    "DepthType",
                    if *through_all {
                        "ThroughAll"
                    } else {
                        "Dimension"
                    }
                    .into(),
                    Kind::Enum(vec!["Dimension".into(), "ThroughAll".into()]),
                ));
                v.push(row("Hole", "Depth", mm(*depth), Kind::Number));
                let cut_name = match cut {
                    HoleCut::None => "None",
                    HoleCut::Counterbore { .. } => "Counterbore",
                    HoleCut::Countersink { .. } => "Countersink",
                };
                v.push(row(
                    "Hole",
                    "HoleCutType",
                    cut_name.into(),
                    Kind::Enum(vec![
                        "None".into(),
                        "Counterbore".into(),
                        "Countersink".into(),
                    ]),
                ));
                v.push(row(
                    "Hole",
                    "DrillPoint",
                    if drill_point.is_some() {
                        "Angled"
                    } else {
                        "Flat"
                    }
                    .into(),
                    Kind::Enum(vec!["Flat".into(), "Angled".into()]),
                ));
            }
            Feature::Mirrored { originals, plane } => {
                v.push(row(
                    "Base",
                    "Originals",
                    originals
                        .iter()
                        .map(|o| self.label_of(o))
                        .collect::<Vec<_>>()
                        .join(", "),
                    Kind::ReadOnly,
                ));
                let name = match plane {
                    PlaneRef::SketchV => "V_Axis".to_owned(),
                    PlaneRef::SketchH => "H_Axis".to_owned(),
                    PlaneRef::Base(p) => p.name().to_owned(),
                    PlaneRef::Face(r) => r.name.clone(),
                    PlaneRef::Datum(d) => d.clone(),
                };
                v.push(row(
                    "Base",
                    "MirrorPlane",
                    name,
                    Kind::Enum(
                        ["V_Axis", "H_Axis", "XY_Plane", "XZ_Plane", "YZ_Plane"]
                            .iter()
                            .map(|s| (*s).into())
                            .collect(),
                    ),
                ));
            }
            Feature::LinearPattern {
                originals,
                direction,
                length,
                occurrences,
                reversed,
            } => {
                v.push(row(
                    "Base",
                    "Originals",
                    originals
                        .iter()
                        .map(|o| self.label_of(o))
                        .collect::<Vec<_>>()
                        .join(", "),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Base",
                    "Direction",
                    axis_name(direction),
                    Kind::Enum(axis_names()),
                ));
                v.push(row(
                    "Base",
                    "Reversed",
                    yes_no(*reversed),
                    Kind::Bool(*reversed),
                ));
                v.push(row("Base", "Length", mm(*length), Kind::Number));
                v.push(row(
                    "Base",
                    "Occurrences",
                    occurrences.to_string(),
                    Kind::Number,
                ));
            }
            Feature::PolarPattern {
                originals,
                axis,
                angle,
                occurrences,
                reversed,
            } => {
                v.push(row(
                    "Base",
                    "Originals",
                    originals
                        .iter()
                        .map(|o| self.label_of(o))
                        .collect::<Vec<_>>()
                        .join(", "),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Base",
                    "Axis",
                    axis_name(axis),
                    Kind::Enum(axis_names()),
                ));
                v.push(row(
                    "Base",
                    "Reversed",
                    yes_no(*reversed),
                    Kind::Bool(*reversed),
                ));
                v.push(row("Base", "Angle", deg(*angle), Kind::Number));
                v.push(row(
                    "Base",
                    "Occurrences",
                    occurrences.to_string(),
                    Kind::Number,
                ));
            }
            Feature::Part { solid } => {
                v.push(row(
                    "Shape",
                    "Faces",
                    solid.faces.len().to_string(),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Shape",
                    "Edges",
                    solid
                        .edges
                        .iter()
                        .filter(|e| !e.degenerate)
                        .count()
                        .to_string(),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Shape",
                    "Vertexes",
                    solid.vertices.len().to_string(),
                    Kind::ReadOnly,
                ));
            }
            Feature::Mesh { mesh } => {
                v.push(row(
                    "Mesh",
                    "Points",
                    mesh.verts.len().to_string(),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Mesh",
                    "Faces",
                    mesh.tris.len().to_string(),
                    Kind::ReadOnly,
                ));
                v.push(row(
                    "Mesh",
                    "Watertight",
                    yes_no(mesh.is_watertight()),
                    Kind::ReadOnly,
                ));
            }
        }
        for r in &mut v {
            if r.kind == Kind::Number {
                r.expression = o.expressions.get(&expression_key(r)).cloned();
            }
        }
        v
    }

    fn support_text(&self, f: &Feature) -> String {
        match f.support() {
            Some(Support::Plane { plane }) => plane.name().to_owned(),
            Some(Support::Face { feature, face }) => {
                format!("{}:{}", self.label_of(feature), face.name)
            }
            Some(Support::Datum { datum }) => self.label_of(datum),
            None => String::new(),
        }
    }

    /// The expression a property is bound to, or nothing.
    pub(crate) fn expression_of(&self, object: &str, name: &str) -> String {
        let key = self
            .property_rows(object)
            .into_iter()
            .find(|r| r.name == name)
            .map(|r| expression_key(&r))
            .unwrap_or_else(|| name.to_owned());
        self.doc
            .get(object)
            .and_then(|o| o.expressions.get(&key))
            .cloned()
            .unwrap_or_default()
    }

    /// Bind a property to an expression (an empty text unbinds it); the document is
    /// recomputed with the expression evaluated, and a bad expression is refused.
    pub(crate) fn set_expression(
        &mut self,
        object: &str,
        name: &str,
        text: &str,
    ) -> Result<(), String> {
        let row = self
            .property_rows(object)
            .into_iter()
            .find(|r| r.name == name)
            .ok_or_else(|| format!("{object} has no property {name}"))?;
        if row.kind != Kind::Number {
            return Err(format!("{name} cannot be bound to an expression"));
        }
        let key = expression_key(&row);
        let text = text.trim().trim_start_matches('=').trim().to_owned();
        let mut doc = self.doc.clone();
        let o = doc.get_mut(object).ok_or("no such object")?;
        if text.is_empty() {
            if o.expressions.remove(&key).is_none() {
                return Ok(());
            }
        } else {
            cw_cad::expr::parse(&text)?;
            o.expressions.insert(key.clone(), text.clone());
            let errors = cw_cad::document::evaluate_expressions(&mut doc);
            if let Some(e) = errors.get(&(object.to_owned(), key.clone())) {
                return Err(format!("{name}: {e}"));
            }
        }
        self.checkpoint(&format!("Set expression {name}"));
        self.doc = doc;
        self.recompute();
        self.status = if text.is_empty() {
            format!("{name} is no longer bound")
        } else {
            format!("{name} = {text}")
        };
        Ok(())
    }

    pub(crate) fn property_text(&self, object: &str, name: &str) -> Result<String, String> {
        self.property_rows(object)
            .into_iter()
            .find(|r| r.name == name)
            .map(|r| r.value)
            .ok_or_else(|| format!("{object} has no property {name}"))
    }

    /// A click on a property's value: edit a number or text, flip a boolean, drop down
    /// the choices of an enumeration.
    pub(crate) fn property_click(&mut self, name: &str) -> Result<Vec<AppEffect>, String> {
        let object = self.selected_object().ok_or("Select an object")?.to_owned();
        let row = self
            .property_rows(&object)
            .into_iter()
            .find(|r| r.name == name)
            .ok_or_else(|| format!("no property {name}"))?;
        if row.expression.is_some() {
            // A bound value is set by its expression: the click edits that.
            self.field = Some(Field {
                target: FieldTarget::Expression {
                    object,
                    name: name.to_owned(),
                },
                text: row.expression.unwrap_or_default(),
                replace: true,
            });
            return Ok(vec![]);
        }
        match row.kind {
            Kind::ReadOnly => Err(format!("{name} is read-only")),
            Kind::Bool(on) => {
                self.set_property(&object, name, if on { "false" } else { "true" })?;
                Ok(vec![])
            }
            Kind::Enum(_) => {
                let id = format!("dd:prop/{name}");
                self.menu = if self.menu.as_deref() == Some(id.as_str()) {
                    None
                } else {
                    Some(id)
                };
                Ok(vec![])
            }
            Kind::Number | Kind::Text => {
                self.field = Some(Field {
                    target: if name == "Label" && row.kind == Kind::Text {
                        FieldTarget::Label { object }
                    } else {
                        FieldTarget::Property {
                            object,
                            name: name.to_owned(),
                        }
                    },
                    text: row.value,
                    replace: true,
                });
                Ok(vec![])
            }
        }
    }

    /// Set a property from text; recompute. Undoable.
    pub(crate) fn set_property(
        &mut self,
        object: &str,
        name: &str,
        text: &str,
    ) -> Result<(), String> {
        let view_tab = self.props == PropTab::View;
        if view_tab || name == "Visibility" {
            let mut vp = self.view_props(object);
            match name {
                "Visibility" => {
                    let o = self.doc.get_mut(object).ok_or("no such object")?;
                    o.visible = text == "true";
                }
                "Display Mode" => {
                    vp.display = DisplayMode::ALL
                        .into_iter()
                        .find(|d| d.label() == text)
                        .ok_or("unknown display mode")?;
                }
                "Transparency" => {
                    let n: f64 = text
                        .trim()
                        .trim_end_matches('%')
                        .trim()
                        .parse()
                        .map_err(|_| "Transparency is a percentage")?;
                    if !(0.0..=100.0).contains(&n) {
                        return Err("Transparency must be between 0 and 100".into());
                    }
                    vp.transparency = n as u8;
                }
                "Line Width" => {
                    let n: f64 = text
                        .trim()
                        .trim_end_matches("px")
                        .trim()
                        .parse()
                        .map_err(|_| "Line width is a number of pixels")?;
                    if !(0.5..=10.0).contains(&n) {
                        return Err("Line width must be between 0.5 and 10".into());
                    }
                    vp.line_width = n;
                }
                _ => return Err(format!("no view property {name}")),
            }
            self.view_props.insert(object.to_owned(), vp);
            self.rev += 1;
            return Ok(());
        }
        if name.starts_with("Constraint") || self.is_named_constraint(object, name) {
            return self.set_sketch_dimension(object, name, text);
        }
        let mut doc = self.doc.clone();
        let o = doc.get_mut(object).ok_or("no such object")?;
        if name == "Attachment Offset" {
            let v = parse_quantity(text, false)?;
            match o.feature.attachment_offset_mut() {
                Some(offset) => *offset = v,
                None => return Err("no attachment".into()),
            }
        } else {
            set_feature_property(&mut o.feature, name, text)?;
        }
        self.checkpoint(&format!("Edit {name}"));
        self.doc = doc;
        self.recompute();
        Ok(())
    }

    fn is_named_constraint(&self, object: &str, name: &str) -> bool {
        matches!(self.doc.get(object).map(|o| &o.feature), Some(Feature::Sketch { sketch, .. }) if sketch.constraints.iter().any(|c| c.name == name))
    }

    /// A dimension edited from the property editor re-solves its sketch and recomputes
    /// everything downstream, the way FreeCAD's expression-free constraint edit does.
    fn set_sketch_dimension(&mut self, object: &str, name: &str, text: &str) -> Result<(), String> {
        let mut doc = self.doc.clone();
        let Some(Object {
            feature: Feature::Sketch { sketch, .. },
            ..
        }) = doc.get_mut(object)
        else {
            return Err("not a sketch".into());
        };
        let i = sketch
            .constraints
            .iter()
            .position(|c| c.name == name)
            .or_else(|| {
                name.strip_prefix("Constraint")
                    .and_then(|n| n.parse::<usize>().ok())
                    .map(|n| n - 1)
            })
            .filter(|i| *i < sketch.constraints.len())
            .ok_or_else(|| format!("no constraint {name}"))?;
        let angle = sketch.constraints[i].kind == cw_cad::sketch::ConstraintType::Angle;
        let v = parse_quantity(text, angle)?;
        sketch.constraints[i].value = if angle { cw_cad::math::radians(v) } else { v };
        let report = sketch.solve();
        if !report.solved() {
            return Err(format!("{}: {}", name, report.message()));
        }
        self.checkpoint(&format!("Edit {name}"));
        self.doc = doc;
        self.recompute();
        Ok(())
    }
}

/// Set one Data property of a feature from text (used by the property editor).
pub(crate) fn set_feature_property(f: &mut Feature, name: &str, text: &str) -> Result<(), String> {
    let num = |angle: bool| parse_quantity(text, angle);
    let positive = |v: f64| {
        if v > 0.0 {
            Ok(v)
        } else {
            Err(format!("{name} must be positive"))
        }
    };
    let flag = || match text {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{name} is true or false")),
    };
    match (f, name) {
        (Feature::Pad { extent, .. } | Feature::Pocket { extent, .. }, "Type") => {
            *extent = match text {
                "Length" => Extent::Length,
                "TwoLengths" => Extent::TwoLengths,
                "ThroughAll" => Extent::ThroughAll,
                _ => return Err("unknown type".into()),
            }
        }
        (Feature::Pad { length, .. } | Feature::Pocket { length, .. }, "Length") => {
            *length = positive(num(false)?)?
        }
        (Feature::Pad { length2, .. } | Feature::Pocket { length2, .. }, "Length2") => {
            *length2 = num(false)?
        }
        (
            Feature::Pad { midplane, .. }
            | Feature::Pocket { midplane, .. }
            | Feature::Revolution { midplane, .. }
            | Feature::Groove { midplane, .. },
            "Midplane",
        ) => *midplane = flag()?,
        (
            Feature::Pad { reversed, .. }
            | Feature::Pocket { reversed, .. }
            | Feature::Revolution { reversed, .. }
            | Feature::Groove { reversed, .. }
            | Feature::LinearPattern { reversed, .. }
            | Feature::PolarPattern { reversed, .. },
            "Reversed",
        ) => *reversed = flag()?,
        (Feature::Revolution { axis, .. } | Feature::Groove { axis, .. }, "ReferenceAxis") => {
            *axis = parse_axis(text)?
        }
        (
            Feature::Revolution { angle, .. }
            | Feature::Groove { angle, .. }
            | Feature::PolarPattern { angle, .. },
            "Angle",
        ) => {
            let v = num(true)?;
            if v == 0.0 || v.abs() > 360.0 {
                return Err("The angle must be between 0 and 360°".into());
            }
            *angle = v;
        }
        (Feature::Fillet { radius, .. }, "Radius") => *radius = positive(num(false)?)?,
        (Feature::Chamfer { size, .. }, "Size") => *size = positive(num(false)?)?,
        (Feature::Chamfer { size2, .. }, "Size2") => *size2 = positive(num(false)?)?,
        (Feature::Chamfer { angle, .. }, "Angle") => {
            let v = num(true)?;
            if !(v > 0.0 && v < 180.0) {
                return Err("Angle must be greater than 0 and less than 180".into());
            }
            *angle = v;
        }
        (Feature::Chamfer { flip, .. }, "FlipDirection") => *flip = flag()?,
        (Feature::Chamfer { kind, .. }, "ChamferType") => {
            *kind = ChamferType::ALL
                .into_iter()
                .find(|c| c.label() == text)
                .ok_or("unknown chamfer type")?
        }
        (Feature::Fillet { all_edges, .. } | Feature::Chamfer { all_edges, .. }, "UseAllEdges") => {
            *all_edges = flag()?
        }
        (Feature::Hole { diameter, .. }, "Diameter") => *diameter = positive(num(false)?)?,
        (Feature::Hole { depth, .. }, "Depth") => *depth = positive(num(false)?)?,
        (Feature::Hole { through_all, .. }, "DepthType") => *through_all = text == "ThroughAll",
        (Feature::Hole { cut, diameter, .. }, "HoleCutType") => {
            *cut = match text {
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
            *drill_point = if text == "Angled" { Some(118.0) } else { None }
        }
        (Feature::Mirrored { plane, .. }, "MirrorPlane") => {
            *plane = match text {
                "V_Axis" => PlaneRef::SketchV,
                "H_Axis" => PlaneRef::SketchH,
                other => {
                    PlaneRef::Base(cw_cad::document::plane_by_name(other).ok_or("unknown plane")?)
                }
            }
        }
        (Feature::LinearPattern { direction, .. }, "Direction") => *direction = parse_axis(text)?,
        (Feature::PolarPattern { axis, .. }, "Axis") => *axis = parse_axis(text)?,
        (Feature::LinearPattern { length, .. }, "Length") => *length = positive(num(false)?)?,
        (
            Feature::LinearPattern { occurrences, .. } | Feature::PolarPattern { occurrences, .. },
            "Occurrences",
        ) => {
            let n: u32 = text
                .trim()
                .parse()
                .map_err(|_| "Occurrences is a whole number")?;
            if !(2..=100).contains(&n) {
                return Err("Occurrences must be between 2 and 100".into());
            }
            *occurrences = n;
        }
        (Feature::Boolean { kind, .. }, "Type") => {
            *kind = BoolType::by_name(text).ok_or("unknown boolean type")?
        }
        (
            f @ (Feature::Primitive { .. }
            | Feature::DatumPlane { .. }
            | Feature::DatumLine { .. }
            | Feature::DatumPoint { .. }),
            name,
        ) => f.set_number(name, num(name == "Angle")?)?,
        (_, other) => return Err(format!("{other} cannot be changed here")),
    }
    Ok(())
}
