//! KiCad 8: the project manager, Schematic Editor, PCB Editor and SPICE Simulator, each
//! its own window over one open project, as the real program's frames share one
//! process. The engine is `cw_eda`; this module is the program around it: frames,
//! tools, dialogs, the project's files on the machine's filesystem.
//!
//! Every frame of a project holds the same [`Session`]; the desktop copies the newest
//! one to the other frames after each action (`NativeApp::share_from`), so an edit in
//! the schematic is in the board editor's Update PCB and the simulator's netlist.
use crate::desktop_scene::{DesktopTheme, Painter};
use crate::{AppEffect, PointerPhase};
use cw_eda::drc;
use cw_eda::erc;
use cw_eda::files;
use cw_eda::geom::Pt;
use cw_eda::pcb::{Board, BoardItem, Change, Layer};
use cw_eda::schematic::{History, Item, Schematic};
use serde::{Deserialize, Serialize};

mod draw;
mod icons;
mod pcb;
mod pm;
mod sch;
mod sim;
#[cfg(test)]
mod tests;
mod widgets;

pub use sim::{Plot, Trace};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frame {
    #[default]
    ProjectManager,
    Schematic,
    Pcb,
    Simulator,
}
impl Frame {
    fn code(self) -> &'static str {
        match self {
            Self::ProjectManager => "pm",
            Self::Schematic => "sch",
            Self::Pcb => "pcb",
            Self::Simulator => "sim",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pm" => Self::ProjectManager,
            "sch" => Self::Schematic,
            "pcb" | "pcb-update" => Self::Pcb,
            "sim" => Self::Simulator,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Folder holding the project's files.
    pub dir: String,
    /// Base name shared by `<name>.kicad_pro`, `.kicad_sch` and `.kicad_pcb`.
    pub name: String,
}
impl Project {
    pub fn from_pro(path: &str) -> Option<Self> {
        let (dir, file) = path.rsplit_once('/')?;
        let name = file.strip_suffix(".kicad_pro")?;
        Some(Self {
            dir: dir.to_owned(),
            name: name.to_owned(),
        })
    }
    pub fn file(&self, ext: &str) -> String {
        format!("{}/{}.{ext}", self.dir, self.name)
    }
    pub fn pro(&self) -> String {
        self.file("kicad_pro")
    }
}

/// DRC results as the DRC dialog's tabs hold them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrcReport {
    pub violations: Vec<drc::Violation>,
    pub unconnected: Vec<drc::Violation>,
    pub parity: Vec<drc::Violation>,
}

/// What every frame of the open project shares.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Bumped on every change, so the desktop knows which frame holds the newest copy.
    pub revision: u64,
    /// Where new projects are created and the Open dialog looks.
    pub projects_dir: String,
    pub project: Option<Project>,
    /// The project folder's listing, as the project tree shows it.
    pub listing: Vec<String>,
    /// `.kicad_pro` files found under `projects_dir` for the Open dialog.
    pub found: Vec<String>,
    pub schematic: Schematic,
    pub board: Board,
    pub sch_history: History<Schematic>,
    pub pcb_history: History<Board>,
    pub sch_dirty: bool,
    pub pcb_dirty: bool,
    pub erc: Option<Vec<erc::Violation>>,
    pub drc: Option<DrcReport>,
    pub sim: sim::SimState,
    /// Why the project could not be read, shown in place of its contents.
    pub problem: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    /// World coordinate at the canvas's left and top edges.
    pub x0: i64,
    pub y0: i64,
    /// Pixels per 1000 world units (mils in the schematic, micrometres on the board).
    pub zoom: i64,
    /// Fit the page (or board) to the canvas on the next paint.
    pub fit: bool,
}
impl View {
    pub fn to_px(&self, v: i64, origin: i64) -> i32 {
        ((v - origin) as i128 * self.zoom as i128 / 1000) as i32
    }
    pub fn px(&self, p: Pt) -> (i32, i32) {
        (self.to_px(p.x, self.x0), self.to_px(p.y, self.y0))
    }
    pub fn world(&self, x: i32, y: i32) -> Pt {
        let z = self.zoom.max(1) as i128;
        Pt::new(
            self.x0 + (x as i128 * 1000 / z) as i64,
            self.y0 + (y as i128 * 1000 / z) as i64,
        )
    }
    pub fn len(&self, v: i64) -> i32 {
        (v as i128 * self.zoom as i128 / 1000) as i32
    }
    /// Zoom by `num/den` keeping world point `at` under the same pixel.
    pub fn zoom_about(&mut self, at: Pt, num: i64, den: i64, min: i64, max: i64) {
        let old = self.zoom.max(1);
        let new = (old * num / den).clamp(min, max);
        if new == old {
            return;
        }
        self.x0 = at.x - ((at.x - self.x0) as i128 * old as i128 / new as i128) as i64;
        self.y0 = at.y - ((at.y - self.y0) as i128 * old as i128 / new as i128) as i64;
        self.zoom = new;
        self.fit = false;
    }
    /// Fit a world rectangle into a canvas of `w`×`h` pixels with a margin.
    pub fn fitted(min: Pt, max: Pt, w: u32, h: u32) -> View {
        let (ww, wh) = ((max.x - min.x).max(1), (max.y - min.y).max(1));
        let zx = (w.max(1) as i128 * 1000 * 100 / (ww as i128 * 108)) as i64;
        let zy = (h.max(1) as i128 * 1000 * 100 / (wh as i128 * 108)) as i64;
        let zoom = zx.min(zy).max(1);
        let cx = (min.x + max.x) / 2;
        let cy = (min.y + max.y) / 2;
        View {
            x0: cx - (w as i128 * 1000 / 2 / zoom as i128) as i64,
            y0: cy - (h as i128 * 1000 / 2 / zoom as i128) as i64,
            zoom,
            fit: false,
        }
    }
    /// Encode as target arguments, and read back.
    pub fn args(&self) -> String {
        format!("{}:{}:{}", self.x0, self.y0, self.zoom)
    }
    pub fn from_args(parts: &[&str]) -> Option<View> {
        Some(View {
            x0: parts.first()?.parse().ok()?,
            y0: parts.get(1)?.parse().ok()?,
            zoom: parts.get(2)?.parse().ok()?,
            fit: false,
        })
    }
}

/// A modal dialog and the values being edited in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "dialog", rename_all = "snake_case")]
pub enum Dialog {
    NewProject {
        name: String,
        error: String,
    },
    OpenProject {
        selected: Option<usize>,
    },
    About,
    Chooser {
        power: bool,
        filter: String,
        selected: Option<String>,
        collapsed: Vec<String>,
    },
    SymbolProperties {
        id: u64,
        fields: Vec<(String, String)>,
        exclude_sim: bool,
        on_board: bool,
        in_bom: bool,
        dnp: bool,
        error: String,
    },
    Label {
        pos: Pt,
        global: bool,
        text: String,
        edit: Option<u64>,
    },
    Annotate {
        by_x: bool,
        reset: bool,
        log: Vec<String>,
    },
    Erc {
        selected: Option<usize>,
    },
    Netlist {
        spice: bool,
        log: Vec<String>,
    },
    Bom {
        log: Vec<String>,
    },
    UpdatePcb {
        changes: Vec<Change>,
        applied: bool,
    },
    Drc {
        tab: u8,
        selected: Option<usize>,
    },
    Plot {
        svg: bool,
        dir: String,
        layers: Vec<Layer>,
        log: Vec<String>,
    },
    Drill {
        log: Vec<String>,
    },
    BoardSetup {
        fields: Vec<(String, String)>,
        error: String,
    },
    ZoneProperties {
        outline: Vec<Pt>,
        net: usize,
        layer: Layer,
        clearance: String,
        error: String,
    },
    SimSettings {
        tab: u8,
        fields: Vec<(String, String)>,
        error: String,
    },
    AddSignals {
        chosen: Vec<String>,
    },
}
impl Dialog {
    fn fields_mut(&mut self) -> Option<&mut Vec<(String, String)>> {
        match self {
            Self::SymbolProperties { fields, .. }
            | Self::BoardSetup { fields, .. }
            | Self::SimSettings { fields, .. } => Some(fields),
            _ => None,
        }
    }
    /// The text a named field holds.
    pub fn field_mut(&mut self, name: &str) -> Option<&mut String> {
        match (self, name) {
            (Self::NewProject { name: n, .. }, "name") => Some(n),
            (Self::Chooser { filter, .. }, "filter") => Some(filter),
            (Self::Label { text, .. }, "text") => Some(text),
            (Self::Plot { dir, .. }, "dir") => Some(dir),
            (Self::ZoneProperties { clearance, .. }, "clearance") => Some(clearance),
            (d, n) => d
                .fields_mut()?
                .iter_mut()
                .find(|(k, _)| k == n)
                .map(|(_, v)| v),
        }
    }
    fn title(&self) -> &'static str {
        match self {
            Self::NewProject { .. } => "Create New Project",
            Self::OpenProject { .. } => "Open Existing Project",
            Self::About => "About KiCad",
            Self::Chooser { power: false, .. } => "Choose Symbol",
            Self::Chooser { power: true, .. } => "Choose Power Symbol",
            Self::SymbolProperties { .. } => "Symbol Properties",
            Self::Label { global: false, .. } => "Label Properties",
            Self::Label { global: true, .. } => "Global Label Properties",
            Self::Annotate { .. } => "Annotate Schematic",
            Self::Erc { .. } => "Electrical Rules Checker",
            Self::Netlist { .. } => "Export Netlist",
            Self::Bom { .. } => "Generate Bill of Materials",
            Self::UpdatePcb { .. } => "Update PCB from Schematic",
            Self::Drc { .. } => "Design Rules Checker",
            Self::Plot { .. } => "Plot",
            Self::Drill { .. } => "Generate Drill Files",
            Self::BoardSetup { .. } => "Board Setup",
            Self::ZoneProperties { .. } => "Copper Zone Properties",
            Self::SimSettings { .. } => "Simulation Analysis",
            Self::AddSignals { .. } => "Add Signals",
        }
    }
}

/// Where a drag started and what it is doing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "drag", rename_all = "snake_case")]
pub enum Drag {
    /// Moving the selection; `start` and `at` are world points.
    Move { start: Pt, at: Pt },
    /// A selection rectangle.
    Box { start: Pt, at: Pt },
    /// A simulator cursor.
    Cursor { index: u8 },
    /// Pressed on nothing in particular, not yet moved.
    Press { at: Pt },
}

/// Per-window state: the frame's own tools, view and dialogs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ui {
    pub menu: Option<String>,
    pub dialog: Option<Dialog>,
    /// The dialog field keystrokes go to.
    pub focus: Option<String>,
    pub status: String,
    pub view: View,
    /// Canvas size in pixels, as last painted and reported by a pointer event.
    pub canvas: (u32, u32),
    pub hover: Option<Pt>,
    pub drag: Option<Drag>,
    pub sch: sch::SchUi,
    pub pcb: pcb::PcbUi,
    pub sim: sim::SimUi,
    /// Selected row in the project tree.
    pub tree_row: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kicad {
    pub frame: Frame,
    pub session: Box<Session>,
    pub ui: Box<Ui>,
}

pub(crate) fn act(url: &str) -> cw_protocol::PageAction {
    cw_protocol::PageAction {
        method: "APP".into(),
        url: url.into(),
        fields: Default::default(),
    }
}

impl Kicad {
    pub const KIND: &'static str = "kicad";

    /// `argument` is a projects folder, a `.kicad_pro` path, or `<frame>|<.kicad_pro>`
    /// for an editor frame (`sch`, `pcb`, `pcb-update`, `sim`).
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        let (frame, target) = match argument.split_once('|') {
            Some((f, rest)) => (Frame::parse(f).unwrap_or_default(), rest),
            None => (Frame::ProjectManager, argument),
        };
        let mut app = Self {
            frame,
            session: Box::default(),
            ui: Box::default(),
        };
        app.ui.view.fit = true;
        app.ui.pcb = pcb::PcbUi::new();
        let target = target.trim_end_matches('/');
        let effects = if let Some(project) = Project::from_pro(target) {
            app.session.projects_dir = project
                .dir
                .rsplit_once('/')
                .map(|(d, _)| d.to_owned())
                .unwrap_or_default();
            app.load(window, project)
        } else {
            app.session.projects_dir = target.to_owned();
            vec![]
        };
        if argument.starts_with("pcb-update|") {
            app.open_update_dialog();
        }
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        let name = self.session.project.as_ref().map(|p| p.name.as_str());
        // KiCad marks unsaved changes with "*"; on a Mac the window says "Edited" instead.
        let star = |dirty: bool| {
            if dirty && theme != DesktopTheme::Macos {
                "*"
            } else {
                ""
            }
        };
        match (self.frame, name) {
            (Frame::ProjectManager, Some(n)) => format!("KiCad 8.0 — {n}"),
            (Frame::ProjectManager, None) => "KiCad 8.0".into(),
            (Frame::Schematic, n) => format!(
                "{}{} [/] — Schematic Editor",
                star(self.session.sch_dirty),
                n.unwrap_or("untitled")
            ),
            (Frame::Pcb, n) => format!(
                "{}{} — PCB Editor",
                star(self.session.pcb_dirty),
                n.unwrap_or("untitled")
            ),
            (Frame::Simulator, n) => format!("{} — SPICE Simulator", n.unwrap_or("untitled")),
        }
    }
    pub fn document(&self) -> String {
        let Some(p) = &self.session.project else {
            return String::new();
        };
        match self.frame {
            Frame::ProjectManager | Frame::Simulator => p.pro(),
            Frame::Schematic => p.file("kicad_sch"),
            Frame::Pcb => p.file("kicad_pcb"),
        }
    }
    pub fn caption(&self) -> String {
        self.ui.status.clone()
    }
    pub fn modified(&self) -> bool {
        match self.frame {
            Frame::Schematic => self.session.sch_dirty,
            Frame::Pcb => self.session.pcb_dirty,
            _ => false,
        }
    }
    pub fn offline(&mut self, _tag: &str, reason: &str) {
        self.session.problem = Some(reason.to_owned());
        self.touch();
    }
    pub fn http(&mut self, _w: u64, _t: &str, _s: u16, _b: &str) -> Result<Vec<AppEffect>, String> {
        Err("KiCad works on local files and makes no requests".into())
    }

    // ---- linked frames --------------------------------------------------------------
    pub fn instance_key(&self) -> String {
        format!("kicad:{}", self.frame.code())
    }
    pub fn adopt(&mut self, other: &Kicad) {
        // A frame that adopts the open project has nothing of its own left to load.
        if self.ui.status.starts_with("Loading") {
            self.ui.status.clear();
        }
        self.session = other.session.clone();
    }
    pub fn reopen(&mut self, window: u64, argument: &str) -> Vec<AppEffect> {
        let (frame, target) = argument.split_once('|').unwrap_or(("", argument));
        let mut effects = vec![];
        if let Some(project) = Project::from_pro(target) {
            if self.session.project.as_ref() != Some(&project) {
                effects = self.load(window, project);
            }
        }
        if frame == "pcb-update" {
            self.open_update_dialog();
        }
        effects
    }
    pub(crate) fn touch(&mut self) {
        self.session.revision += 1;
    }

    // ---- project files -------------------------------------------------------------
    /// Read a project: its three files at once (a missing schematic or board is a new,
    /// empty one) and its folder for the tree.
    fn load(&mut self, window: u64, project: Project) -> Vec<AppEffect> {
        let paths = vec![
            project.pro(),
            project.file("kicad_sch"),
            project.file("kicad_pcb"),
        ];
        let dir = project.dir.clone();
        self.session.problem = None;
        self.ui.status = format!("Loading {}…", project.pro());
        self.session.schematic = Schematic::new(&project.pro());
        self.session.board = Board::new(&project.pro());
        self.session.sch_history.clear();
        self.session.pcb_history.clear();
        self.session.sch_dirty = false;
        self.session.pcb_dirty = false;
        self.session.erc = None;
        self.session.drc = None;
        self.session.sim = sim::SimState::default();
        self.session.project = Some(project);
        self.ui.view.fit = true;
        self.touch();
        vec![
            AppEffect::ReadFiles {
                window,
                tag: "open".into(),
                paths,
            },
            AppEffect::ListDirectory {
                window,
                tab: 0,
                path: dir,
            },
        ]
    }
    pub fn files_read(
        &mut self,
        _window: u64,
        tag: &str,
        files: Vec<(String, Result<String, String>)>,
    ) -> Vec<AppEffect> {
        if tag != "open" {
            return vec![];
        }
        let mut notes = vec![];
        for (path, result) in files {
            let text = match result {
                Ok(t) => t,
                Err(e) => {
                    if path.ends_with(".kicad_pro") {
                        self.session.problem = Some(format!("{path}: {e}"));
                    } else {
                        notes.push(format!(
                            "{} not found; starting a new one",
                            path.rsplit('/').next().unwrap_or(&path)
                        ));
                    }
                    continue;
                }
            };
            if path.ends_with(".kicad_pro") {
                match files::read_project(&text) {
                    Ok(rules) => self.session.board.rules = rules,
                    Err(e) => self.session.problem = Some(format!("{path}: {e}")),
                }
            } else if path.ends_with(".kicad_sch") {
                match files::read_schematic(&text) {
                    Ok((s, warnings)) => {
                        self.session.schematic = s;
                        notes.extend(warnings);
                    }
                    Err(e) => self.session.problem = Some(format!("{path}: {e}")),
                }
            } else if path.ends_with(".kicad_pcb") {
                let rules = self.session.board.rules.clone();
                match files::read_board(&text) {
                    Ok((mut b, warnings)) => {
                        b.rules = rules;
                        self.session.board = b;
                        notes.extend(warnings);
                    }
                    Err(e) => self.session.problem = Some(format!("{path}: {e}")),
                }
            }
        }
        self.session.sim.command = self
            .session
            .schematic
            .sim_command()
            .map(|t| t.text.clone())
            .unwrap_or_default();
        self.ui.status = match (&self.session.problem, notes.is_empty()) {
            (Some(p), _) => p.clone(),
            (None, true) => format!(
                "Opened {}",
                self.session
                    .project
                    .as_ref()
                    .map(|p| p.pro())
                    .unwrap_or_default()
            ),
            (None, false) => notes.join("; "),
        };
        self.ui.view.fit = true;
        self.touch();
        vec![]
    }
    /// The project folder's listing, for the tree.
    pub fn listed(&mut self, entries: Vec<String>) {
        self.session.listing = entries;
        self.touch();
    }
    pub fn tree_listed(
        &mut self,
        _window: u64,
        _path: &str,
        result: Result<Vec<String>, String>,
    ) -> Vec<AppEffect> {
        match result {
            Ok(entries) => {
                let root = self.session.projects_dir.trim_end_matches('/').to_owned();
                self.session.found = entries
                    .into_iter()
                    .filter(|e| e.ends_with(".kicad_pro"))
                    .map(|e| format!("{root}/{e}"))
                    .collect();
            }
            Err(_) => self.session.found.clear(),
        }
        self.touch();
        vec![]
    }
    pub fn written(&mut self, path: &str) {
        self.ui.status = format!("Saved {path}");
    }
    fn save_schematic(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let project = self.session.project.clone().ok_or("No project is open")?;
        let text = files::write_schematic(&self.session.schematic, &project.name);
        self.session.sch_dirty = false;
        self.touch();
        Ok(vec![
            AppEffect::WriteFile {
                window,
                path: project.file("kicad_sch"),
                content: text,
            },
            AppEffect::ListDirectory {
                window,
                tab: 0,
                path: project.dir,
            },
        ])
    }
    fn save_board(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let project = self.session.project.clone().ok_or("No project is open")?;
        let text = files::write_board(&self.session.board);
        self.session.pcb_dirty = false;
        self.touch();
        Ok(vec![
            AppEffect::WriteFile {
                window,
                path: project.file("kicad_pcb"),
                content: text,
            },
            AppEffect::WriteFile {
                window,
                path: project.pro(),
                content: files::write_project(&project.name, &self.session.board.rules),
            },
            AppEffect::ListDirectory {
                window,
                tab: 0,
                path: project.dir,
            },
        ])
    }
    /// Open another frame of this project (or raise it if it is open).
    fn launch_frame(&self, window: u64, frame: &str) -> Result<Vec<AppEffect>, String> {
        let project = self.session.project.as_ref().ok_or("No project is open")?;
        Ok(vec![AppEffect::Launch {
            window,
            kind: Self::KIND.into(),
            argument: format!("{frame}|{}", project.pro()),
        }])
    }
    fn open_update_dialog(&mut self) {
        let mut preview = self.session.board.clone();
        let changes = preview.update_from_schematic(&self.session.schematic, true);
        self.ui.dialog = Some(Dialog::UpdatePcb {
            changes,
            applied: false,
        });
        self.ui.menu = None;
    }

    // ---- input -----------------------------------------------------------------------
    pub fn accepts_text(&self) -> bool {
        self.ui.focus.is_some() && self.ui.dialog.is_some()
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        self.text_effects(0, text).map(|_| ())
    }
    pub fn text_effects(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
        if let (Some(focus), Some(dialog)) = (self.ui.focus.clone(), self.ui.dialog.as_mut()) {
            let field = dialog
                .field_mut(&focus)
                .ok_or("the focused field is gone")?;
            super::push_bounded(field, text, 256);
            return Ok(vec![]);
        }
        // With no field focused, typed letters are the frame's hotkeys.
        let mut effects = vec![];
        for ch in text.chars() {
            effects.extend(self.key(window, &ch.to_string(), 0)?);
        }
        Ok(effects)
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        let before = self.session.clone();
        let result = self.key_inner(window, key, clock_us);
        if self.session != before {
            self.touch();
        }
        result
    }
    fn key_inner(
        &mut self,
        window: u64,
        key: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        if self.ui.dialog.is_some() {
            return match key {
                "Escape" => self.command(window, "dlg:cancel", clock_us),
                "Enter" => self.command(window, "dlg:ok", clock_us),
                "Backspace" => {
                    let focus = self.ui.focus.clone().ok_or("no field is focused")?;
                    let field = self
                        .ui
                        .dialog
                        .as_mut()
                        .and_then(|d| d.field_mut(&focus))
                        .ok_or("the focused field is gone")?;
                    field.pop();
                    Ok(vec![])
                }
                "Tab" => Ok(vec![]),
                other => Err(format!(
                    "{other} does nothing while the {} dialog is open",
                    self.ui.dialog.as_ref().map(Dialog::title).unwrap_or("")
                )),
            };
        }
        if self.ui.menu.is_some() && key == "Escape" {
            self.ui.menu = None;
            return Ok(vec![]);
        }
        match (key, self.frame) {
            ("Ctrl+s" | "Meta+s", Frame::Schematic) => self.save_schematic(window),
            ("Ctrl+s" | "Meta+s", Frame::Pcb) => self.save_board(window),
            (_, Frame::Schematic) => self.sch_key(window, key),
            (_, Frame::Pcb) => self.pcb_key(window, key),
            (_, Frame::Simulator) => self.sim_key(window, key),
            (_, Frame::ProjectManager) => match key {
                "Ctrl+n" | "Meta+n" => self.command(window, "pm:new", clock_us),
                "Ctrl+o" | "Meta+o" => self.command(window, "pm:open", clock_us),
                "Ctrl+e" | "Meta+e" => self.command(window, "pm:launch:sch", clock_us),
                "Ctrl+p" | "Meta+p" => self.command(window, "pm:launch:pcb", clock_us),
                other => Err(format!("unsupported project manager key {other}")),
            },
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        self.click_at(window, target, 0, 0, clock_us)
    }
    pub fn click_at(
        &mut self,
        window: u64,
        target: &str,
        dx: i32,
        dy: i32,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("kicad:")
            .ok_or("interaction does not belong to KiCad")?
            .to_owned();
        if self.drags(target) {
            let mut effects = self.pointer(window, target, PointerPhase::Down, dx, dy, clock_us)?;
            effects.extend(self.pointer(window, target, PointerPhase::Up, dx, dy, clock_us)?);
            return Ok(effects);
        }
        let before = self.session.clone();
        let result = self.command(window, &command, clock_us);
        if self.session != before {
            self.touch();
        }
        result
    }
    /// A double click: opens what was clicked, or finishes the wire, track or zone
    /// being drawn.
    pub fn activate(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("kicad:")
            .ok_or("interaction does not belong to KiCad")?
            .to_owned();
        let before = self.session.clone();
        let result = if command.starts_with("canvas:") {
            match self.frame {
                Frame::Schematic => self.sch_activate(window),
                Frame::Pcb => self.pcb_activate(window),
                _ => Ok(vec![]),
            }
        } else if let Some(rest) = command.strip_prefix("pm:file:") {
            let i: usize = rest.parse().map_err(|_| "bad row")?;
            self.open_tree_row(window, i)
        } else if let Some(lib_id) = command.strip_prefix("dlg:pick:") {
            self.command(window, &format!("dlg:pick:{lib_id}"), clock_us)?;
            self.command(window, "dlg:ok", clock_us)
        } else if let Some(i) = command.strip_prefix("dlg:project:") {
            self.command(window, &format!("dlg:project:{i}"), clock_us)?;
            self.command(window, "dlg:ok", clock_us)
        } else {
            self.command(window, &command, clock_us)
        };
        if self.session != before {
            self.touch();
        }
        result
    }
    pub fn drags(&self, target: &str) -> bool {
        target.starts_with("kicad:canvas:")
    }
    pub fn hover(&mut self, target: &str, x: i32, y: i32) -> bool {
        let Some(rest) = target.strip_prefix("kicad:canvas:") else {
            return false;
        };
        let parts: Vec<&str> = rest.split(':').collect();
        if !matches!(parts[0], "sch" | "pcb") || self.ui.dialog.is_some() {
            return false;
        }
        let Some(view) = View::from_args(&parts[1..]) else {
            return false;
        };
        if let (Some(w), Some(h)) = (
            parts.get(4).and_then(|v| v.parse().ok()),
            parts.get(5).and_then(|v| v.parse().ok()),
        ) {
            self.ui.canvas = (w, h);
        }
        // Keyboard zoom works about the pointer, so the view it zooms is the painted one.
        if !self.ui.view.fit {
            self.ui.view = view;
        }
        let at = self.canvas_world(view.world(x, y));
        let snapped = self.snap(at, true);
        if self.ui.hover == Some(snapped) {
            return false;
        }
        self.ui.hover = Some(snapped);
        true
    }
    pub fn pointer(
        &mut self,
        window: u64,
        target: &str,
        phase: PointerPhase,
        x: i32,
        y: i32,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let rest = target
            .strip_prefix("kicad:canvas:")
            .ok_or("not a KiCad canvas")?;
        let parts: Vec<&str> = rest.split(':').collect();
        let frame = parts.first().copied().unwrap_or("");
        if self.ui.dialog.is_some() {
            return Err(format!(
                "close the {} dialog first",
                self.ui.dialog.as_ref().map(Dialog::title).unwrap_or("")
            ));
        }
        self.ui.menu = None;
        let before = self.session.clone();
        let result = match frame {
            "sch" | "pcb" => {
                let view = View::from_args(&parts[1..]).ok_or("bad canvas view")?;
                if let (Some(w), Some(h)) = (
                    parts.get(4).and_then(|v| v.parse().ok()),
                    parts.get(5).and_then(|v| v.parse().ok()),
                ) {
                    self.ui.canvas = (w, h);
                }
                // The view the canvas was painted with is the one the pointer used.
                self.ui.view = view;
                let at = self.canvas_world(view.world(x, y));
                if frame == "sch" {
                    self.sch_pointer(window, phase, at)
                } else {
                    self.pcb_pointer(window, phase, at)
                }
            }
            "plot" => self.plot_pointer(phase, &parts[1..], x),
            _ => Err("unknown canvas".into()),
        };
        if self.session != before {
            self.touch();
        }
        result
    }
    /// A canvas view's world point in the frame's own units: the schematic view works
    /// in mils directly; the board view works in micrometres over nanometre geometry.
    fn canvas_world(&self, p: Pt) -> Pt {
        match self.frame {
            Frame::Pcb => Pt::new(p.x * 1000, p.y * 1000),
            _ => p,
        }
    }
    /// Snap a world point the way the frame's tools do.
    fn snap(&self, p: Pt, hover: bool) -> Pt {
        match self.frame {
            Frame::Schematic => p.snap(cw_eda::schematic::GRID),
            Frame::Pcb => self.pcb_snap(p, hover),
            _ => p,
        }
    }

    /// A command from a control, without the `kicad:` prefix.
    fn command(
        &mut self,
        window: u64,
        command: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let (head, rest) = command.split_once(':').unwrap_or((command, ""));
        // A dialog is modal: only its own controls and the menu bar's closing act.
        if self.ui.dialog.is_some() && !matches!(head, "dlg" | "field") {
            return Err(format!(
                "close the {} dialog first",
                self.ui.dialog.as_ref().map(Dialog::title).unwrap_or("")
            ));
        }
        if head != "menu" {
            self.ui.menu = None;
        }
        match head {
            "menu" => {
                self.ui.menu = if self.ui.menu.as_deref() == Some(rest) {
                    None
                } else {
                    Some(rest.to_owned())
                };
                Ok(vec![])
            }
            "field" => {
                let dialog = self.ui.dialog.as_mut().ok_or("no dialog is open")?;
                dialog.field_mut(rest).ok_or("no such field")?;
                self.ui.focus = Some(rest.to_owned());
                Ok(vec![])
            }
            "dlg" => self.dialog_command(window, rest, clock_us),
            "about" => {
                self.ui.dialog = Some(Dialog::About);
                Ok(vec![])
            }
            "pm" => self.pm_command(window, rest),
            "sch" => self.sch_command(window, rest),
            "pcb" => self.pcb_command(window, rest),
            "sim" => self.sim_command(window, rest),
            "noop" => Err("this control is disabled".into()),
            other => Err(format!("unknown KiCad command {other}")),
        }
    }
    fn close_dialog(&mut self) {
        self.ui.dialog = None;
        self.ui.focus = None;
    }
    fn dialog_command(
        &mut self,
        window: u64,
        rest: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let dialog = self.ui.dialog.clone().ok_or("no dialog is open")?;
        if rest == "cancel" {
            // Closing a dialog whose work is done (a report, an applied update) is its
            // Close button; closing one mid-edit discards the edit, as Cancel does.
            self.close_dialog();
            return Ok(vec![]);
        }
        match dialog {
            Dialog::NewProject { .. } | Dialog::OpenProject { .. } | Dialog::About => {
                self.pm_dialog(window, rest)
            }
            Dialog::Chooser { .. }
            | Dialog::SymbolProperties { .. }
            | Dialog::Label { .. }
            | Dialog::Annotate { .. }
            | Dialog::Erc { .. }
            | Dialog::Netlist { .. }
            | Dialog::Bom { .. } => self.sch_dialog(window, rest, clock_us),
            Dialog::UpdatePcb { .. }
            | Dialog::Drc { .. }
            | Dialog::Plot { .. }
            | Dialog::Drill { .. }
            | Dialog::BoardSetup { .. }
            | Dialog::ZoneProperties { .. } => self.pcb_dialog(window, rest),
            Dialog::SimSettings { .. } | Dialog::AddSignals { .. } => self.sim_dialog(window, rest),
        }
    }

    // ---- presentation -------------------------------------------------------------
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        match self.frame {
            Frame::ProjectManager => self.render_pm(p, env),
            Frame::Schematic => self.render_sch(p, env),
            Frame::Pcb => self.render_pcb(p, env),
            Frame::Simulator => self.render_sim(p, env),
        }
        self.render_dialog(p, env);
    }
    fn render_dialog(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let Some(dialog) = &self.ui.dialog else {
            return;
        };
        p.z += 50;
        match dialog {
            Dialog::NewProject { .. } | Dialog::OpenProject { .. } | Dialog::About => {
                self.render_pm_dialog(p, env, dialog)
            }
            Dialog::Chooser { .. }
            | Dialog::SymbolProperties { .. }
            | Dialog::Label { .. }
            | Dialog::Annotate { .. }
            | Dialog::Erc { .. }
            | Dialog::Netlist { .. }
            | Dialog::Bom { .. } => self.render_sch_dialog(p, env, dialog),
            Dialog::SimSettings { .. } | Dialog::AddSignals { .. } => {
                self.render_sim_dialog(p, env, dialog)
            }
            _ => self.render_pcb_dialog(p, env, dialog),
        }
        p.z -= 50;
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        page.elements.push(E::Heading {
            id: "kicad-frame".into(),
            text: self.title(DesktopTheme::Ubuntu),
            level: 1,
        });
        if !self.ui.status.is_empty() {
            page.elements.push(E::Text {
                id: "kicad-status".into(),
                text: self.ui.status.clone(),
            });
        }
        if let Some(problem) = &self.session.problem {
            page.elements.push(E::Text {
                id: "kicad-problem".into(),
                text: problem.clone(),
            });
        }
        match self.frame {
            Frame::ProjectManager => self.pm_page(page),
            Frame::Schematic => self.sch_page(page),
            Frame::Pcb => self.pcb_page(page),
            Frame::Simulator => self.sim_page(page),
        }
        if let Some(d) = &self.ui.dialog {
            page.elements.push(E::Heading {
                id: "kicad-dialog".into(),
                text: d.title().into(),
                level: 2,
            });
            let mut dd = d.clone();
            let names: Vec<String> = match d {
                Dialog::NewProject { .. } => vec!["name".into()],
                Dialog::Chooser { .. } => vec!["filter".into()],
                Dialog::Label { .. } => vec!["text".into()],
                Dialog::Plot { .. } => vec!["dir".into()],
                Dialog::ZoneProperties { .. } => vec!["clearance".into()],
                Dialog::SymbolProperties { fields, .. }
                | Dialog::BoardSetup { fields, .. }
                | Dialog::SimSettings { fields, .. } => {
                    fields.iter().map(|(k, _)| k.clone()).collect()
                }
                _ => vec![],
            };
            for n in names {
                let value = dd.field_mut(&n).cloned().unwrap_or_default();
                page.elements.push(E::Input {
                    id: format!("kicad:field:{n}"),
                    label: n.clone(),
                    value,
                    placeholder: String::new(),
                });
            }
            for (id, label) in [("kicad:dlg:ok", "OK"), ("kicad:dlg:cancel", "Cancel")] {
                page.elements.push(E::Button {
                    id: id.into(),
                    text: label.into(),
                    action: act(id),
                });
            }
        }
    }
}

/// Items a schematic selection names, still present.
pub(crate) fn live_items(s: &Schematic, sel: &[Item]) -> Vec<Item> {
    sel.iter().copied().filter(|i| s.exists(*i)).collect()
}
pub(crate) fn live_board_items(b: &Board, sel: &[BoardItem]) -> Vec<BoardItem> {
    sel.iter().copied().filter(|i| b.exists(*i)).collect()
}
