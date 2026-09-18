//! Files: the document in FreeCAD's native structure (as JSON), STL/OBJ/DXF import,
//! STL (binary and ASCII), OBJ, DXF and SVG export, through a file dialog over the
//! machine's real folders. Reads and writes are effects the environment carries out;
//! nothing is marked saved until the write is reported.
use super::*;
use cw_cad::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Open,
    SaveAs,
    Import,
    Export,
}
impl Purpose {
    pub fn title(self) -> &'static str {
        match self {
            Purpose::Open => "Open document",
            Purpose::SaveAs => "Save As",
            Purpose::Import => "Import file",
            Purpose::Export => "Export file",
        }
    }
    pub fn button(self) -> &'static str {
        match self {
            Purpose::Open => "Open",
            Purpose::SaveAs => "Save",
            Purpose::Import => "Open",
            Purpose::Export => "Save",
        }
    }
    pub fn saving(self) -> bool {
        matches!(self, Purpose::SaveAs | Purpose::Export)
    }
}

/// The file types each dialog offers: (filter text, extension written or accepted).
pub fn filters(p: Purpose) -> &'static [(&'static str, &'static str)] {
    match p {
        Purpose::Open | Purpose::SaveAs => &[("FreeCAD document (*.FCStd.json)", "FCStd.json")],
        Purpose::Import => &[
            ("STL Mesh (*.stl)", "stl"),
            ("Alias Mesh (*.obj)", "obj"),
            ("Autodesk DXF 2D (*.dxf)", "dxf"),
        ],
        Purpose::Export => &[
            ("STL Mesh (*.stl)", "stl"),
            ("ASCII STL (*.ast)", "ast"),
            ("Alias Mesh (*.obj)", "obj"),
            ("Autodesk DXF 2D (*.dxf)", "dxf"),
            ("Flattened SVG (*.svg)", "svg"),
        ],
    }
}

/// The platform's own file dialog, as FreeCAD 1.0 shows it on each desktop (an
/// NSSavePanel/NSOpenPanel, Windows' common item dialog, GTK's file chooser): one
/// state over the machine's real folders, drawn in the look of the desktop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileDialog {
    pub purpose: Purpose,
    pub folder: String,
    pub entries: Vec<String>,
    pub loading: bool,
    pub name: String,
    pub filter: usize,
    /// After a Save As, what to do next (`new`, `open`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub then: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Where Back returns to, newest last, and where Forward goes again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub back: Vec<super::browse::Location>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forward: Vec<super::browse::Location>,
    /// Explorer's Home: the pinned Quick access folders of `folder` (the home folder)
    /// rather than its whole listing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub home_view: bool,
    /// The entry highlighted in the list (a folder keeps its trailing `/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    /// The New Folder name prompt (macOS sheet, GTK popover), with its last refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<super::browse::FolderPrompt>,
    /// A save would replace this existing file: the platform's "Replace?" question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<String>,
    /// The Mac save panel folded down to its Where pop-up.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
}
impl FileDialog {
    pub fn extension(&self) -> &'static str {
        filters(self.purpose)
            .get(self.filter)
            .map(|f| f.1)
            .unwrap_or("")
    }
    /// Entries the current filter shows: folders and matching files.
    pub fn visible(&self) -> Vec<&String> {
        if self.home_view {
            // Explorer's Home pins the standard folders that really exist, in its order.
            return crate::QUICK_ACCESS
                .iter()
                .filter_map(|name| self.entries.iter().find(|e| **e == format!("{name}/")))
                .collect();
        }
        let exts: Vec<&str> = match self.purpose {
            Purpose::Import => filters(self.purpose).iter().map(|f| f.1).collect(),
            _ => vec![self.extension()],
        };
        self.entries
            .iter()
            .filter(|e| {
                e.ends_with('/')
                    || exts.iter().any(|x| {
                        e.to_ascii_lowercase()
                            .ends_with(&format!(".{}", x.to_ascii_lowercase()))
                    })
            })
            .collect()
    }
}

/// Parent folder of a path.
pub(super) fn parent(path: &str) -> String {
    let t = path.trim_end_matches('/');
    match t.rsplit_once('/') {
        Some(("", _)) => "/".into(),
        Some((p, _)) => p.into(),
        None => "/".into(),
    }
}
pub(super) fn join(folder: &str, name: &str) -> String {
    if folder.ends_with('/') {
        format!("{folder}{name}")
    } else {
        format!("{folder}/{name}")
    }
}
fn stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let name = name.strip_suffix(".FCStd.json").unwrap_or(name);
    name.rsplit_once('.')
        .map(|(s, _)| s)
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
        .to_owned()
}

/// What a read that is in flight is for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    pub purpose: Purpose,
    pub path: String,
}

impl Cad {
    fn default_folder(&self) -> String {
        if !self.path.is_empty() {
            return parent(&self.path);
        }
        if self.home.is_empty() {
            return "/".into();
        }
        join(&self.home, "Documents")
    }

    pub(crate) fn show_file_dialog(&mut self, window: u64, purpose: Purpose) -> Vec<AppEffect> {
        let folder = self.default_folder();
        let name = match purpose {
            Purpose::SaveAs => format!("{}.FCStd.json", self.doc.label),
            Purpose::Export => {
                let base = self
                    .export_target()
                    .map(|(n, _)| self.label_of(&n))
                    .unwrap_or_else(|| self.doc.label.clone());
                format!("{base}.stl")
            }
            _ => String::new(),
        };
        self.dialog = Some(Dialog::File(Box::new(FileDialog {
            purpose,
            folder: folder.clone(),
            entries: vec![],
            loading: true,
            name: name.clone(),
            filter: 0,
            then: None,
            error: None,
            back: vec![],
            forward: vec![],
            home_view: false,
            selected: None,
            prompt: None,
            confirm: None,
            collapsed: false,
        })));
        if purpose.saving() {
            self.field = Some(Field {
                target: FieldTarget::FileName,
                text: name,
                replace: true,
            });
        }
        vec![AppEffect::ListDirectory {
            window,
            tab: 0,
            path: folder,
        }]
    }

    /// A folder listing for the file dialog arrived.
    pub fn listed(&mut self, mut entries: Vec<String>) {
        entries.sort_by(|a, b| {
            (!a.ends_with('/'))
                .cmp(&!b.ends_with('/'))
                .then(a.to_lowercase().cmp(&b.to_lowercase()))
        });
        if let Some(Dialog::File(d)) = &mut self.dialog {
            d.entries = entries;
            d.loading = false;
            d.error = None;
        }
    }
    pub(crate) fn listing_failed(&mut self, reason: &str) {
        if let Some(Dialog::File(d)) = &mut self.dialog {
            d.loading = false;
            d.entries.clear();
            d.error = Some(reason.to_owned());
        } else {
            self.log(ReportKind::Error, reason);
        }
    }

    pub(crate) fn file_command(
        &mut self,
        window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        // Browsing (places, path bar, history, New Folder, Replace?) is the dialog's
        // own navigation; what follows here is the file type and the final answer.
        if let Some(result) = self.browse_command(window, rest) {
            return result;
        }
        let Some(Dialog::File(d)) = &mut self.dialog else {
            return Err("no file dialog is open".into());
        };
        if let Some(i) = rest.strip_prefix("type:") {
            let i: usize = i.parse().map_err(|_| "bad file type")?;
            if i >= filters(d.purpose).len() {
                return Err("no such file type".into());
            }
            d.filter = i;
            if d.purpose.saving() {
                // A name typed but not yet committed is the one that changes extension.
                if let Some(Field {
                    target: FieldTarget::FileName,
                    text,
                    ..
                }) = &self.field
                {
                    d.name = text.trim().to_owned();
                }
                let ext = d.extension();
                let base = stem(&d.name);
                d.name = format!("{base}.{ext}");
                self.field = Some(Field {
                    target: FieldTarget::FileName,
                    text: d.name.clone(),
                    replace: true,
                });
            }
            return Ok(vec![]);
        }
        match rest {
            "cancel" => {
                self.dialog = None;
                self.field = None;
                Ok(vec![])
            }
            "ok" => self.file_ok(window, false),
            "replace" => self.file_ok(window, true),
            other => Err(format!("unknown file dialog command {other}")),
        }
    }

    /// Save or Open. A save over a file the folder already holds asks first, as every
    /// platform's dialog does; `replace` is the answer Yes/Replace.
    fn file_ok(&mut self, window: u64, replace: bool) -> Result<Vec<AppEffect>, String> {
        let Some(Dialog::File(d)) = &mut self.dialog else {
            return Err("no file dialog is open".into());
        };
        if d.confirm.is_some() && !replace {
            return Err("Answer whether to replace the file first".into());
        }
        if replace && d.confirm.is_none() {
            return Err("nothing is waiting to be replaced".into());
        }
        // A name typed but not yet committed counts.
        if let Some(Field {
            target: FieldTarget::FileName,
            text,
            ..
        }) = &self.field
        {
            d.name = text.trim().to_owned();
        }
        // Open with a folder highlighted (or on Explorer's Home) goes into the folder.
        let chosen_folder = d.selected.as_ref().filter(|s| s.ends_with('/'));
        if !d.purpose.saving() || d.home_view || d.name.trim().is_empty() {
            if let Some(folder) = chosen_folder.cloned() {
                return self.open_entry(window, &folder);
            }
            if d.home_view {
                return Err("Choose a folder first".into());
            }
        }
        let d = (**d).clone();
        if d.name.trim().is_empty() || d.name.contains('/') {
            return Err("Type a file name".into());
        }
        let mut name = d.name.trim().to_owned();
        let ext = d.extension();
        if d.purpose.saving()
            && !name
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", ext.to_ascii_lowercase()))
            && !(ext == "ast" && name.to_ascii_lowercase().ends_with(".stl"))
        {
            name = format!("{name}.{ext}");
        }
        // Windows' file system ignores case, so "a.stl" would replace "A.STL" there.
        let windows = self.platform == Some(DesktopTheme::Windows);
        let taken = |e: &String| {
            if windows {
                e.eq_ignore_ascii_case(&name)
            } else {
                *e == name
            }
        };
        if d.purpose.saving() && !replace && d.entries.iter().any(taken) {
            if let Some(Dialog::File(live)) = &mut self.dialog {
                live.name = name.clone();
                live.confirm = Some(name);
            }
            // The question takes the keyboard: Return answers it, not the name box.
            self.field = None;
            return Ok(vec![]);
        }
        if d.entries.iter().any(|e| *e == format!("{name}/")) {
            return Err(format!("“{name}” is a folder"));
        }
        let path = join(&d.folder, &name);
        self.field = None;
        self.dialog = None;
        match d.purpose {
            Purpose::Open | Purpose::Import => {
                self.io_read = Some(Pending {
                    purpose: d.purpose,
                    path: path.clone(),
                });
                Ok(vec![AppEffect::ReadBytes { window, path }])
            }
            Purpose::SaveAs => {
                let effects = self.write_document(window, &path);
                if let Some(then) = d.then {
                    self.after_save = Some(then);
                }
                Ok(effects)
            }
            Purpose::Export => self.export(window, &path, ext),
        }
    }

    fn write_document(&mut self, window: u64, path: &str) -> Vec<AppEffect> {
        self.path = path.to_owned();
        let label = stem(path);
        if !label.is_empty() {
            self.doc.label = label;
        }
        vec![AppEffect::WriteFile {
            window,
            path: path.to_owned(),
            content: io::save_native(&self.doc),
        }]
    }

    pub(crate) fn save(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        if self.path.is_empty() {
            return Ok(self.show_file_dialog(window, Purpose::SaveAs));
        }
        let path = self.path.clone();
        Ok(self.write_document(window, &path))
    }

    /// New and Open replace the document; with unsaved changes they ask first.
    pub(crate) fn guarded(&mut self, window: u64, then: &str) -> Result<Vec<AppEffect>, String> {
        if self.modified {
            self.dialog = Some(Dialog::Unsaved {
                then: then.to_owned(),
            });
            return Ok(vec![]);
        }
        self.proceed(window, then)
    }
    fn proceed(&mut self, window: u64, then: &str) -> Result<Vec<AppEffect>, String> {
        match then {
            "new" => {
                let (home, platform) = (self.home.clone(), self.platform);
                let (fresh, _) = Freecad::launch("", window, 0);
                *self = *fresh.0;
                self.home = home;
                self.platform = platform;
                self.log(ReportKind::Log, "New document Unnamed");
                Ok(vec![])
            }
            "open" => Ok(self.show_file_dialog(window, Purpose::Open)),
            _ => Err("unknown follow-up".into()),
        }
    }

    pub(crate) fn dialog_command(
        &mut self,
        window: u64,
        rest: &str,
    ) -> Result<Vec<AppEffect>, String> {
        match (self.dialog.clone(), rest) {
            (Some(_), "block") => {
                self.status = "Finish the dialog first".into();
                Ok(vec![])
            }
            (Some(Dialog::File(_)), "ok" | "cancel") => self.file_command(window, rest),
            (Some(Dialog::Dimension { .. }), "ok") => {
                if self.field.is_some() {
                    self.commit_field(window)?;
                }
                self.dialog = None;
                Ok(vec![])
            }
            (Some(Dialog::Dimension { .. }), "cancel") => {
                // The constraint stays with the value the geometry had, as in FreeCAD.
                self.field = None;
                self.dialog = None;
                Ok(vec![])
            }
            (Some(Dialog::Unsaved { then }), "save") => {
                self.dialog = None;
                if self.path.is_empty() {
                    let effects = self.show_file_dialog(window, Purpose::SaveAs);
                    if let Some(Dialog::File(d)) = &mut self.dialog {
                        d.then = Some(then);
                    }
                    return Ok(effects);
                }
                let mut effects = self.save(window)?;
                self.modified = false;
                effects.extend(self.proceed(window, &then)?);
                Ok(effects)
            }
            (Some(Dialog::Unsaved { then }), "discard") => {
                self.dialog = None;
                self.modified = false;
                self.proceed(window, &then)
            }
            (Some(Dialog::Unsaved { .. }), "cancel" | "ok") => {
                self.dialog = None;
                Ok(vec![])
            }
            (Some(Dialog::Message { .. } | Dialog::About), "ok" | "cancel") => {
                self.dialog = None;
                Ok(vec![])
            }
            (None, _) => Err("no dialog is open".into()),
            (_, other) => Err(format!("the dialog has no {other} button")),
        }
    }

    /// A write this window asked for reached the disk.
    pub fn written(&mut self, path: &str) {
        if path == self.path {
            self.modified = false;
            self.log(ReportKind::Log, &format!("Saved {path}"));
            self.status = format!("Saved {path}");
            if let Some(then) = self.after_save.take() {
                let _ = self.proceed(0, &then);
            }
        } else {
            self.log(ReportKind::Log, &format!("Exported {path}"));
            self.status = format!("Exported {path}");
        }
    }

    /// Bytes a read asked for arrived (or the reason they did not).
    pub fn bytes_loaded(
        &mut self,
        path: &str,
        result: Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        let pending = self
            .io_read
            .take()
            .filter(|p| p.path == path)
            .ok_or("nothing was waiting for that file")?;
        let bytes = match result {
            Ok(b) => b,
            Err(e) => {
                self.dialog = Some(Dialog::Message {
                    title: pending.purpose.title().into(),
                    text: format!("Cannot open {path}: {e}"),
                });
                self.log(ReportKind::Error, &format!("Cannot open {path}: {e}"));
                return Ok(());
            }
        };
        let fail = |cad: &mut Cad, e: String| {
            cad.dialog = Some(Dialog::Message {
                title: pending.purpose.title().into(),
                text: e.clone(),
            });
            cad.log(ReportKind::Error, &e);
        };
        match pending.purpose {
            Purpose::Open => {
                let text = String::from_utf8_lossy(&bytes);
                match io::load_native(&text) {
                    Ok(doc) => {
                        let (home, platform) = (self.home.clone(), self.platform);
                        let (fresh, _) = Freecad::launch("", 0, 0);
                        *self = *fresh.0;
                        self.home = home;
                        self.platform = platform;
                        self.doc = doc;
                        self.path = path.to_owned();
                        self.active_body = self.doc.bodies().first().map(|b| (*b).to_owned());
                        self.expanded = self.doc.bodies().iter().map(|b| (*b).to_owned()).collect();
                        self.recompute();
                        self.fit_all();
                        self.log(ReportKind::Log, &format!("Opened {path}"));
                    }
                    Err(e) => fail(self, format!("{path}: {e}")),
                }
            }
            Purpose::Import => {
                let lower = path.to_ascii_lowercase();
                let label = stem(path);
                if lower.ends_with(".dxf") {
                    let text = String::from_utf8_lossy(&bytes);
                    match io::read_dxf(&text) {
                        Ok((sketch, skipped)) => {
                            self.checkpoint("Import");
                            let feature = Feature::Sketch {
                                sketch,
                                support: cw_cad::document::Support::Plane {
                                    plane: cw_cad::document::BasePlane::XY,
                                },
                                offset: 0.0,
                            };
                            let name = match self.body() {
                                Some(b) => self.doc.add_to_body(&b, feature)?,
                                None => self.doc.add(feature),
                            };
                            if let Some(o) = self.doc.get_mut(&name) {
                                o.label = label;
                            }
                            self.recompute();
                            self.fit_all();
                            let note = if skipped > 0 {
                                format!(" ({skipped} unsupported entities skipped)")
                            } else {
                                String::new()
                            };
                            self.log(ReportKind::Log, &format!("Imported {path} as {name}{note}"));
                        }
                        Err(e) => fail(self, format!("{path}: {e}")),
                    }
                } else {
                    let mesh = if lower.ends_with(".obj") {
                        io::read_obj(&String::from_utf8_lossy(&bytes))
                    } else {
                        io::read_stl(&bytes)
                    };
                    match mesh {
                        Ok(m) => {
                            self.checkpoint("Import");
                            let name = self.doc.add(Feature::Mesh { mesh: Arc::new(m) });
                            if let Some(o) = self.doc.get_mut(&name) {
                                o.label = label;
                            }
                            self.recompute();
                            self.fit_all();
                            self.selection = vec![Sel {
                                object: name.clone(),
                                sub: String::new(),
                                point: V3::ZERO,
                            }];
                            self.log(ReportKind::Log, &format!("Imported {path} as mesh {name}"));
                        }
                        Err(e) => fail(self, format!("{path}: {e}")),
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Objects that can be exported.
    pub(crate) fn exportable(&self) -> Vec<String> {
        let model = self.model();
        self.doc
            .objects
            .iter()
            .filter(|o| match &o.feature {
                Feature::Body { .. } => model.body_shape.contains_key(&o.name),
                Feature::Mesh { .. } | Feature::Sketch { .. } => true,
                f => f.is_solid_feature() && model.shapes.contains_key(&o.name),
            })
            .map(|o| o.name.clone())
            .collect()
    }
    /// What Export writes: the selected object, else the active body's shape.
    pub(crate) fn export_target(&self) -> Option<(String, Arc<cw_cad::document::Shape>)> {
        let model = self.model();
        let shape_of = |n: &str| -> Option<Arc<cw_cad::document::Shape>> {
            match self.doc.get(n).map(|o| &o.feature) {
                Some(Feature::Body { .. }) => model.body_shape.get(n).cloned(),
                _ => model.shapes.get(n).cloned(),
            }
        };
        self.selection
            .iter()
            .find_map(|s| shape_of(&s.object).map(|sh| (s.object.clone(), sh)))
            .or_else(|| self.body().and_then(|b| shape_of(&b).map(|sh| (b, sh))))
    }
    fn export_sketch(&self) -> Option<String> {
        if let Some(e) = self.sketch_edit() {
            return Some(e.name.clone());
        }
        self.selection.iter().map(|s| s.object.clone()).find(|o| {
            matches!(
                self.doc.get(o).map(|x| &x.feature),
                Some(Feature::Sketch { .. })
            )
        })
    }

    fn export(&mut self, window: u64, path: &str, ext: &str) -> Result<Vec<AppEffect>, String> {
        let effect = match ext {
            "dxf" => {
                let name = self
                    .export_sketch()
                    .ok_or("Select a sketch to export as DXF")?;
                let Some(Feature::Sketch { sketch, .. }) = self.doc.get(&name).map(|o| &o.feature)
                else {
                    return Err("not a sketch".into());
                };
                AppEffect::WriteFile {
                    window,
                    path: path.into(),
                    content: io::dxf(sketch),
                }
            }
            "svg" => {
                let mut shapes: Vec<Arc<cw_cad::document::Shape>> = vec![];
                if let Some((_, sh)) = self.export_target() {
                    shapes.push(sh);
                }
                if shapes.is_empty() {
                    return Err("There is no solid to project".into());
                }
                let refs: Vec<(&cw_cad::mesh::Mesh, &cw_cad::solid::Topology)> =
                    shapes.iter().map(|s| (&s.mesh, &s.topo)).collect();
                AppEffect::WriteFile {
                    window,
                    path: path.into(),
                    content: cw_cad::view::svg(&self.camera, &refs, true),
                }
            }
            _ => {
                let (name, shape) = self.export_target().ok_or("Select an object to export")?;
                let label = self.label_of(&name);
                match ext {
                    "ast" => AppEffect::WriteFile {
                        window,
                        path: path.into(),
                        content: io::stl_ascii(&shape.mesh, &label),
                    },
                    "obj" => AppEffect::WriteFile {
                        window,
                        path: path.into(),
                        content: io::obj(&shape.mesh, &label),
                    },
                    _ => AppEffect::WriteBytes {
                        window,
                        path: path.into(),
                        bytes: io::stl_binary(&shape.mesh, &format!("{label} exported by FreeCAD")),
                    },
                }
            }
        };
        Ok(vec![effect])
    }

    /// Open a document or import a file named at launch.
    pub(crate) fn open_path(&mut self, window: u64, path: &str) -> Vec<AppEffect> {
        let purpose = if path.ends_with(".FCStd.json") {
            Purpose::Open
        } else {
            Purpose::Import
        };
        self.io_read = Some(Pending {
            purpose,
            path: path.to_owned(),
        });
        vec![AppEffect::ReadBytes {
            window,
            path: path.to_owned(),
        }]
    }
}
