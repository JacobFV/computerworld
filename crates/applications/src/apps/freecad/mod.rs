//! FreeCAD 1.0: the Part Design workbench and the Sketcher over the `cw-cad` kernel.
//!
//! The application is FreeCAD's main window as it ships with the "FreeCAD Light" theme
//! on the three desktops — menu bar (the Mac's global menu bar on macOS), toolbars, the
//! Combo View with its model tree, property editor and task panels, the 3D view with
//! navigation cube and axis cross, the report view and the status bar. Every command
//! runs against a real parametric document: sketches are solved, features recomputed
//! in order, files written to and read from the machine's filesystem.
use crate::desktop_scene::{DesktopTheme, Painter};
use crate::AppEffect;
use cw_cad::document::{self, Document, Feature, Model, Object, Status};
use cw_cad::math::{V2, V3};
use cw_cad::sketch::{Pos, SolveReport};
use cw_cad::view::{Camera, Element, StdView};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

mod browse;
mod commands;
mod file_dialog;
mod files;
pub mod icons;
mod layout;
mod props;
mod render;
mod sketcher;
mod tasks;
#[cfg(test)]
mod tests;
mod view3d;

pub use layout::Layout;

/// Files a file manager opens with FreeCAD: its documents and the CAD exchange
/// formats it imports.
pub fn opens(name: &str) -> bool {
    let lower = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
    lower.ends_with(".fcstd.json")
        || [".step", ".stp", ".stl", ".obj", ".dxf"]
            .iter()
            .any(|e| lower.ends_with(e))
}

/// Undo levels kept. Each holds a whole document, so this bounds a snapshot's size.
const UNDO_LIMIT: usize = 30;
/// Report view lines kept.
const REPORT_LIMIT: usize = 200;

/// Something picked: an object, and optionally one of its faces, edges or vertices
/// (`Face6`), or a sketch element (`Edge3`, `Vertex2`, `Constraint4` in FreeCAD's
/// sketch-edit naming).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sel {
    pub object: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sub: String,
    /// Where on it the pick landed.
    #[serde(default)]
    pub point: V3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComboTab {
    #[default]
    Model,
    Tasks,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropTab {
    View,
    #[default]
    Data,
}

/// FreeCAD's mouse navigation styles this front end implements.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavStyle {
    /// Left drag rotates, right drag pans, the wheel zooms.
    #[default]
    Gesture,
    /// Left drag rotates, middle drag pans, the wheel zooms.
    OpenInventor,
}
impl NavStyle {
    pub fn label(self) -> &'static str {
        match self {
            NavStyle::Gesture => "Gesture",
            NavStyle::OpenInventor => "OpenInventor",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            NavStyle::Gesture => {
                "Select: left click · Rotate: left drag · Pan: right drag · Zoom: wheel"
            }
            NavStyle::OpenInventor => {
                "Select: left click · Rotate: left drag · Pan: middle drag · Zoom: wheel"
            }
        }
    }
    pub const ALL: [NavStyle; 2] = [NavStyle::Gesture, NavStyle::OpenInventor];
}

/// How a shape is drawn: FreeCAD's DisplayMode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayMode {
    #[default]
    #[serde(rename = "Flat Lines")]
    FlatLines,
    Shaded,
    Wireframe,
}
impl DisplayMode {
    pub fn label(self) -> &'static str {
        match self {
            DisplayMode::FlatLines => "Flat Lines",
            DisplayMode::Shaded => "Shaded",
            DisplayMode::Wireframe => "Wireframe",
        }
    }
    pub const ALL: [DisplayMode; 3] = [
        DisplayMode::FlatLines,
        DisplayMode::Shaded,
        DisplayMode::Wireframe,
    ];
}

/// View-provider properties (the property editor's View tab) of one object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewProps {
    #[serde(default)]
    pub display: DisplayMode,
    /// Percent.
    #[serde(default)]
    pub transparency: u8,
    #[serde(default = "line_width")]
    pub line_width: f64,
}
fn line_width() -> f64 {
    2.0
}

/// A text field taking keystrokes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub target: FieldTarget,
    pub text: String,
    /// The whole text is selected, so typing replaces it (as a spin box does on focus).
    #[serde(default)]
    pub replace: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "field", rename_all = "snake_case")]
pub enum FieldTarget {
    /// A Data property of an object.
    Property { object: String, name: String },
    /// A parameter of the open task panel.
    Task { name: String },
    /// The value of a sketch constraint (1-based index in FreeCAD's list).
    Constraint { index: usize },
    /// The file name box of the file dialog.
    FileName,
    /// The name of a folder the file dialog is about to create.
    FolderName,
    /// Renaming a tree item's label.
    Label { object: String },
    /// The expression bound to a Data property (the f(x) button).
    Expression { object: String, name: String },
}

/// A sketch open in the Sketcher.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchEdit {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<sketcher::Tool>,
    /// Points the active tool has collected so far (sketch coordinates).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clicks: Vec<V2>,
    /// What the clicks landed on, for automatic constraints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snaps: Vec<Option<(i32, Pos)>>,
    /// Selected geometry and points.
    #[serde(default)]
    pub picked: Vec<(i32, Pos)>,
    /// Selected constraints (0-based).
    #[serde(default)]
    pub constraints: Vec<usize>,
    pub report: SolveReport,
    /// New geometry is construction geometry (FreeCAD's construction mode toggle).
    #[serde(default)]
    pub construction: bool,
    /// Radius the fillet tool rounds with.
    pub fillet_radius: f64,
    /// A geometric-constraint command waiting for its picks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<String>,
    /// Which panel sections are folded.
    #[serde(default)]
    pub folded: BTreeSet<String>,
}

/// A feature's parameters being edited in the task panel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeatureEdit {
    pub name: String,
    /// The document before the dialog opened; Cancel returns to it.
    pub before: Document,
    /// Edge picks add (true) or remove (false) references, for Fillet and Chamfer.
    #[serde(default)]
    pub select_mode: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "task", rename_all = "snake_case")]
pub enum Task {
    Sketch(Box<SketchEdit>),
    Feature(Box<FeatureEdit>),
    /// Create Sketch with nothing selected: choose a base plane or a datum plane of
    /// the body (`plane` is the base plane's or the datum's name).
    PickPlane {
        body: String,
        plane: String,
    },
    /// The unified Measure tool.
    Measure,
}

/// A modal dialog over the main window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "dialog", rename_all = "snake_case")]
pub enum Dialog {
    File(Box<files::FileDialog>),
    /// A new dimension's value (FreeCAD's "Insert length" / "Insert angle" / …).
    Dimension {
        index: usize,
        title: String,
    },
    /// The document has unsaved changes and something would discard it.
    Unsaved {
        then: String,
    },
    /// A message with an OK button.
    Message {
        title: String,
        text: String,
    },
    /// Help → About FreeCAD.
    About,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    Log,
    Warning,
    Error,
}

/// A pointer press being tracked on the 3D view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Press {
    pub x: i32,
    pub y: i32,
    pub last: (i32, i32),
    pub button: u8,
    pub moved: bool,
    /// What the press grabbed, when it grabbed sketch geometry to drag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grab: Option<(i32, Pos)>,
    /// Pressed on the navigation cube's area rather than the model.
    #[serde(default)]
    pub on_cube: bool,
}

/// A menu entry for the Mac's global menu bar: (label, target, enabled, keys).
pub type MenuEntry = (String, String, Result<(), String>, String);

/// The last rendered 3D view, keyed by a hash of everything it depends on.
type RasterCache = Mutex<Option<(u64, Arc<Vec<u8>>)>>;

/// The derived model, recomputed from the document and never serialised: a snapshot
/// restores the document and recomputes it, which is deterministic.
#[derive(Default)]
struct Cache {
    model: Mutex<Option<(u64, Arc<Model>)>>,
    raster: RasterCache,
}
impl Clone for Cache {
    fn clone(&self) -> Self {
        Cache {
            model: Mutex::new(self.model.lock().map(|g| g.clone()).unwrap_or(None)),
            raster: Mutex::new(self.raster.lock().map(|g| g.clone()).unwrap_or(None)),
        }
    }
}
impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cache")
    }
}
impl PartialEq for Cache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cad {
    pub doc: Document,
    /// File the document was opened from or last saved to; empty when never saved.
    pub path: String,
    pub modified: bool,
    #[serde(default)]
    pub undo: Vec<(String, Document)>,
    #[serde(default)]
    pub redo: Vec<(String, Document)>,
    pub camera: Camera,
    #[serde(default)]
    pub selection: Vec<Sel>,
    #[serde(default)]
    pub expanded: BTreeSet<String>,
    #[serde(default)]
    pub combo: ComboTab,
    #[serde(default)]
    pub props: PropTab,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<Task>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialog: Option<Dialog>,
    /// Open pull-down or pop-up menu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<Field>,
    #[serde(default)]
    pub report: Vec<(ReportKind, String)>,
    #[serde(default)]
    pub show_report: bool,
    #[serde(default)]
    pub nav: NavStyle,
    /// Last status bar message.
    #[serde(default)]
    pub status: String,
    /// View → Bounding box.
    #[serde(default)]
    pub bbox: bool,
    /// View → Axis cross.
    #[serde(default = "yes")]
    pub axis_cross: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press: Option<Press>,
    /// Button of the press about to be delivered.
    #[serde(default)]
    pub button: u8,
    #[serde(default)]
    pub view_props: std::collections::BTreeMap<String, ViewProps>,
    /// The user's home folder, where file dialogs start.
    #[serde(default)]
    pub home: String,
    /// The desktop the window opened on; its file dialogs follow that platform's
    /// conventions (which button Return presses in "Replace?").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<DesktopTheme>,
    #[serde(default)]
    pub active_body: Option<String>,
    /// Bumped on every document change; keys the derived model.
    #[serde(default)]
    pub rev: u64,
    /// The world's clock at the last interaction: what a file's timestamp records.
    #[serde(default)]
    pub clock_us: u64,
    /// Width the 3D view was last painted at, so keyboard zoom centres on it.
    #[serde(default)]
    pub view_size: (u32, u32),
    /// A file read in flight and what it is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io_read: Option<files::Pending>,
    /// What to do once the pending save lands (New or Open after "Save changes?").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_save: Option<String>,
    /// The folder FreeCAD was launched on (`~/Documents/Parts`): where its file
    /// dialogs start while it exists.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub start_folder: String,
    #[serde(skip)]
    cache: Cache,
}
fn yes() -> bool {
    true
}
// The document holds floating point; equality is still an equivalence (no NaN is ever
// stored: every value is validated as finite before it reaches the document).
impl Eq for Cad {}

/// The kind `application.v1 launch` takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Freecad(pub Box<Cad>);
impl std::ops::Deref for Freecad {
    type Target = Cad;
    fn deref(&self) -> &Cad {
        &self.0
    }
}
impl std::ops::DerefMut for Freecad {
    fn deref_mut(&mut self) -> &mut Cad {
        &mut self.0
    }
}

impl Freecad {
    pub const KIND: &'static str = "freecad";
    pub fn launch(argument: &str, window: u64, clock_us: u64) -> (Self, Vec<AppEffect>) {
        let (doc, body) = document::with_body("Unnamed");
        let mut cad = Cad {
            doc,
            path: String::new(),
            modified: false,
            undo: vec![],
            redo: vec![],
            camera: Camera::default(),
            selection: vec![],
            expanded: [body.clone()].into_iter().collect(),
            combo: ComboTab::Model,
            props: PropTab::Data,
            task: None,
            dialog: None,
            menu: None,
            field: None,
            report: vec![],
            show_report: false,
            nav: NavStyle::Gesture,
            status: String::new(),
            bbox: false,
            axis_cross: true,
            press: None,
            button: 0,
            view_props: Default::default(),
            home: String::new(),
            platform: None,
            active_body: Some(body),
            rev: 0,
            clock_us,
            view_size: (0, 0),
            io_read: None,
            after_save: None,
            start_folder: String::new(),
            cache: Cache::default(),
        };
        cad.camera.half_height = 60.0;
        let mut effects = vec![];
        if !argument.is_empty() {
            effects = cad.open_path(window, argument);
        }
        (Freecad(Box::new(cad)), effects)
    }
    /// The shell tells a fresh window where the user's files are.
    pub fn attach(&mut self, home: &str, platform: Option<DesktopTheme>) {
        self.home = home.trim_end_matches('/').to_owned();
        self.platform = platform;
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, _theme: DesktopTheme) -> String {
        "FreeCAD 1.0".into()
    }
    pub fn document(&self) -> String {
        self.path.clone()
    }
    pub fn caption(&self) -> String {
        format!(
            "{}{}",
            self.doc.label,
            if self.modified { " *" } else { "" }
        )
    }
    pub fn modified(&self) -> bool {
        self.modified
    }
    pub fn http(
        &mut self,
        _w: u64,
        _tag: &str,
        _status: u16,
        _body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        Err("FreeCAD makes no network requests".into())
    }
    pub fn offline(&mut self, _tag: &str, reason: &str) {
        self.0.listing_failed(reason);
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        self.0.type_text(text)
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        self.0.clock_us = clock_us;
        self.0.key(window, key)
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        self.0.clock_us = clock_us;
        let command = target
            .strip_prefix("freecad:")
            .ok_or("interaction does not belong to FreeCAD")?
            .to_owned();
        let r = self.0.command(window, &command, None);
        self.0.report_refusal(r)
    }
    /// A click that knows where inside its control it landed (the navigation cube).
    pub fn click_at(
        &mut self,
        window: u64,
        target: &str,
        dx: i32,
        dy: i32,
        _clock: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("freecad:")
            .ok_or("interaction does not belong to FreeCAD")?
            .to_owned();
        let r = self.0.command(window, &command, Some((dx, dy)));
        self.0.report_refusal(r)
    }
    pub fn activate(
        &mut self,
        window: u64,
        target: &str,
        _clock: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("freecad:")
            .ok_or("interaction does not belong to FreeCAD")?
            .to_owned();
        let r = self.0.double_click(window, &command);
        self.0.report_refusal(r)
    }
    /// The 3D view follows drags; its target carries the size it was painted at, so
    /// pointer positions map onto the same projection the pixels came from.
    pub fn drags(&self, target: &str) -> bool {
        view3d::view_size_of(target).is_some()
    }
    pub fn pointer(
        &mut self,
        window: u64,
        target: &str,
        phase: crate::PointerPhase,
        x: i32,
        y: i32,
    ) -> Result<Vec<AppEffect>, String> {
        let size =
            view3d::view_size_of(target).ok_or("that surface belongs to another application")?;
        if self.dialog.is_some() {
            return Err("Finish the dialog first".into());
        }
        self.0.view_size = size;
        let r = self.0.pointer(window, phase, x, y);
        self.0.report_refusal(r)
    }
    /// Which button the next press is made with (0 left, 1 middle, 2 right).
    pub fn pointer_button(&mut self, button: u8) {
        self.0.button = button;
    }
    pub fn wheel(&mut self, target: &str, x: i32, y: i32, delta: i32) -> Result<bool, String> {
        // The file dialog's list and sidebar scroll under the wheel.
        if self.0.file_dialog_wheel(target, delta) {
            return Ok(true);
        }
        let Some(size) = view3d::view_size_of(target) else {
            return Ok(false);
        };
        self.0.view_size = size;
        self.0.wheel(x, y, delta);
        Ok(true)
    }
    /// Bytes of a file this window asked to read, or why they could not be read.
    pub fn bytes_loaded(
        &mut self,
        path: &str,
        result: Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        self.0.bytes_loaded(path, result)
    }
    pub fn written(&mut self, path: &str) {
        self.0.written(path)
    }
    pub fn listed(&mut self, entries: Vec<String>) {
        self.0.listed(entries)
    }
    /// FreeCAD's menus for the Mac's global menu bar: (label, target, enabled, keys).
    pub fn mac_menu(&self, panel: &str) -> Option<Vec<MenuEntry>> {
        let name = match panel {
            "file" => "File",
            "edit" => "Edit",
            "view" => "View",
            "help" => "Help",
            _ => return None,
        };
        let (_, items) = commands::MENUS.iter().find(|(n, _)| *n == name)?;
        let mut out: Vec<(String, String, Result<(), String>, String)> = items
            .iter()
            .map(|id| {
                if *id == "-" {
                    return (String::new(), String::new(), Ok(()), String::new());
                }
                let c = commands::command(id);
                (
                    c.map(|c| c.label).unwrap_or(id).to_owned(),
                    format!("freecad:cmd:{id}"),
                    self.0.available(id),
                    c.map(|c| c.keys).unwrap_or("").to_owned(),
                )
            })
            .collect();
        // The Mac's menu bar carries File, Edit, View and Help; the commands of Tools,
        // Part Design and Sketch that no toolbar button offers hang off View's end.
        if name == "View" {
            out.push((String::new(), String::new(), Ok(()), String::new()));
            for id in [
                "Std_Measure",
                "PartDesign_MoveTip",
                "Sketcher_EditSketch",
                "Sketcher_SelectConflictingConstraints",
            ] {
                let c = commands::command(id);
                out.push((
                    c.map(|c| c.label).unwrap_or(id).to_owned(),
                    format!("freecad:cmd:{id}"),
                    self.0.available(id),
                    c.map(|c| c.keys).unwrap_or("").to_owned(),
                ));
            }
        }
        Some(out)
    }
    /// The Mac menus, encoded for the window frame (`WindowView::chrome`): one entry per
    /// menu, items separated by U+001E and fields (label, target, why disabled, keys)
    /// by U+001F.
    pub fn chrome(&self) -> Vec<(String, String)> {
        ["file", "edit", "view", "help"]
            .iter()
            .filter_map(|panel| {
                let items = self.mac_menu(panel)?;
                let text = items
                    .into_iter()
                    .map(|(label, target, ok, keys)| {
                        format!(
                            "{label}\u{1f}{target}\u{1f}{}\u{1f}{keys}",
                            ok.err().unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\u{1e}");
                Some((format!("mac:{panel}"), text))
            })
            .collect()
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        self.0.page(page)
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        render::render(&self.0, p, env)
    }
}

impl Cad {
    /// A refused command says why in the status bar, as FreeCAD's does, since the
    /// action's own error reaches only the caller.
    fn report_refusal(
        &mut self,
        r: Result<Vec<AppEffect>, String>,
    ) -> Result<Vec<AppEffect>, String> {
        if let Err(e) = &r {
            self.status = e.clone();
        }
        r
    }
    /// The recomputed model of the current document.
    pub fn model(&self) -> Arc<Model> {
        if let Ok(guard) = self.cache.model.lock() {
            if let Some((rev, m)) = guard.as_ref() {
                if *rev == self.rev {
                    return m.clone();
                }
            }
        }
        let mut doc = self.doc.clone();
        let m = Arc::new(document::recompute(&mut doc));
        if let Ok(mut guard) = self.cache.model.lock() {
            *guard = Some((self.rev, m.clone()));
        }
        m
    }
    /// Recompute after an edit, keeping the references recompute re-found.
    pub(crate) fn recompute(&mut self) {
        self.rev += 1;
        let m = Arc::new(document::recompute(&mut self.doc));
        if let Ok(mut guard) = self.cache.model.lock() {
            *guard = Some((self.rev, m.clone()));
        }
        // Errors go to the report view, as FreeCAD's recompute reports them.
        let errors: Vec<String> = m
            .status
            .iter()
            .filter_map(|(name, s)| match s {
                Status::Error(e)
                    if !self
                        .doc
                        .get(name)
                        .is_some_and(|o| matches!(o.feature, Feature::Body { .. })) =>
                {
                    Some(format!("{}: {e}", self.label_of(name)))
                }
                _ => None,
            })
            .collect();
        for e in errors {
            self.log(ReportKind::Error, &e);
        }
    }
    pub(crate) fn raster_cache(&self) -> &RasterCache {
        &self.cache.raster
    }
    /// Record the document before an edit, for Undo.
    pub(crate) fn checkpoint(&mut self, label: &str) {
        self.undo.push((label.to_owned(), self.doc.clone()));
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.modified = true;
    }
    pub(crate) fn log(&mut self, kind: ReportKind, text: &str) {
        self.report.push((kind, text.to_owned()));
        if self.report.len() > REPORT_LIMIT {
            self.report.remove(0);
        }
        // FreeCAD pops the report view up on warnings and errors.
        if kind != ReportKind::Log {
            self.show_report = true;
        }
    }
    pub fn label_of(&self, name: &str) -> String {
        self.doc
            .get(name)
            .map(|o| o.label.clone())
            .unwrap_or_else(|| name.to_owned())
    }
    pub fn object(&self, name: &str) -> Option<&Object> {
        self.doc.get(name)
    }
    /// The body new features go into: the active one, else the only one.
    pub fn body(&self) -> Option<String> {
        self.active_body
            .clone()
            .filter(|b| self.doc.get(b).is_some())
            .or_else(|| self.doc.bodies().first().map(|b| (*b).to_owned()))
    }
    pub fn sketch_edit(&self) -> Option<&SketchEdit> {
        match &self.task {
            Some(Task::Sketch(s)) => Some(s),
            _ => None,
        }
    }
    pub fn sketch_edit_mut(&mut self) -> Option<&mut SketchEdit> {
        match &mut self.task {
            Some(Task::Sketch(s)) => Some(s),
            _ => None,
        }
    }
    pub fn view_props(&self, name: &str) -> ViewProps {
        self.view_props.get(name).cloned().unwrap_or(ViewProps {
            display: DisplayMode::FlatLines,
            transparency: 0,
            line_width: 2.0,
        })
    }
    /// The selected object (the first selection's), if any.
    pub fn selected_object(&self) -> Option<&str> {
        self.selection.first().map(|s| s.object.as_str())
    }
    pub(crate) fn set_view(&mut self, v: StdView) {
        self.camera.set_view(v);
    }
    pub(crate) fn element_of(sub: &str) -> Option<Element> {
        let n = |p: &str| {
            sub.strip_prefix(p)
                .and_then(|d| d.parse::<usize>().ok())
                .filter(|i| *i > 0)
                .map(|i| i - 1)
        };
        n("Face")
            .map(Element::Face)
            .or_else(|| n("Edge").map(Element::Edge))
            .or_else(|| n("Vertex").map(Element::Vertex))
    }
}
