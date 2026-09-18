//! A Part Design document: bodies holding an ordered feature list, sketches attached to
//! base planes or to faces, and imported meshes. [`recompute`] evaluates every feature
//! from its parameters in order, so editing anything upstream flows downstream.
//!
//! Names and types follow FreeCAD (`Body`, `Sketch001`, `PartDesign::Pad`, `Face6`), and
//! a reference to a face or edge of an earlier result is kept as FreeCAD's element name
//! plus a geometric hint, so it survives the renumbering a parameter change causes
//! (FreeCAD 1.0's answer to the topological naming problem is its element map; this is
//! the same idea at this kernel's scale).
use crate::csg::{self, Op};
use crate::math::{self, Frame, Xform, TAU, V2, V3};
use crate::mesh::{Mesh, Surface};
use crate::sketch::{profile, Geom, Sketch, SolveReport};
use crate::solid::{self, Dress, EdgeKind, Topology};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BasePlane {
    #[serde(rename = "XY_Plane")]
    XY,
    #[serde(rename = "XZ_Plane")]
    XZ,
    #[serde(rename = "YZ_Plane")]
    YZ,
}
impl BasePlane {
    pub fn frame(self) -> Frame {
        match self {
            BasePlane::XY => Frame::XY,
            BasePlane::XZ => Frame::XZ,
            BasePlane::YZ => Frame::YZ,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            BasePlane::XY => "XY_Plane",
            BasePlane::XZ => "XZ_Plane",
            BasePlane::YZ => "YZ_Plane",
        }
    }
}

/// A face, edge or vertex of an earlier feature's shape: FreeCAD's element name, and
/// where it was, to find it again after the shape changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubRef {
    /// `Face6`, `Edge3`, `Vertex2` (1-based, as FreeCAD numbers them).
    pub name: String,
    pub center: V3,
    /// Face normal or edge direction; zero when not meaningful.
    #[serde(default)]
    pub dir: V3,
}
impl SubRef {
    pub fn index(&self) -> Option<usize> {
        let digits = self
            .name
            .trim_start_matches(|c: char| c.is_ascii_alphabetic());
        digits
            .parse::<usize>()
            .ok()
            .filter(|i| *i > 0)
            .map(|i| i - 1)
    }
    pub fn is_face(&self) -> bool {
        self.name.starts_with("Face")
    }
    pub fn is_edge(&self) -> bool {
        self.name.starts_with("Edge")
    }
    pub fn face(t: &Topology, m: &Mesh, i: usize) -> SubRef {
        SubRef {
            name: format!("Face{}", i + 1),
            center: t.faces[i].centroid,
            dir: face_normal(m, t, i),
        }
    }
    pub fn edge(t: &Topology, m: &Mesh, i: usize) -> SubRef {
        let e = &t.edges[i];
        SubRef {
            name: format!("Edge{}", i + 1),
            center: edge_mid(m, t, i),
            dir: if e.kind == EdgeKind::Line {
                (m.verts[*e.verts.last().unwrap() as usize] - m.verts[e.verts[0] as usize]).norm()
            } else {
                e.circle.map(|c| c.2).unwrap_or(V3::ZERO)
            },
        }
    }
}
pub fn face_normal(m: &Mesh, t: &Topology, i: usize) -> V3 {
    match &m.surfaces[t.faces[i].surface as usize] {
        Surface::Plane { normal, .. } => *normal,
        Surface::Cylinder { axis, .. }
        | Surface::Cone { axis, .. }
        | Surface::Revolved { axis, .. } => *axis,
        Surface::Facets => V3::ZERO,
    }
}
pub fn edge_mid(m: &Mesh, t: &Topology, i: usize) -> V3 {
    let e = &t.edges[i];
    // The point halfway along the polyline.
    let pts: Vec<V3> = e.verts.iter().map(|v| m.verts[*v as usize]).collect();
    let half = e.length / 2.0;
    let mut run = 0.0;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if run + l >= half && l > 0.0 {
            return w[0].lerp(w[1], (half - run) / l);
        }
        run += l;
    }
    pts[0]
}

/// Find a referenced face or edge in a (possibly changed) shape.
pub fn resolve(r: &SubRef, m: &Mesh, t: &Topology) -> Option<usize> {
    let scale = m.bounds().map(|b| b.diagonal()).unwrap_or(1.0).max(1.0);
    if r.is_face() {
        let score = |i: usize| {
            let n = face_normal(m, t, i);
            let turn = if r.dir.len() > 0.0 {
                (n - r.dir).len()
            } else {
                0.0
            };
            (turn > 1e-3, t.faces[i].centroid.dist(r.center))
        };
        if let Some(i) = r.index().filter(|i| *i < t.faces.len()) {
            let (turned, d) = score(i);
            if !turned && d < 1e-6 * scale {
                return Some(i);
            }
        }
        (0..t.faces.len())
            .map(|i| (score(i), i))
            .filter(|((turned, d), _)| !turned && *d < scale * 2.0)
            .min_by(|a, b| a.0 .1.total_cmp(&b.0 .1).then(a.1.cmp(&b.1)))
            .map(|(_, i)| i)
    } else if r.is_edge() {
        let score = |i: usize| {
            let e = SubRef::edge(t, m, i);
            let parallel = r.dir.len() == 0.0 || e.dir.cross(r.dir).len() < 1e-3;
            (!parallel, e.center.dist(r.center))
        };
        if let Some(i) = r.index().filter(|i| *i < t.edges.len()) {
            let (turned, d) = score(i);
            if !turned && d < 1e-6 * scale {
                return Some(i);
            }
        }
        (0..t.edges.len())
            .map(|i| (score(i), i))
            .filter(|((turned, d), _)| !turned && *d < scale)
            .min_by(|a, b| a.0 .1.total_cmp(&b.0 .1).then(a.1.cmp(&b.1)))
            .map(|(_, i)| i)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "Type")]
pub enum Support {
    Plane {
        plane: BasePlane,
    },
    /// A planar face of an earlier feature's result.
    Face {
        feature: String,
        face: SubRef,
    },
}

/// How far a Pad or Pocket goes: FreeCAD's `Type` property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Extent {
    Length,
    TwoLengths,
    ThroughAll,
}

/// An axis for revolutions and patterns (FreeCAD's `ReferenceAxis`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AxisRef {
    #[serde(rename = "V_Axis")]
    SketchV,
    #[serde(rename = "H_Axis")]
    SketchH,
    /// The sketch's normal through its origin.
    #[serde(rename = "N_Axis")]
    SketchNormal,
    #[serde(rename = "X_Axis")]
    X,
    #[serde(rename = "Y_Axis")]
    Y,
    #[serde(rename = "Z_Axis")]
    Z,
    /// A (construction) line of the feature's own sketch.
    SketchLine(i32),
    /// A straight edge of the shape the feature starts from.
    Edge(SubRef),
}
impl AxisRef {
    pub fn label(&self) -> String {
        match self {
            AxisRef::SketchV => "Vertical sketch axis".into(),
            AxisRef::SketchH => "Horizontal sketch axis".into(),
            AxisRef::SketchNormal => "Normal sketch axis".into(),
            AxisRef::X => "X axis".into(),
            AxisRef::Y => "Y axis".into(),
            AxisRef::Z => "Z axis".into(),
            AxisRef::SketchLine(i) => format!("Construction line {}", i + 1),
            AxisRef::Edge(r) => r.name.clone(),
        }
    }
}
/// A mirror plane (FreeCAD's `MirrorPlane`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlaneRef {
    /// The plane through the sketch's vertical axis and its normal.
    #[serde(rename = "V_Axis")]
    SketchV,
    #[serde(rename = "H_Axis")]
    SketchH,
    Base(BasePlane),
    Face(SubRef),
}
impl PlaneRef {
    pub fn label(&self) -> String {
        match self {
            PlaneRef::SketchV => "Vertical sketch axis".into(),
            PlaneRef::SketchH => "Horizontal sketch axis".into(),
            PlaneRef::Base(p) => p.name().into(),
            PlaneRef::Face(r) => r.name.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "Type")]
pub enum HoleCut {
    None,
    Counterbore { diameter: f64, depth: f64 },
    Countersink { diameter: f64, angle: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "TypeId")]
pub enum Feature {
    #[serde(rename = "PartDesign::Body")]
    Body {
        #[serde(rename = "Group")]
        group: Vec<String>,
        #[serde(rename = "Tip")]
        tip: Option<String>,
    },
    #[serde(rename = "Sketcher::SketchObject")]
    Sketch {
        #[serde(rename = "Sketch")]
        sketch: Sketch,
        #[serde(rename = "Support")]
        support: Support,
        #[serde(rename = "AttachmentOffset", default)]
        offset: f64,
    },
    #[serde(rename = "PartDesign::Pad")]
    Pad {
        #[serde(rename = "Profile")]
        profile: String,
        #[serde(rename = "Type")]
        extent: Extent,
        #[serde(rename = "Length")]
        length: f64,
        #[serde(rename = "Length2", default)]
        length2: f64,
        #[serde(rename = "Midplane", default)]
        midplane: bool,
        #[serde(rename = "Reversed", default)]
        reversed: bool,
    },
    #[serde(rename = "PartDesign::Pocket")]
    Pocket {
        #[serde(rename = "Profile")]
        profile: String,
        #[serde(rename = "Type")]
        extent: Extent,
        #[serde(rename = "Length")]
        length: f64,
        #[serde(rename = "Length2", default)]
        length2: f64,
        #[serde(rename = "Midplane", default)]
        midplane: bool,
        #[serde(rename = "Reversed", default)]
        reversed: bool,
    },
    #[serde(rename = "PartDesign::Revolution")]
    Revolution {
        #[serde(rename = "Profile")]
        profile: String,
        #[serde(rename = "ReferenceAxis")]
        axis: AxisRef,
        /// Degrees.
        #[serde(rename = "Angle")]
        angle: f64,
        #[serde(rename = "Midplane", default)]
        midplane: bool,
        #[serde(rename = "Reversed", default)]
        reversed: bool,
    },
    #[serde(rename = "PartDesign::Groove")]
    Groove {
        #[serde(rename = "Profile")]
        profile: String,
        #[serde(rename = "ReferenceAxis")]
        axis: AxisRef,
        #[serde(rename = "Angle")]
        angle: f64,
        #[serde(rename = "Midplane", default)]
        midplane: bool,
        #[serde(rename = "Reversed", default)]
        reversed: bool,
    },
    #[serde(rename = "PartDesign::Fillet")]
    Fillet {
        #[serde(rename = "Base")]
        edges: Vec<SubRef>,
        #[serde(rename = "Radius")]
        radius: f64,
    },
    #[serde(rename = "PartDesign::Chamfer")]
    Chamfer {
        #[serde(rename = "Base")]
        edges: Vec<SubRef>,
        #[serde(rename = "Size")]
        size: f64,
    },
    #[serde(rename = "PartDesign::Hole")]
    Hole {
        #[serde(rename = "Profile")]
        profile: String,
        #[serde(rename = "Diameter")]
        diameter: f64,
        #[serde(rename = "Depth")]
        depth: f64,
        #[serde(rename = "ThroughAll", default)]
        through_all: bool,
        #[serde(rename = "HoleCutType")]
        cut: HoleCut,
        /// Drill point angle in degrees; `None` for a flat bottom.
        #[serde(rename = "DrillPointAngle", default)]
        drill_point: Option<f64>,
    },
    #[serde(rename = "PartDesign::Mirrored")]
    Mirrored {
        #[serde(rename = "Originals")]
        originals: Vec<String>,
        #[serde(rename = "MirrorPlane")]
        plane: PlaneRef,
    },
    #[serde(rename = "PartDesign::LinearPattern")]
    LinearPattern {
        #[serde(rename = "Originals")]
        originals: Vec<String>,
        #[serde(rename = "Direction")]
        direction: AxisRef,
        #[serde(rename = "Length")]
        length: f64,
        #[serde(rename = "Occurrences")]
        occurrences: u32,
        #[serde(rename = "Reversed", default)]
        reversed: bool,
    },
    #[serde(rename = "PartDesign::PolarPattern")]
    PolarPattern {
        #[serde(rename = "Originals")]
        originals: Vec<String>,
        #[serde(rename = "Axis")]
        axis: AxisRef,
        /// Degrees.
        #[serde(rename = "Angle")]
        angle: f64,
        #[serde(rename = "Occurrences")]
        occurrences: u32,
        #[serde(rename = "Reversed", default)]
        reversed: bool,
    },
    /// An imported triangle mesh, outside any body (FreeCAD's `Mesh::Feature`).
    #[serde(rename = "Mesh::Feature")]
    Mesh {
        #[serde(rename = "Mesh")]
        mesh: Arc<Mesh>,
    },
}
impl Feature {
    pub fn type_id(&self) -> &'static str {
        match self {
            Feature::Body { .. } => "PartDesign::Body",
            Feature::Sketch { .. } => "Sketcher::SketchObject",
            Feature::Pad { .. } => "PartDesign::Pad",
            Feature::Pocket { .. } => "PartDesign::Pocket",
            Feature::Revolution { .. } => "PartDesign::Revolution",
            Feature::Groove { .. } => "PartDesign::Groove",
            Feature::Fillet { .. } => "PartDesign::Fillet",
            Feature::Chamfer { .. } => "PartDesign::Chamfer",
            Feature::Hole { .. } => "PartDesign::Hole",
            Feature::Mirrored { .. } => "PartDesign::Mirrored",
            Feature::LinearPattern { .. } => "PartDesign::LinearPattern",
            Feature::PolarPattern { .. } => "PartDesign::PolarPattern",
            Feature::Mesh { .. } => "Mesh::Feature",
        }
    }
    /// The base name FreeCAD gives a new object of this type.
    pub fn base_name(&self) -> &'static str {
        match self {
            Feature::Body { .. } => "Body",
            Feature::Sketch { .. } => "Sketch",
            Feature::Pad { .. } => "Pad",
            Feature::Pocket { .. } => "Pocket",
            Feature::Revolution { .. } => "Revolution",
            Feature::Groove { .. } => "Groove",
            Feature::Fillet { .. } => "Fillet",
            Feature::Chamfer { .. } => "Chamfer",
            Feature::Hole { .. } => "Hole",
            Feature::Mirrored { .. } => "Mirrored",
            Feature::LinearPattern { .. } => "LinearPattern",
            Feature::PolarPattern { .. } => "PolarPattern",
            Feature::Mesh { .. } => "Mesh",
        }
    }
    /// The sketch a sketch-based feature consumes.
    pub fn profile(&self) -> Option<&str> {
        match self {
            Feature::Pad { profile, .. }
            | Feature::Pocket { profile, .. }
            | Feature::Revolution { profile, .. }
            | Feature::Groove { profile, .. }
            | Feature::Hole { profile, .. } => Some(profile),
            _ => None,
        }
    }
    pub fn originals(&self) -> &[String] {
        match self {
            Feature::Mirrored { originals, .. }
            | Feature::LinearPattern { originals, .. }
            | Feature::PolarPattern { originals, .. } => originals,
            _ => &[],
        }
    }
    /// Solid features that change a body's shape (everything but bodies, sketches, meshes).
    pub fn is_solid_feature(&self) -> bool {
        !matches!(
            self,
            Feature::Body { .. } | Feature::Sketch { .. } | Feature::Mesh { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Label")]
    pub label: String,
    #[serde(rename = "Visibility", default = "yes")]
    pub visible: bool,
    #[serde(flatten)]
    pub feature: Feature,
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(rename = "Label")]
    pub label: String,
    #[serde(rename = "Objects")]
    pub objects: Vec<Object>,
}

impl Document {
    pub fn new(label: &str) -> Document {
        Document {
            label: label.into(),
            objects: vec![],
        }
    }
    pub fn get(&self, name: &str) -> Option<&Object> {
        self.objects.iter().find(|o| o.name == name)
    }
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.name == name)
    }
    /// FreeCAD's naming: `Pad`, then `Pad001`, `Pad002`…
    pub fn unique_name(&self, base: &str) -> String {
        if self.get(base).is_none() {
            return base.into();
        }
        (1..)
            .map(|i| format!("{base}{i:03}"))
            .find(|n| self.get(n).is_none())
            .unwrap()
    }
    pub fn add(&mut self, feature: Feature) -> String {
        let name = self.unique_name(feature.base_name());
        self.objects.push(Object {
            label: name.clone(),
            name: name.clone(),
            visible: true,
            feature,
        });
        name
    }
    /// The body a feature belongs to.
    pub fn body_of(&self, name: &str) -> Option<&str> {
        self.objects.iter().find_map(|o| match &o.feature {
            Feature::Body { group, .. } if group.iter().any(|g| g == name) => Some(o.name.as_str()),
            _ => None,
        })
    }
    pub fn bodies(&self) -> Vec<&str> {
        self.objects
            .iter()
            .filter(|o| matches!(o.feature, Feature::Body { .. }))
            .map(|o| o.name.as_str())
            .collect()
    }
    /// Add a feature to a body after its current tip, and make it the tip (for solid
    /// features), as Part Design does.
    pub fn add_to_body(&mut self, body: &str, feature: Feature) -> Result<String, String> {
        let solid = feature.is_solid_feature();
        if !matches!(
            self.get(body).map(|o| &o.feature),
            Some(Feature::Body { .. })
        ) {
            return Err("no such body".into());
        }
        let name = self.add(feature);
        // Like `PartDesign::Body::insertObject(.., getNextSolidFeature())`: after the tip
        // and whatever sketches follow it, before the next solid feature.
        let solid_names: Vec<String> = self
            .objects
            .iter()
            .filter(|o| o.feature.is_solid_feature())
            .map(|o| o.name.clone())
            .collect();
        let Some(Object {
            feature: Feature::Body { group, tip },
            ..
        }) = self.get_mut(body)
        else {
            return Err("no such body".into());
        };
        let after = tip
            .as_ref()
            .and_then(|t| group.iter().position(|g| g == t))
            .map(|p| p + 1)
            .unwrap_or(0);
        let at = (after..group.len())
            .find(|&i| solid_names.contains(&group[i]))
            .unwrap_or(group.len());
        group.insert(at, name.clone());
        if solid {
            *tip = Some(name.clone());
        }
        Ok(name)
    }
    /// Remove objects, from their bodies too. Features that used them are left to
    /// report the missing link on recompute, as FreeCAD does.
    pub fn remove(&mut self, names: &[String]) {
        self.objects.retain(|o| !names.contains(&o.name));
        for o in &mut self.objects {
            if let Feature::Body { group, tip } = &mut o.feature {
                let before = group.clone();
                group.retain(|g| !names.contains(g));
                if tip.as_ref().is_some_and(|t| names.contains(t)) {
                    // The tip moves back to the last surviving solid feature before it.
                    let pos = before
                        .iter()
                        .position(|g| Some(g) == tip.as_ref())
                        .unwrap_or(0);
                    *tip = before[..pos]
                        .iter()
                        .rev()
                        .find(|g| group.contains(g))
                        .cloned();
                }
            }
        }
    }
}

/// What recompute made of one object.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Ok,
    Error(String),
    /// After the body's tip: kept but not part of the shape.
    Inactive,
}

/// A computed shape: the mesh and the faces, edges and vertices recovered from it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub mesh: Mesh,
    pub topo: Topology,
}
impl Shape {
    pub fn new(mesh: Mesh) -> Arc<Shape> {
        let topo = solid::topology(&mesh);
        Arc::new(Shape { mesh, topo })
    }
}

/// Everything recompute derives from a document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub status: BTreeMap<String, Status>,
    /// The body's shape after each solid feature, and each mesh object's mesh.
    pub shapes: BTreeMap<String, Arc<Shape>>,
    /// Each additive/subtractive feature's tool and whether it adds.
    pub tools: BTreeMap<String, (Arc<Mesh>, bool)>,
    /// Where each sketch sits, and the solver's word on it.
    pub frames: BTreeMap<String, Frame>,
    pub reports: BTreeMap<String, SolveReport>,
    /// The shape each body shows: its tip's.
    pub body_shape: BTreeMap<String, Arc<Shape>>,
}
impl Model {
    pub fn error(&self, name: &str) -> Option<&str> {
        match self.status.get(name) {
            Some(Status::Error(e)) => Some(e),
            _ => None,
        }
    }
}

/// Length that reaches through anything in the model.
fn through_all(base: &Mesh, frame: &Frame) -> f64 {
    let b = base.bounds();
    let reach = b
        .map(|b| b.diagonal() + b.center().dist(frame.origin) + b.min.len().max(b.max.len()))
        .unwrap_or(100.0);
    reach * 2.0 + 10.0
}

/// Number of separate solids (vertex-connected triangle sets) in a mesh.
pub fn solids(m: &Mesh) -> usize {
    let mut parent: Vec<u32> = (0..m.verts.len() as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    for t in &m.tris {
        let a = find(&mut parent, t[0]);
        for &v in &t[1..] {
            let b = find(&mut parent, v);
            if a != b {
                parent[b as usize] = a;
            }
        }
    }
    let mut roots: Vec<u32> = m.tris.iter().map(|t| find(&mut parent, t[0])).collect();
    roots.sort_unstable();
    roots.dedup();
    roots.len()
}

fn sketch_axis(frame: &Frame, sketch: &Sketch, axis: &AxisRef) -> Result<(V3, V3), String> {
    Ok(match axis {
        AxisRef::SketchV => (frame.origin, frame.y),
        AxisRef::SketchH => (frame.origin, frame.x),
        AxisRef::SketchNormal => (frame.origin, frame.z),
        AxisRef::X => (V3::ZERO, V3::X),
        AxisRef::Y => (V3::ZERO, V3::Y),
        AxisRef::Z => (V3::ZERO, V3::Z),
        AxisRef::SketchLine(i) => match sketch.geom(*i) {
            Some(Geom::Line { a, b }) => {
                let (pa, pb) = (frame.to_world(a), frame.to_world(b));
                (pa, (pb - pa).norm())
            }
            _ => return Err(format!("Construction line {} is not a line", i + 1)),
        },
        AxisRef::Edge(_) => return Err("an edge axis needs a base shape".into()),
    })
}

fn edge_axis(r: &SubRef, base: Option<&Arc<Shape>>) -> Result<(V3, V3), String> {
    let shape = base.ok_or("the axis edge has no shape to belong to")?;
    let i =
        resolve(r, &shape.mesh, &shape.topo).ok_or_else(|| format!("{} was not found", r.name))?;
    let e = &shape.topo.edges[i];
    match (e.kind, e.circle) {
        (EdgeKind::Line, _) => {
            let a = shape.mesh.verts[e.verts[0] as usize];
            let b = shape.mesh.verts[*e.verts.last().unwrap() as usize];
            Ok((a, (b - a).norm()))
        }
        (_, Some((c, _, n))) => Ok((c, n)),
        _ => Err(format!("{} is not straight or circular", r.name)),
    }
}

/// Recompute the whole document. References to faces and edges are re-found in the
/// changed shapes and their hints updated, so the document the caller keeps tracks
/// the model as it evolves.
pub fn recompute(doc: &mut Document) -> Model {
    let mut model = Model::default();
    let bodies: Vec<String> = doc.bodies().into_iter().map(str::to_owned).collect();
    // Sketches outside bodies, and meshes.
    for o in &doc.objects {
        if let Feature::Mesh { mesh } = &o.feature {
            model
                .shapes
                .insert(o.name.clone(), Shape::new((**mesh).clone()));
            model.status.insert(o.name.clone(), Status::Ok);
        }
    }
    for body in bodies {
        let (group, tip) = match &doc.get(&body).unwrap().feature {
            Feature::Body { group, tip } => (group.clone(), tip.clone()),
            _ => unreachable!(),
        };
        let mut current: Option<Arc<Shape>> = None;
        let mut past_tip = false;
        let mut tip_shape: Option<Arc<Shape>> = None;
        for name in &group {
            // Past the tip, solid features wait; sketches are still placed and solved,
            // since the next feature will be built on them.
            let solid = doc.get(name).is_some_and(|o| o.feature.is_solid_feature());
            if past_tip && solid {
                model.status.insert(name.clone(), Status::Inactive);
                continue;
            }
            let result = feature_step(doc, &mut model, name, current.clone());
            match result {
                Ok(Some(shape)) => {
                    model.shapes.insert(name.clone(), shape.clone());
                    current = Some(shape);
                    model.status.insert(name.clone(), Status::Ok);
                }
                Ok(None) => {
                    model.status.entry(name.clone()).or_insert(Status::Ok);
                }
                Err(e) => {
                    model.status.insert(name.clone(), Status::Error(e));
                }
            }
            if tip.as_deref() == Some(name.as_str()) {
                tip_shape = current.clone();
                past_tip = true;
            }
        }
        if tip.is_none() {
            tip_shape = current.clone();
        }
        let failed = group
            .iter()
            .any(|g| matches!(model.status.get(g), Some(Status::Error(_))));
        model.status.insert(
            body.clone(),
            if failed {
                Status::Error("A feature in this body failed to recompute".into())
            } else {
                Status::Ok
            },
        );
        if let Some(s) = tip_shape {
            model.body_shape.insert(body, s);
        }
    }
    // Loose sketches.
    let loose: Vec<String> = doc
        .objects
        .iter()
        .filter(|o| {
            matches!(o.feature, Feature::Sketch { .. }) && !model.status.contains_key(&o.name)
        })
        .map(|o| o.name.clone())
        .collect();
    for name in loose {
        let r = feature_step(doc, &mut model, &name, None);
        model
            .status
            .insert(name, r.err().map_or(Status::Ok, Status::Error));
    }
    model
}

/// Evaluate one object. Returns the body's new shape for solid features.
fn feature_step(
    doc: &mut Document,
    model: &mut Model,
    name: &str,
    base: Option<Arc<Shape>>,
) -> Result<Option<Arc<Shape>>, String> {
    let object = doc.get(name).ok_or("missing object")?.clone();
    let base_mesh = || base.as_ref().map(|b| b.mesh.clone()).unwrap_or_default();
    let profile_of = |model: &Model, profile: &str| -> Result<(Sketch, Frame), String> {
        let o = doc
            .get(profile)
            .ok_or_else(|| format!("Profile {profile} not found"))?;
        let Feature::Sketch { sketch, .. } = &o.feature else {
            return Err(format!("{profile} is not a sketch"));
        };
        let frame = *model
            .frames
            .get(profile)
            .ok_or_else(|| format!("{profile} could not be placed"))?;
        Ok((sketch.clone(), frame))
    };
    let finish = |mesh: Mesh, what: &str| -> Result<Option<Arc<Shape>>, String> {
        if mesh.is_empty() || mesh.volume() <= 1e-9 {
            return Err(format!("{what}: Resulting shape is empty"));
        }
        if solids(&mesh) > 1 {
            return Err(format!(
                "{what}: Result has multiple solids: that is not currently supported."
            ));
        }
        Ok(Some(Shape::new(mesh)))
    };
    match &object.feature {
        Feature::Body { .. } | Feature::Mesh { .. } => Ok(None),
        Feature::Sketch {
            support,
            offset,
            sketch,
        } => {
            let frame = match support {
                Support::Plane { plane } => plane.frame(),
                Support::Face { feature, face } => {
                    let shape = model
                        .shapes
                        .get(feature)
                        .cloned()
                        .ok_or_else(|| format!("Sketch support {feature} has no shape"))?;
                    let i = resolve(face, &shape.mesh, &shape.topo).ok_or_else(|| {
                        format!("Sketch support {}.{} was not found", feature, face.name)
                    })?;
                    let f = solid::face_frame(&shape.mesh, &shape.topo, i)
                        .ok_or_else(|| format!("{}.{} is not planar", feature, face.name))?;
                    // Remember where the face is now.
                    let fresh = SubRef::face(&shape.topo, &shape.mesh, i);
                    if let Some(Object {
                        feature:
                            Feature::Sketch {
                                support: Support::Face { face, .. },
                                ..
                            },
                        ..
                    }) = doc.get_mut(name)
                    {
                        *face = fresh;
                    }
                    f
                }
            };
            model.frames.insert(name.into(), frame.offset(*offset));
            let mut s = sketch.clone();
            let report = crate::sketch::solver::solve(&mut s, None);
            model.reports.insert(name.into(), report);
            Ok(None)
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
            let pocket = matches!(object.feature, Feature::Pocket { .. });
            let what = if pocket { "Pocket" } else { "Pad" };
            let (sketch, frame) = profile_of(model, profile)?;
            let regions = profile::regions(&sketch)?;
            let base_m = base_mesh();
            if pocket && base_m.is_empty() {
                return Err(
                    "Pocket: Cannot do a pocket without a base shape; create a Pad first".into(),
                );
            }
            let l = match extent {
                Extent::ThroughAll => through_all(&base_m, &frame),
                _ => *length,
            };
            if (l.is_nan() || l <= 0.0) || !l.is_finite() {
                return Err(format!("{what}: Length too small"));
            }
            // A Pad grows along the sketch normal, a Pocket against it.
            let (mut z0, mut z1) = match extent {
                Extent::TwoLengths => (-length2.max(0.0), l),
                _ if *midplane => (-l / 2.0, l / 2.0),
                _ => (0.0, l),
            };
            if pocket != *reversed {
                (z0, z1) = (-z1, -z0);
            }
            let tool = solid::extrude(&sketch, &regions, &frame, z0, z1);
            model
                .tools
                .insert(name.into(), (Arc::new(tool.clone()), !pocket));
            let out = csg::boolean(
                &base_m,
                &tool,
                if pocket { Op::Difference } else { Op::Union },
            );
            finish(out, what)
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
            let groove = matches!(object.feature, Feature::Groove { .. });
            let what = if groove { "Groove" } else { "Revolution" };
            let (sketch, frame) = profile_of(model, profile)?;
            let regions = profile::regions(&sketch)?;
            let (o, d) = match axis {
                AxisRef::Edge(r) => edge_axis(r, base.as_ref())?,
                other => sketch_axis(&frame, &sketch, other)?,
            };
            let a = math::radians(angle.clamp(-360.0, 360.0));
            if a.abs() < 1e-9 {
                return Err(format!("{what}: Angle of revolution too small"));
            }
            let (start, sweep) = if *midplane { (-a / 2.0, a) } else { (0.0, a) };
            let (start, sweep) = if *reversed {
                (-start, -sweep)
            } else {
                (start, sweep)
            };
            let tool = solid::revolve(&sketch, &regions, &frame, o, d, start, sweep)
                .map_err(|e| format!("{what}: {e}"))?;
            model
                .tools
                .insert(name.into(), (Arc::new(tool.clone()), !groove));
            let base_m = base_mesh();
            if groove && base_m.is_empty() {
                return Err("Groove: Cannot do a groove without a base shape".into());
            }
            let out = csg::boolean(
                &base_m,
                &tool,
                if groove { Op::Difference } else { Op::Union },
            );
            finish(out, what)
        }
        Feature::Fillet { edges, radius }
        | Feature::Chamfer {
            edges,
            size: radius,
        } => {
            let chamfer = matches!(object.feature, Feature::Chamfer { .. });
            let what = if chamfer { "Chamfer" } else { "Fillet" };
            let shape = base.clone().ok_or(format!("{what}: No base shape"))?;
            if edges.is_empty() {
                return Err(format!("{what}: No edges selected"));
            }
            let mut tools = Vec::new();
            let mut fresh = Vec::new();
            for r in edges {
                let i = resolve(r, &shape.mesh, &shape.topo)
                    .ok_or_else(|| format!("{what}: {} was not found", r.name))?;
                let dress = if chamfer {
                    Dress::Chamfer(*radius)
                } else {
                    Dress::Fillet(*radius)
                };
                let (tool, cut) = solid::dress_tool(&shape.mesh, &shape.topo, i, dress)
                    .map_err(|e| format!("{what} on {}: {e}", r.name))?;
                tools.push((tool, cut));
                fresh.push(SubRef::edge(&shape.topo, &shape.mesh, i));
            }
            if let Some(Object {
                feature: Feature::Fillet { edges, .. } | Feature::Chamfer { edges, .. },
                ..
            }) = doc.get_mut(name)
            {
                *edges = fresh;
            }
            let mut out = shape.mesh.clone();
            // Cuts first, then additions: an added round must not be cut by a
            // neighbouring edge's tool.
            for (tool, _) in tools.iter().filter(|t| t.1) {
                out = csg::boolean(&out, tool, Op::Difference);
            }
            for (tool, _) in tools.iter().filter(|t| !t.1) {
                out = csg::boolean(&out, tool, Op::Union);
            }
            finish(out, what)
        }
        Feature::Hole {
            profile,
            diameter,
            depth,
            through_all: all,
            cut,
            drill_point,
        } => {
            let (sketch, frame) = profile_of(model, profile)?;
            let base_m = base_mesh();
            if base_m.is_empty() {
                return Err("Hole: Cannot create a hole without a base shape".into());
            }
            let r = diameter / 2.0;
            if r.is_nan() || r <= 0.0 {
                return Err("Hole: Diameter too small".into());
            }
            let d = if *all {
                through_all(&base_m, &frame)
            } else {
                *depth
            };
            if d.is_nan() || d <= 0.0 {
                return Err("Hole: Depth too small".into());
            }
            let centers: Vec<V2> = sketch
                .geos
                .iter()
                .filter(|g| !g.construction)
                .filter_map(|g| match g.geom {
                    Geom::Circle { c, .. } => Some(c),
                    _ => None,
                })
                .collect();
            if centers.is_empty() {
                return Err("Hole: The sketch has no circles to place holes at".into());
            }
            let top = 0.5 + r * 0.1;
            let mut rz: Vec<V2> = vec![V2 { x: 0.0, y: top }];
            match cut {
                HoleCut::None => rz.push(V2 { x: r, y: top }),
                HoleCut::Counterbore {
                    diameter: cd,
                    depth: cdep,
                } => {
                    let cr = cd / 2.0;
                    if cr <= r || *cdep <= 0.0 || *cdep >= d {
                        return Err(
                            "Hole: The counterbore must be wider than the hole and shallower"
                                .into(),
                        );
                    }
                    rz.push(V2 { x: cr, y: top });
                    rz.push(V2 { x: cr, y: -cdep });
                    rz.push(V2 { x: r, y: -cdep });
                }
                HoleCut::Countersink {
                    diameter: cd,
                    angle,
                } => {
                    let cr = cd / 2.0;
                    let half = math::radians(angle / 2.0);
                    if cr <= r || !(half > 0.0 && half < math::FRAC_PI_2) {
                        return Err("Hole: The countersink must be wider than the hole".into());
                    }
                    let sink = (cr - r) / math::tan(half);
                    rz.push(V2 {
                        x: cr + top * math::tan(half),
                        y: top,
                    });
                    rz.push(V2 { x: r, y: -sink });
                }
            }
            rz.push(V2 { x: r, y: -d });
            if let Some(tip) = drill_point {
                let half = math::radians(tip / 2.0);
                if half > 0.0 && half < math::FRAC_PI_2 && !*all {
                    rz.push(V2 {
                        x: 0.0,
                        y: -d - r / math::tan(half),
                    });
                } else {
                    rz.push(V2 { x: 0.0, y: -d });
                }
            } else {
                rz.push(V2 { x: 0.0, y: -d });
            }
            let mut tool = Mesh::default();
            for c in centers {
                let at = frame.to_world(c);
                let one =
                    solid::revolve_rz(at, frame.z, &rz, None).map_err(|e| format!("Hole: {e}"))?;
                tool = csg::boolean(&tool, &one, Op::Union);
            }
            model
                .tools
                .insert(name.into(), (Arc::new(tool.clone()), false));
            finish(csg::boolean(&base_m, &tool, Op::Difference), "Hole")
        }
        Feature::Mirrored { originals, plane } => {
            let what = "Mirrored";
            let xform = |first: &str| -> Result<Vec<Xform>, String> {
                let (o, n) = match plane {
                    PlaneRef::Base(p) => {
                        let f = p.frame();
                        (f.origin, f.z)
                    }
                    PlaneRef::SketchV | PlaneRef::SketchH => {
                        let frame = sketch_frame_of(doc, model, first)?;
                        let axis = if *plane == PlaneRef::SketchV {
                            frame.y
                        } else {
                            frame.x
                        };
                        (frame.origin, axis.cross(frame.z).norm())
                    }
                    PlaneRef::Face(r) => {
                        let shape = base.as_ref().ok_or("no base shape")?;
                        let i = resolve(r, &shape.mesh, &shape.topo)
                            .ok_or_else(|| format!("{} was not found", r.name))?;
                        let f = solid::face_frame(&shape.mesh, &shape.topo, i)
                            .ok_or("the mirror face is not planar")?;
                        (f.origin, f.z)
                    }
                };
                Ok(vec![Xform::mirror(o, n)])
            };
            let xs = match originals.first() {
                Some(first) => xform(first),
                None => Ok(vec![]),
            };
            transform_feature(model, base, originals, what, xs)
        }
        Feature::LinearPattern {
            originals,
            direction,
            length,
            occurrences,
            reversed,
        } => {
            let what = "LinearPattern";
            let (length, occurrences, reversed) = (*length, *occurrences, *reversed);
            if occurrences < 2 {
                return Err(format!("{what}: At least two occurrences required"));
            }
            let xform = |first: &str| -> Result<Vec<Xform>, String> {
                let (_, d) = match direction {
                    AxisRef::Edge(r) => edge_axis(r, base.as_ref())?,
                    other => {
                        let frame = sketch_frame_of(doc, model, first)?;
                        let sketch = sketch_of(doc, first)?;
                        sketch_axis(&frame, &sketch, other)?
                    }
                };
                let d = if reversed { -d } else { d };
                let step = length / (occurrences - 1) as f64;
                Ok((1..occurrences)
                    .map(|k| Xform::translate(d * (step * k as f64)))
                    .collect())
            };
            let xs = match originals.first() {
                Some(first) => xform(first),
                None => Ok(vec![]),
            };
            transform_feature(model, base, originals, what, xs)
        }
        Feature::PolarPattern {
            originals,
            axis,
            angle,
            occurrences,
            reversed,
        } => {
            let what = "PolarPattern";
            let (angle, occurrences, reversed) = (*angle, *occurrences, *reversed);
            if occurrences < 2 {
                return Err(format!("{what}: At least two occurrences required"));
            }
            let xform = |first: &str| -> Result<Vec<Xform>, String> {
                let (o, d) = match axis {
                    AxisRef::Edge(r) => edge_axis(r, base.as_ref())?,
                    other => {
                        let frame = sketch_frame_of(doc, model, first)?;
                        let sketch = sketch_of(doc, first)?;
                        sketch_axis(&frame, &sketch, other)?
                    }
                };
                let d = if reversed { -d } else { d };
                let total = math::radians(angle.clamp(-360.0, 360.0));
                // A full turn spreads N copies evenly; less spans the angle end to end.
                let step = if (total.abs() - TAU).abs() < 1e-9 {
                    total / occurrences as f64
                } else {
                    total / (occurrences - 1) as f64
                };
                Ok((1..occurrences)
                    .map(|k| Xform::rotate(o, d, step * k as f64))
                    .collect())
            };
            let xs = match originals.first() {
                Some(first) => xform(first),
                None => Ok(vec![]),
            };
            transform_feature(model, base, originals, what, xs)
        }
    }
}

fn sketch_of(doc: &Document, feature: &str) -> Result<Sketch, String> {
    let profile = doc
        .get(feature)
        .and_then(|o| o.feature.profile())
        .ok_or_else(|| format!("{feature} is not sketch-based"))?;
    match doc.get(profile).map(|o| &o.feature) {
        Some(Feature::Sketch { sketch, .. }) => Ok(sketch.clone()),
        _ => Err(format!("{profile} is not a sketch")),
    }
}
fn sketch_frame_of(doc: &Document, model: &Model, feature: &str) -> Result<Frame, String> {
    let profile = doc
        .get(feature)
        .and_then(|o| o.feature.profile())
        .ok_or_else(|| format!("{feature} is not sketch-based: choose a base plane or axis"))?;
    model
        .frames
        .get(profile)
        .copied()
        .ok_or_else(|| format!("{profile} could not be placed"))
}

/// Apply the originals' tools, transformed, to the current shape.
fn transform_feature(
    model: &mut Model,
    base: Option<Arc<Shape>>,
    originals: &[String],
    what: &str,
    xs: Result<Vec<Xform>, String>,
) -> Result<Option<Arc<Shape>>, String> {
    let shape = base.ok_or(format!("{what}: No base shape"))?;
    if originals.is_empty() {
        return Err(format!("{what}: No originals selected"));
    }
    let xs = xs.map_err(|e| format!("{what}: {e}"))?;
    let mut out = shape.mesh.clone();
    for original in originals {
        let (tool, adds) = model
            .tools
            .get(original)
            .cloned()
            .ok_or_else(|| format!("{what}: {original} cannot be transformed"))?;
        for x in &xs {
            let moved = tool.transformed(x);
            out = csg::boolean(&out, &moved, if adds { Op::Union } else { Op::Difference });
        }
    }
    if out.is_empty() || out.volume() <= 1e-9 {
        return Err(format!("{what}: Resulting shape is empty"));
    }
    if solids(&out) > 1 {
        return Err(format!(
            "{what}: Transformed shape does not intersect the support"
        ));
    }
    Ok(Some(Shape::new(out)))
}

/// A starter document: one body, as Part Design's "Create body" makes.
pub fn with_body(label: &str) -> (Document, String) {
    let mut d = Document::new(label);
    let body = d.add(Feature::Body {
        group: vec![],
        tip: None,
    });
    (d, body)
}

/// Offset used by tests and callers that want a base plane frame by name.
pub fn plane_by_name(name: &str) -> Option<BasePlane> {
    match name {
        "XY_Plane" => Some(BasePlane::XY),
        "XZ_Plane" => Some(BasePlane::XZ),
        "YZ_Plane" => Some(BasePlane::YZ),
        _ => None,
    }
}
