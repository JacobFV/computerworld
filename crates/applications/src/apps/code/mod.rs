//! Visual Studio Code. A real editor over the machine's own filesystem: the Explorer is
//! a listing of a real folder, a tab is a real file's text, Save writes it back, the
//! integrated terminal is a session of the machine's shell, Run executes the file with
//! that shell's own interpreters, and Source Control drives the machine's git. Nothing
//! on screen is a stand-in: a control either does what it says or is shown disabled.
pub mod buffer;
pub mod commands;
pub mod debug;
pub mod frame;
mod icons;
pub mod problems;
mod render;
pub mod syntax;

use crate::desktop_scene::{DesktopTheme, Painter};
use crate::{AppEffect, ShellOutcome, TerminalEntry};
use buffer::{find_all, Doc, FindOptions};
use problems::{Problem, Severity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use syntax::Language;

pub use frame::title_bar;

/// Retained output, results and listings are bounded so a snapshot is not a function of
/// how long a session ran.
const OUTPUT_LIMIT: usize = 400;
const TRANSCRIPT_LIMIT: usize = 200;
const TREE_LIMIT: usize = 4000;
const HIT_LIMIT: usize = 2000;
const SEARCH_FILES: usize = 400;
const FIELD_LIMIT: usize = 512;
const TEXT_LIMIT: usize = 1 << 20;
const TERMINAL_LIMIT: usize = 5;
const TREE_DEPTH: u32 = 12;
/// Editor groups a window may be split into.
const GROUP_LIMIT: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Explorer,
    Search,
    Scm,
    Run,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelTab {
    Problems,
    Output,
    #[default]
    Terminal,
    /// The Debug Console: the program's output, and expressions evaluated in its frame.
    Debug,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Focus {
    #[default]
    Editor,
    Explorer,
    Terminal,
    /// The Debug Console's input, or a Run and Debug prompt (a watch expression, a
    /// breakpoint's condition).
    Debug,
    Search,
    SearchReplace,
    ScmMessage,
    Quick,
    Find,
    Replace,
    Inline,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    #[default]
    File,
    Untitled,
    Settings,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    /// Identity that survives the list moving under it, so a group can name its editor.
    #[serde(default)]
    pub id: u32,
    /// The editor group this tab is open in; 0 until the editor is split.
    #[serde(default)]
    pub group: usize,
    /// Absolute path; `Untitled-N` for an untitled buffer.
    pub path: String,
    pub kind: TabKind,
    pub doc: Doc,
    /// What the file held when last read or written, normalised to `\n`.
    pub saved: String,
    pub loaded: bool,
    /// A preview tab is replaced by the next file opened from a single click.
    pub preview: bool,
    pub lang: Language,
    /// The file uses CRLF line endings on disk.
    pub crlf: bool,
    /// First visual row on screen.
    pub scroll: usize,
    /// Why the file could not be shown, when it could not.
    pub error: Option<String>,
    /// Line and column to put the caret on once the file arrives.
    /// Line, column and length of a range to select once the file arrives.
    pub reveal: Option<(usize, usize, usize)>,
    /// The view follows the caret; a scrollbar click lets it look elsewhere.
    #[serde(default)]
    pub follow: bool,
}
impl Tab {
    pub fn dirty(&self) -> bool {
        match self.kind {
            TabKind::Settings => false,
            _ => self.loaded && self.doc.text != self.saved,
        }
    }
    pub fn name(&self) -> String {
        match self.kind {
            TabKind::Settings => "Settings".into(),
            _ => basename(&self.path).to_owned(),
        }
    }
    pub fn editable(&self) -> bool {
        self.kind != TabKind::Settings && self.loaded && self.error.is_none()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terminal {
    pub name: String,
    pub cwd: String,
    pub prompt: String,
    pub input: String,
    pub cursor: usize,
    pub transcript: Vec<TerminalEntry>,
    pub history: Vec<String>,
    /// Position in `history` while Up/Down recall it.
    pub recall: Option<usize>,
    pub scroll: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    /// 1-based line; byte range of the match within that line.
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub preview: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHits {
    /// Relative to the workspace.
    pub path: String,
    pub hits: Vec<Hit>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Search {
    pub query: String,
    pub replace: String,
    pub opts: FindOptions,
    pub show_replace: bool,
    pub results: Vec<FileHits>,
    pub error: Option<String>,
    pub searched: bool,
    /// Files skipped because they could not be read as text.
    pub skipped: usize,
    pub collapsed: BTreeSet<String>,
}
impl Search {
    pub fn total(&self) -> usize {
        self.results.iter().map(|f| f.hits.len()).sum()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scm {
    /// `None` until git has been asked; `Some(false)` when the folder is no repository.
    pub repo: Option<bool>,
    pub branch: String,
    pub branches: Vec<String>,
    /// (status letter, workspace-relative path)
    pub staged: Vec<(char, String)>,
    pub changes: Vec<(char, String)>,
    pub message: String,
    pub error: Option<String>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickMode {
    /// Files, `>` commands and `:` lines, chosen by the prefix as VS Code does.
    #[default]
    Open,
    Theme,
    Language,
    TabSize,
    Eol,
    OpenFolder,
    SaveAs,
    Branch,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quick {
    pub mode: QuickMode,
    pub value: String,
    pub selected: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Find {
    pub query: String,
    pub replace: String,
    pub opts: FindOptions,
    pub replace_open: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InlineKind {
    NewFile,
    NewFolder,
    Rename,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inline {
    pub kind: InlineKind,
    /// Folder the new entry goes in, relative ("" for the root).
    pub parent: String,
    /// The entry being renamed, relative.
    pub from: Option<String>,
    pub value: String,
}
/// Which context menu is open, and what it is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    /// A right click on an Explorer row (`Workbench::selected` names it).
    Explorer,
    /// A right click in the text of the active editor.
    Editor,
}
impl ContextKind {
    /// The commands the menu offers, `-` being a separator, in VS Code's order.
    pub fn items(self) -> &'static [&'static str] {
        match self {
            Self::Explorer => &[
                "explorer.newFile",
                "explorer.newFolder",
                "-",
                "renameFile",
                "deleteFile",
                "-",
                "copyFilePath",
                "revealFileInOS",
                "openInIntegratedTerminal",
            ],
            Self::Editor => &[
                "editor.action.clipboardCutAction",
                "editor.action.clipboardCopyAction",
                "editor.action.clipboardPasteAction",
                "-",
                "editor.action.revealDefinition",
                "-",
                "workbench.action.showCommands",
            ],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dialog {
    pub message: String,
    pub detail: String,
    /// (label, internal action); the first is the default button.
    pub buttons: Vec<(String, String)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub dark: bool,
    pub font_size: u16,
    pub word_wrap: bool,
    pub tab_size: usize,
    /// `editor.renderWhitespace`: draw a dot for every space and an arrow for every tab.
    #[serde(default)]
    pub render_whitespace: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            dark: true,
            font_size: 14,
            word_wrap: false,
            tab_size: 4,
            render_whitespace: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Mac,
    Windows,
    #[default]
    Linux,
}
/// One choice in the quick input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickItem {
    pub label: String,
    pub detail: String,
    pub keys: String,
    pub action: String,
    /// Character positions of `label` the query matched.
    pub hits: Vec<usize>,
}

/// One editor group: the tabs whose `group` is its index, and which of them it shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// `Tab::id` of the editor on show, or `None` for an empty group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<u32>,
}
/// How the groups divide the editor area: a leaf is one group, a split is groups side by
/// side (`row`) or stacked. Splitting beside a group that is already in a split of that
/// direction joins that split, as VS Code's grid does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Leaf(usize),
    Split { row: bool, children: Vec<Slot> },
}
impl Default for Slot {
    fn default() -> Self {
        Self::Leaf(0)
    }
}
/// A place on screen as `(x, y, width, height)`.
pub type Area = (i32, i32, u32, u32);
/// An editor group and where it is painted.
pub type GroupRect = (usize, Area);

impl Slot {
    /// The groups it holds, left to right and top to bottom.
    pub fn leaves(&self, out: &mut Vec<usize>) {
        match self {
            Self::Leaf(group) => out.push(*group),
            Self::Split { children, .. } => children.iter().for_each(|c| c.leaves(out)),
        }
    }
    /// Put `new` beside `group`, splitting along `row`. Returns whether it was placed.
    fn split(&mut self, group: usize, new: usize, row: bool) -> bool {
        match self {
            Self::Leaf(g) if *g == group => {
                *self = Self::Split {
                    row,
                    children: vec![Self::Leaf(group), Self::Leaf(new)],
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split {
                row: axis,
                children,
            } => {
                if *axis == row {
                    if let Some(at) = children
                        .iter()
                        .position(|c| matches!(c, Self::Leaf(g) if *g == group))
                    {
                        children.insert(at + 1, Self::Leaf(new));
                        return true;
                    }
                }
                children.iter_mut().any(|c| c.split(group, new, row))
            }
        }
    }
    /// Drop `group` and collapse a split left with one child.
    fn remove(&mut self, group: usize) -> bool {
        let Self::Split { children, .. } = self else {
            return false;
        };
        let before = children.len();
        children.retain(|c| !matches!(c, Self::Leaf(g) if *g == group));
        let mut removed = children.len() != before;
        for child in children.iter_mut() {
            removed |= child.remove(group);
        }
        if children.len() == 1 {
            *self = children.remove(0);
        }
        removed
    }
    /// Groups numbered above `gone` move down one, so ids stay the list's indices.
    fn renumber(&mut self, gone: usize) {
        match self {
            Self::Leaf(g) => {
                if *g > gone {
                    *g -= 1;
                }
            }
            Self::Split { children, .. } => children.iter_mut().for_each(|c| c.renumber(gone)),
        }
    }
    /// Where each group is painted inside `r`, dividing it equally at each split.
    pub fn rects(&self, r: Area, out: &mut Vec<GroupRect>) {
        match self {
            Self::Leaf(group) => out.push((*group, r)),
            Self::Split { row, children } => {
                let n = children.len().max(1) as u32;
                for (i, child) in children.iter().enumerate() {
                    let i = i as u32;
                    let part = if *row {
                        let x0 = r.2 * i / n;
                        let x1 = r.2 * (i + 1) / n;
                        (r.0 + x0 as i32, r.1, x1 - x0, r.3)
                    } else {
                        let y0 = r.3 * i / n;
                        let y1 = r.3 * (i + 1) / n;
                        (r.0, r.1 + y0 as i32, r.2, y1 - y0)
                    };
                    child.rects(part, out);
                }
            }
        }
    }
}

/// Visual Studio Code as the desktop holds it: the workbench behind one pointer, so a
/// window's state stays the size of any other application's while it holds tabs,
/// terminals and search results.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Code(pub Box<Workbench>);
impl std::ops::Deref for Code {
    type Target = Workbench;
    fn deref(&self) -> &Workbench {
        &self.0
    }
}
impl std::ops::DerefMut for Code {
    fn deref_mut(&mut self) -> &mut Workbench {
        &mut self.0
    }
}
impl Code {
    pub const KIND: &'static str = Workbench::KIND;
    pub fn launch(argument: &str, window: u64, clock_us: u64) -> (Self, Vec<AppEffect>) {
        let (app, effects) = Workbench::launch(argument, window, clock_us);
        (Self(Box::new(app)), effects)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workbench {
    /// Workspace folder, absolute; `None` shows the Welcome page.
    pub folder: Option<String>,
    pub home: String,
    pub trash: String,
    pub platform: Platform,
    /// Workspace listing, relative, folders with a trailing `/`.
    pub entries: Vec<String>,
    pub truncated: bool,
    pub expanded: BTreeSet<String>,
    pub selected: Option<String>,
    pub tabs: Vec<Tab>,
    /// The editor on show in the focused group, as an index into `tabs`.
    pub active: Option<usize>,
    /// The editor groups; there is always at least one.
    #[serde(default)]
    pub groups: Vec<Group>,
    /// Which group the keyboard, the active editor and a new editor belong to.
    #[serde(default)]
    pub focus_group: usize,
    /// How the groups divide the editor area.
    #[serde(default)]
    pub layout: Slot,
    /// Next `Tab::id`.
    #[serde(default)]
    pub next_tab: u32,
    pub view: View,
    pub sidebar: bool,
    pub panel: PanelTab,
    pub panel_open: bool,
    pub focus: Focus,
    pub terminals: Vec<Terminal>,
    pub term: usize,
    pub problems: Vec<Problem>,
    pub output: Vec<String>,
    pub search: Search,
    pub scm: Scm,
    pub quick: Option<Quick>,
    pub find: Option<Find>,
    pub settings: Settings,
    pub inline: Option<Inline>,
    pub menu: Option<String>,
    pub dialog: Option<Dialog>,
    pub notice: Option<String>,
    /// First half of a `Ctrl+K` chord was pressed.
    pub chord: bool,
    /// Caret offset a pointer press left, so the release extends a selection from it.
    pub press: Option<usize>,
    /// Folder the Open Folder dialog is listing, and what it holds.
    pub browse: String,
    pub browse_entries: Vec<String>,
    /// Launched without a folder: try `~/project`, and show Welcome if it is not there.
    pub probing: bool,
    pub untitled: u32,
    /// Rows the editor showed at the last click, and the wrap width then.
    pub page: usize,
    pub wrap_cols: usize,
    /// Result of the last Run, for the status bar and the Run view.
    pub last_run: Option<(String, i32)>,
    /// Writes in flight, in order, so the answer marks the right tab saved.
    pub writes: Vec<(String, String)>,
    /// The window this instance draws in, for effects raised outside an input event.
    pub window: u64,
    /// Modifier keys held for the pointer press about to arrive: Alt+click adds a cursor.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub modifiers: u8,
    /// The button of that press: 2 is the right one, which opens a context menu.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub button: u8,
    /// The open context menu (a right click on the Explorer or in the editor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextKind>,
    /// Breakpoints, the debug session and what the Run and Debug view shows of it.
    #[serde(default)]
    pub debug: debug::Debug,
}
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

pub fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("/")
}
fn parent(rel: &str) -> &str {
    rel.trim_end_matches('/')
        .rsplit_once('/')
        .map_or("", |(p, _)| p)
}
/// Quote an argument for the machine's shell dialect.
pub fn quote(arg: &str, powershell: bool) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+:@%,=".contains(c))
    {
        return arg.to_owned();
    }
    let escaped = if powershell {
        arg.replace('\'', "'`''")
    } else {
        arg.replace('\'', "'\\''")
    };
    format!("'{escaped}'")
}
fn bounded(field: &mut String, text: &str, limit: usize) -> Result<(), String> {
    if field.len() + text.len() > limit {
        return Err("the field is full".into());
    }
    field.extend(text.chars().filter(|c| !c.is_control() || *c == '\t'));
    Ok(())
}
fn pop_char(field: &mut String) {
    field.pop();
}

impl Workbench {
    pub const KIND: &'static str = "code";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        let mut app = Self {
            window,
            sidebar: true,
            page: 30,
            wrap_cols: 80,
            ..Self::default()
        };
        let folder = argument.trim();
        let mut effects = vec![];
        if !folder.is_empty() {
            let folder = problems::resolve("/", folder);
            effects.push(app.open_folder_effect(window, &folder));
            app.folder = Some(folder);
        }
        (app, effects)
    }
    /// Machine facts the shell supplies at launch: the user's home and trash. Reads the
    /// user's settings, and with no folder given opens `~/project` if there is one.
    pub fn attach(&mut self, home: &str, trash: &str, window: u64) -> Vec<AppEffect> {
        self.home = home.trim_end_matches('/').to_owned();
        if self.home.is_empty() {
            self.home = "/".into();
        }
        self.trash = trash.to_owned();
        self.platform = if self.home.starts_with("/Users/") {
            Platform::Mac
        } else if self.home.contains(":/") || self.home.starts_with("/C:") {
            Platform::Windows
        } else {
            Platform::Linux
        };
        if self.platform == Platform::Mac {
            self.settings.font_size = 12;
        }
        let mut effects = vec![AppEffect::ReadFiles {
            window,
            tag: "settings".into(),
            paths: vec![self.settings_path()],
        }];
        if self.folder.is_none() {
            let project = format!("{}/project", self.home.trim_end_matches('/'));
            self.probing = true;
            effects.push(self.open_folder_effect(window, &project));
            self.folder = Some(project);
        }
        effects
    }
    pub fn settings_path(&self) -> String {
        let home = self.home.trim_end_matches('/');
        match self.platform {
            Platform::Mac => format!("{home}/Library/Application Support/Code/User/settings.json"),
            Platform::Windows => format!("{home}/AppData/Roaming/Code/User/settings.json"),
            Platform::Linux => format!("{home}/.config/Code/User/settings.json"),
        }
    }
    fn powershell(&self) -> bool {
        self.platform == Platform::Windows
    }
    fn open_folder_effect(&self, window: u64, folder: &str) -> AppEffect {
        AppEffect::ListTree {
            window,
            path: folder.to_owned(),
            depth: TREE_DEPTH,
        }
    }
    fn relist(&self, window: u64) -> Vec<AppEffect> {
        self.folder
            .as_ref()
            .map(|f| vec![self.open_folder_effect(window, f)])
            .unwrap_or_default()
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, _theme: DesktopTheme) -> String {
        "Visual Studio Code".into()
    }
    pub fn document(&self) -> String {
        match self.active_tab() {
            Some(tab) if tab.kind == TabKind::File => tab.path.clone(),
            _ => self.folder.clone().unwrap_or_default(),
        }
    }
    /// Workspace name, as the command center shows it.
    pub fn caption(&self) -> String {
        self.folder
            .as_deref()
            .filter(|_| !self.probing)
            .map(|f| basename(f).to_owned())
            .unwrap_or_default()
    }
    pub fn modified(&self) -> bool {
        self.tabs.iter().any(Tab::dirty)
    }
    pub fn dark(&self) -> bool {
        self.settings.dark
    }
    pub fn offline(&mut self, _tag: &str, reason: &str) {
        self.notice = Some(reason.to_owned());
    }
    pub fn http(
        &mut self,
        _window: u64,
        _tag: &str,
        _status: u16,
        _body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        Err("Visual Studio Code makes no network requests here".into())
    }
    pub fn workspace(&self) -> Option<&str> {
        self.folder.as_deref().filter(|_| !self.probing)
    }
    pub fn abs(&self, rel: &str) -> String {
        let root = self.folder.as_deref().unwrap_or("/").trim_end_matches('/');
        if rel.is_empty() {
            return if root.is_empty() {
                "/".into()
            } else {
                root.to_owned()
            };
        }
        format!("{root}/{}", rel.trim_end_matches('/'))
    }
    pub fn rel(&self, abs: &str) -> Option<String> {
        let root = self.workspace()?.trim_end_matches('/');
        if abs == root {
            return Some(String::new());
        }
        abs.strip_prefix(&format!("{root}/")).map(str::to_owned)
    }
    pub fn active_tab(&self) -> Option<&Tab> {
        self.active.and_then(|i| self.tabs.get(i))
    }
    /// How many editor groups there are; there is always at least one.
    pub fn group_count(&self) -> usize {
        self.groups.len().max(1)
    }
    /// The tabs of one group, as indices into `tabs`, in the order they were opened.
    pub fn group_tabs(&self, group: usize) -> Vec<usize> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.group == group)
            .map(|(i, _)| i)
            .collect()
    }
    /// The editor a group shows, as an index into `tabs`.
    pub fn group_active(&self, group: usize) -> Option<usize> {
        if group == self.focus_group {
            return self.active;
        }
        let id = self.groups.get(group)?.active?;
        self.tabs.iter().position(|t| t.id == id)
    }
    /// Show `index` in the focused group. Every path that changes which editor is on
    /// show goes through here, so the group and the workbench never disagree.
    fn set_active(&mut self, index: Option<usize>) {
        if self.groups.is_empty() {
            self.groups.push(Group::default());
        }
        self.focus_group = self.focus_group.min(self.groups.len() - 1);
        // An editor is shown by the group it belongs to.
        if let Some(group) = index.and_then(|i| self.tabs.get(i)).map(|t| t.group) {
            self.focus_group = group.min(self.groups.len() - 1);
        }
        self.active = index;
        let id = index.and_then(|i| self.tabs.get(i)).map(|t| t.id);
        let focus = self.focus_group;
        self.groups[focus].active = id;
    }
    /// Recompute `active` from the focused group's editor: after the tab list moved.
    fn sync_active(&mut self) {
        if self.groups.is_empty() {
            self.groups.push(Group::default());
        }
        self.focus_group = self.focus_group.min(self.groups.len() - 1);
        let id = self.groups[self.focus_group].active;
        self.active = id.and_then(|id| self.tabs.iter().position(|t| t.id == id));
    }
    /// Where each group is painted inside the editor area. A layout that does not name
    /// every group (an old snapshot, say) falls back to one group filling the area.
    pub fn group_rects(&self, area: Area) -> Vec<GroupRect> {
        let mut leaves = vec![];
        self.layout.leaves(&mut leaves);
        let mut sorted = leaves.clone();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted != (0..self.group_count()).collect::<Vec<_>>() {
            return vec![(self.focus_group.min(self.group_count() - 1), area)];
        }
        let mut out = vec![];
        self.layout.rects(area, &mut out);
        out
    }
    /// One file open in two groups is one document, as it is in VS Code: after anything
    /// that can edit it, the other views take the new text and its undo history, keeping
    /// their own caret (clamped into the text that is now there).
    fn sync_siblings(&mut self) {
        let Some(i) = self.active else { return };
        let Some(source) = self.tabs.get(i) else {
            return;
        };
        if source.kind != TabKind::File {
            return;
        }
        let (path, doc, saved) = (
            source.path.clone(),
            source.doc.clone(),
            source.saved.clone(),
        );
        for (j, tab) in self.tabs.iter_mut().enumerate() {
            if j == i || tab.kind != TabKind::File || tab.path != path {
                continue;
            }
            if tab.doc.text == doc.text && tab.saved == saved {
                continue;
            }
            let (cursor, anchor) = (tab.doc.cursor, tab.doc.anchor);
            tab.doc = doc.clone();
            tab.doc.carets.clear();
            tab.doc.set(anchor, false);
            tab.doc.set(cursor, anchor != cursor);
            tab.saved = saved.clone();
        }
    }
    /// Give a group the focus, and with it whatever editor it shows.
    fn go_to_group(&mut self, group: usize) {
        if group >= self.group_count() {
            return;
        }
        self.focus_group = group;
        self.focus = Focus::Editor;
        self.sync_active();
    }
    /// A fresh tab identity.
    fn new_tab_id(&mut self) -> u32 {
        self.next_tab = self.next_tab.wrapping_add(1);
        self.next_tab
    }
    /// Split the focused group, putting a copy of its editor in a new group beside it
    /// (`row`) or below it, and focus the new group, as VS Code's Split Editor does.
    fn split_group(&mut self, row: bool) -> Result<Vec<AppEffect>, String> {
        if self.groups.is_empty() {
            self.groups.push(Group::default());
        }
        if self.groups.len() >= GROUP_LIMIT {
            return Err(format!("at most {GROUP_LIMIT} editor groups"));
        }
        let from = self.focus_group;
        let new = self.groups.len();
        if !self.layout.split(from, new, row) {
            // A layout that lost track of the group starts again from one row.
            self.layout = Slot::Split {
                row,
                children: vec![Slot::Leaf(from), Slot::Leaf(new)],
            };
        }
        self.groups.push(Group::default());
        // The editor on show is opened in the new group too, at the same place in it.
        if let Some(source) = self.active.and_then(|i| self.tabs.get(i)).cloned() {
            let id = self.new_tab_id();
            let tab = Tab {
                id,
                group: new,
                preview: false,
                ..source
            };
            self.tabs.push(tab);
            self.groups[new].active = Some(id);
        }
        self.focus_group = new;
        self.focus = Focus::Editor;
        self.sync_active();
        Ok(vec![])
    }
    /// Close a group, giving its editors to the group before it (VS Code keeps the
    /// editors and closes the empty group).
    fn close_group(&mut self, group: usize) {
        if self.group_count() <= 1 || group >= self.groups.len() {
            return;
        }
        self.layout.remove(group);
        self.layout.renumber(group);
        self.groups.remove(group);
        self.tabs.retain(|t| t.group != group);
        for tab in &mut self.tabs {
            if tab.group > group {
                tab.group -= 1;
            }
        }
        self.focus_group = self.focus_group.min(self.groups.len() - 1);
        self.sync_active();
    }
    fn active_mut(&mut self) -> Option<&mut Tab> {
        self.active.and_then(move |i| self.tabs.get_mut(i))
    }
    fn editor_mut(&mut self) -> Result<&mut Tab, String> {
        let tab = self.active_mut().ok_or("no editor is open")?;
        if !tab.editable() {
            return Err("this editor cannot be edited".into());
        }
        Ok(tab)
    }
    pub fn terminal(&self) -> Option<&Terminal> {
        self.terminals.get(self.term)
    }
    fn log(&mut self, line: impl Into<String>) {
        for l in line.into().lines() {
            self.output.push(l.to_owned());
        }
        let over = self.output.len().saturating_sub(OUTPUT_LIMIT);
        self.output.drain(..over);
    }
    fn is_dir(&self, rel: &str) -> bool {
        rel.is_empty()
            || self
                .entries
                .iter()
                .any(|e| e.trim_end_matches('/') == rel && e.ends_with('/'))
    }
    fn exists(&self, rel: &str) -> bool {
        self.entries.iter().any(|e| e.trim_end_matches('/') == rel)
    }
    /// Files in the workspace, the way Quick Open and Search see them: `.git` excluded.
    pub fn files(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| !e.ends_with('/') && !excluded(e))
            .map(String::as_str)
            .collect()
    }

    // ----- delivery hooks ------------------------------------------------------------

    /// A folder tree arrived: the workspace, or a folder the Open Folder dialog browses.
    pub fn tree_listed(
        &mut self,
        window: u64,
        path: &str,
        depth: u32,
        result: Result<Vec<String>, String>,
    ) -> Vec<AppEffect> {
        if depth == 1 {
            if path == self.browse {
                self.browse_entries = result.unwrap_or_default();
            }
            return vec![];
        }
        if self.folder.as_deref() != Some(path) {
            return vec![];
        }
        match result {
            Ok(mut entries) => {
                let first = self.probing || self.entries.is_empty();
                self.probing = false;
                self.truncated = entries.len() > TREE_LIMIT;
                entries.truncate(TREE_LIMIT);
                entries.retain(|e| !excluded(e));
                self.entries = entries;
                let live: BTreeSet<String> = self
                    .entries
                    .iter()
                    .map(|e| e.trim_end_matches('/').to_owned())
                    .collect();
                self.expanded.retain(|d| live.contains(d));
                if self.selected.as_ref().is_some_and(|s| !live.contains(s)) {
                    self.selected = None;
                }
                if first {
                    // A newly opened folder: ask git about it, as the Git extension does.
                    return vec![self.git(window, "status", "git status")];
                }
                vec![]
            }
            Err(reason) => {
                if self.probing {
                    // No ~/project: an empty window with the Welcome page.
                    self.probing = false;
                    self.folder = None;
                } else {
                    self.notice = Some(format!("Unable to resolve workspace folder: {reason}"));
                    self.folder = None;
                    self.entries.clear();
                }
                vec![]
            }
        }
    }
    pub fn files_read(
        &mut self,
        window: u64,
        tag: &str,
        files: Vec<(String, Result<String, String>)>,
    ) -> Vec<AppEffect> {
        match tag {
            "settings" => {
                if let Some((_, Ok(text))) = files.first() {
                    self.apply_settings_json(text);
                }
                vec![]
            }
            "open" | "reload" => {
                for (path, result) in files {
                    let Some(tab) = self
                        .tabs
                        .iter_mut()
                        .find(|t| t.kind == TabKind::File && t.path == path)
                    else {
                        continue;
                    };
                    match result {
                        Ok(raw) => {
                            let crlf = raw.contains("\r\n");
                            let text = raw.replace("\r\n", "\n");
                            if tag == "reload" {
                                // An unmodified editor follows the file on disk.
                                if tab.dirty() || text == tab.saved {
                                    continue;
                                }
                                tab.doc.replace_all_text(&text);
                                tab.saved = text;
                                tab.error = None;
                                continue;
                            }
                            tab.doc = Doc::new(text.clone());
                            tab.saved = text;
                            tab.crlf = crlf;
                            tab.loaded = true;
                            tab.error = None;
                            if let Some(at) = tab.reveal.take() {
                                reveal(&mut tab.doc, at);
                            }
                        }
                        Err(reason) if tag == "open" => {
                            tab.error = Some(reason);
                            tab.loaded = false;
                        }
                        Err(_) => {}
                    }
                }
                self.follow_caret();
                vec![]
            }
            "search" => {
                self.search_files(files);
                vec![]
            }
            _ if tag.starts_with("definition:") => {
                let word = tag.trim_start_matches("definition:").to_owned();
                self.reveal_definition(window, &word, files)
            }
            "replace" => self.replace_in_files(window, files),
            _ => vec![],
        }
    }
    /// A write reached the disk: the tab holding that text is clean again.
    pub fn written(&mut self, window: u64, path: &str, content: &str) -> Vec<AppEffect> {
        if let Some(i) = self.writes.iter().position(|(p, _)| p == path) {
            self.writes.remove(i);
        }
        let text = content.replace("\r\n", "\n");
        for tab in &mut self.tabs {
            if tab.kind == TabKind::File && tab.path == path {
                tab.saved = text.clone();
            }
        }
        let mut effects = vec![];
        if path == self.settings_path() {
            self.apply_settings_json(content);
        }
        // Saving changes what git sees.
        if self.scm.repo == Some(true) && self.rel(path).is_some() {
            effects.push(self.git(window, "status", "git status"));
        }
        effects
    }
    pub fn shell_ran(&mut self, window: u64, tag: &str, outcome: ShellOutcome) -> Vec<AppEffect> {
        if let Some(rest) = tag.strip_prefix("git:") {
            return self.git_finished(window, rest, outcome);
        }
        let (kind, index) = tag.split_once(':').unwrap_or((tag, "0"));
        let index: usize = index.parse().unwrap_or(0);
        let Some(term) = self.terminals.get_mut(index) else {
            return vec![];
        };
        let before = term.cwd.clone();
        term.cwd = outcome.cwd.clone();
        term.prompt = outcome.prompt.clone();
        term.scroll = 0;
        let Some(entry) = outcome.entry else {
            return vec![];
        };
        if outcome.clear {
            term.transcript.clear();
        } else {
            term.transcript.push(entry.clone());
            let over = term.transcript.len().saturating_sub(TRANSCRIPT_LIMIT);
            term.transcript.drain(..over);
        }
        let program = entry
            .command
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_owned();
        let interpreter = matches!(
            basename(&program),
            "python" | "python3" | "node" | "bash" | "sh"
        );
        if kind == "run" || interpreter {
            let found = problems::from_output(&program, &entry.stdout, &entry.stderr, &before);
            self.problems.retain(|p| p.source == "json");
            self.problems.extend(found);
            if kind == "run" {
                self.last_run = Some((entry.command.clone(), entry.exit_code));
            }
        }
        // The command may have changed files: refresh the tree, git, and any editor that
        // is showing a file without unsaved changes, as VS Code's file watcher would.
        let mut effects = self.relist(window);
        if self.scm.repo.is_some() {
            effects.push(self.git(window, "status", "git status"));
        }
        let clean: Vec<String> = self
            .tabs
            .iter()
            .filter(|t| t.kind == TabKind::File && t.loaded && !t.dirty())
            .map(|t| t.path.clone())
            .collect();
        if !clean.is_empty() {
            effects.push(AppEffect::ReadFiles {
                window,
                tag: "reload".into(),
                paths: clean,
            });
        }
        effects
    }

    // ----- git -------------------------------------------------------------------------

    fn git(&self, window: u64, tag: &str, command: &str) -> AppEffect {
        AppEffect::ShellRun {
            window,
            tag: format!("git:{tag}"),
            cwd: self.folder.clone().unwrap_or_else(|| self.home.clone()),
            command: command.to_owned(),
        }
    }
    fn git_finished(&mut self, window: u64, tag: &str, outcome: ShellOutcome) -> Vec<AppEffect> {
        let Some(entry) = outcome.entry else {
            return vec![];
        };
        self.log(format!("> {}", entry.command));
        if !entry.stderr.trim().is_empty() {
            self.log(entry.stderr.trim_end().to_owned());
        }
        match tag {
            "status" => {
                if entry.exit_code != 0 {
                    if entry.stderr.contains("not a git repository") {
                        self.scm = Scm {
                            repo: Some(false),
                            message: std::mem::take(&mut self.scm.message),
                            ..Scm::default()
                        };
                    } else {
                        self.scm.error = Some(entry.stderr.trim().to_owned());
                    }
                    return vec![];
                }
                self.scm.repo = Some(true);
                self.scm.error = None;
                self.scm.staged.clear();
                self.scm.changes.clear();
                for line in entry.stdout.lines() {
                    if let Some(branch) = line.strip_prefix("On branch ") {
                        self.scm.branch = branch.trim().to_owned();
                        continue;
                    }
                    let mut chars = line.chars();
                    let (Some(x), Some(y)) = (chars.next(), chars.next()) else {
                        continue;
                    };
                    let path = chars.as_str().trim().to_owned();
                    if path.is_empty() {
                        continue;
                    }
                    if x != ' ' {
                        self.scm.staged.push((x, path.clone()));
                    }
                    if y != ' ' {
                        // In the working tree but not in the index at all: untracked.
                        let letter = if y == 'A' && x == ' ' { 'U' } else { y };
                        self.scm.changes.push((letter, path));
                    }
                }
                vec![]
            }
            "branches" => {
                if entry.exit_code == 0 {
                    self.scm.branches = entry
                        .stdout
                        .lines()
                        .map(|l| l.trim_start_matches(['*', ' ']).trim().to_owned())
                        .filter(|l| !l.is_empty())
                        .collect();
                }
                vec![]
            }
            _ => {
                if entry.exit_code != 0 {
                    let why = entry.stderr.trim().trim_start_matches("git: ").to_owned();
                    self.notice = Some(format!("Git: {why}"));
                } else if tag == "commit" {
                    self.scm.message.clear();
                }
                let mut effects = vec![self.git(window, "status", "git status")];
                if tag == "checkout" {
                    effects.extend(self.relist(window));
                    let clean: Vec<String> = self
                        .tabs
                        .iter()
                        .filter(|t| t.kind == TabKind::File && t.loaded && !t.dirty())
                        .map(|t| t.path.clone())
                        .collect();
                    effects.push(AppEffect::ReadFiles {
                        window,
                        tag: "reload".into(),
                        paths: clean,
                    });
                }
                effects
            }
        }
    }
    fn git_command(
        &mut self,
        window: u64,
        tag: &str,
        command: String,
    ) -> Result<Vec<AppEffect>, String> {
        if self.workspace().is_none() {
            return Err("open a folder first".into());
        }
        Ok(vec![self.git(window, tag, &command)])
    }

    // ----- settings --------------------------------------------------------------------

    fn apply_settings_json(&mut self, text: &str) {
        let Some(value) = problems::parse_jsonc(text) else {
            return;
        };
        if let Some(theme) = value.get("workbench.colorTheme").and_then(|v| v.as_str()) {
            self.settings.dark = !theme.to_ascii_lowercase().contains("light");
        }
        if let Some(size) = value.get("editor.fontSize").and_then(|v| v.as_u64()) {
            self.settings.font_size = size.clamp(8, 32) as u16;
        }
        if let Some(wrap) = value.get("editor.wordWrap").and_then(|v| v.as_str()) {
            self.settings.word_wrap = wrap != "off";
        }
        if let Some(tab) = value.get("editor.tabSize").and_then(|v| v.as_u64()) {
            self.settings.tab_size = tab.clamp(1, 8) as usize;
        }
        if let Some(show) = value
            .get("editor.renderWhitespace")
            .and_then(|v| v.as_str())
        {
            self.settings.render_whitespace = show != "none";
        }
    }
    pub fn settings_json(&self) -> String {
        let value = serde_json::json!({
            "workbench.colorTheme": if self.settings.dark { "Default Dark Modern" } else { "Default Light Modern" },
            "editor.fontSize": self.settings.font_size,
            "editor.wordWrap": if self.settings.word_wrap { "on" } else { "off" },
            "editor.tabSize": self.settings.tab_size,
            "editor.renderWhitespace": if self.settings.render_whitespace { "all" } else { "none" },
        });
        let mut out = serde_json::to_string_pretty(&value).unwrap_or_default();
        out.push('\n');
        out
    }
    /// Persist the settings where VS Code keeps them, and keep an open settings.json
    /// editor showing what is on disk.
    fn save_settings(&mut self, window: u64) -> Vec<AppEffect> {
        let path = self.settings_path();
        let content = self.settings_json();
        for tab in &mut self.tabs {
            if tab.kind == TabKind::File && tab.path == path && !tab.dirty() {
                tab.doc.replace_all_text(&content);
            }
        }
        self.writes.push((path.clone(), content.clone()));
        vec![
            AppEffect::CreateDirectory {
                window,
                path: parent(&path).to_owned(),
            },
            AppEffect::WriteFile {
                window,
                path,
                content,
            },
        ]
    }

    // ----- tabs ------------------------------------------------------------------------

    /// Open `path` in an editor: focus it if open, otherwise a preview tab (replacing
    /// the current preview) or a pinned one.
    pub fn open_file(
        &mut self,
        window: u64,
        path: &str,
        pin: bool,
        reveal_at: Option<(usize, usize, usize)>,
    ) -> Vec<AppEffect> {
        self.focus = Focus::Editor;
        self.menu = None;
        // An editor already open in this group is the one that is shown.
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.kind == TabKind::File && t.path == path && t.group == self.focus_group)
        {
            self.set_active(Some(i));
            let tab = &mut self.tabs[i];
            if pin {
                tab.preview = false;
            }
            if let Some(at) = reveal_at {
                if tab.loaded {
                    reveal(&mut tab.doc, at);
                } else {
                    tab.reveal = Some(at);
                }
            }
            self.follow_caret();
            return vec![];
        }
        let id = self.new_tab_id();
        let tab = Tab {
            id,
            group: self.focus_group,
            path: path.to_owned(),
            kind: TabKind::File,
            preview: !pin,
            lang: Language::from_path(path),
            reveal: reveal_at,
            ..Tab::default()
        };
        // A preview tab is replaced only inside the group it belongs to.
        let slot = self
            .tabs
            .iter()
            .position(|t| t.preview && !t.dirty() && t.group == self.focus_group)
            .filter(|_| !pin);
        let index = match slot {
            Some(i) => {
                self.tabs[i] = tab;
                i
            }
            None => {
                let at = self
                    .active
                    .map_or(self.tabs.len(), |a| (a + 1).min(self.tabs.len()));
                self.tabs.insert(at, tab);
                at
            }
        };
        self.set_active(Some(index));
        if let Some(rel) = self.rel(path) {
            self.selected = Some(rel.clone());
            // Reveal it in the tree, as `explorer.autoReveal` does.
            let mut dir = parent(&rel).to_owned();
            while !dir.is_empty() {
                self.expanded.insert(dir.clone());
                dir = parent(&dir).to_owned();
            }
        }
        vec![AppEffect::ReadFiles {
            window,
            tag: "open".into(),
            paths: vec![path.to_owned()],
        }]
    }
    fn close_tab(&mut self, index: usize, force: bool) -> Result<(), String> {
        let tab = self.tabs.get(index).ok_or("no such editor")?;
        if tab.dirty() && !force {
            self.dialog = Some(Dialog {
                message: format!(
                    "Do you want to save the changes you made to {}?",
                    tab.name()
                ),
                detail: "Your changes will be lost if you don't save them.".into(),
                buttons: vec![
                    ("Save".into(), format!("save-close:{index}")),
                    ("Don't Save".into(), format!("discard-close:{index}")),
                    ("Cancel".into(), "cancel".into()),
                ],
            });
            return Ok(());
        }
        let group = self.tabs[index].group;
        let gone = self.tabs[index].id;
        // Where the closed editor sat among its group's, so its neighbour takes over.
        let siblings: Vec<u32> = self
            .tabs
            .iter()
            .filter(|t| t.group == group)
            .map(|t| t.id)
            .collect();
        let at = siblings.iter().position(|id| *id == gone).unwrap_or(0);
        self.tabs.remove(index);
        for g in 0..self.groups.len() {
            if self.groups[g].active != Some(gone) {
                continue;
            }
            let mine: Vec<u32> = self
                .tabs
                .iter()
                .filter(|t| t.group == g)
                .map(|t| t.id)
                .collect();
            self.groups[g].active = mine.get(at.min(mine.len().saturating_sub(1))).copied();
        }
        // A split group with nothing left in it closes, as VS Code closes it.
        if self.group_count() > 1 && !self.tabs.iter().any(|t| t.group == group) {
            self.close_group(group);
        } else {
            self.sync_active();
        }
        if self.tabs.is_empty() {
            self.find = None;
        }
        Ok(())
    }
    pub(super) fn save_tab(&mut self, window: u64, index: usize) -> Result<Vec<AppEffect>, String> {
        let tab = self.tabs.get(index).ok_or("no editor is open")?;
        match tab.kind {
            TabKind::Settings => return Ok(vec![]),
            TabKind::Untitled => {
                self.set_active(Some(index));
                return self.open_quick(QuickMode::SaveAs, &self.default_save_path());
            }
            TabKind::File => {}
        }
        if !tab.loaded {
            return Err("the file has not been read yet".into());
        }
        let content = if tab.crlf {
            tab.doc.text.replace('\n', "\r\n")
        } else {
            tab.doc.text.clone()
        };
        let path = tab.path.clone();
        self.tabs[index].preview = false;
        self.writes.push((path.clone(), content.clone()));
        Ok(vec![AppEffect::WriteFile {
            window,
            path,
            content,
        }])
    }
    fn default_save_path(&self) -> String {
        let name = self.active_tab().map(|t| t.name()).unwrap_or_default();
        let base = self.workspace().map_or(self.home.clone(), str::to_owned);
        format!("{}/{}.txt", base.trim_end_matches('/'), name)
    }
    /// Save As: the buffer becomes that file.
    fn save_as(&mut self, window: u64, path: &str) -> Result<Vec<AppEffect>, String> {
        let path = problems::resolve(&self.home, path.trim());
        if basename(&path).is_empty() || path.ends_with('/') {
            return Err("that is not a file name".into());
        }
        let index = self.active.ok_or("no editor is open")?;
        let tab = self.tabs.get_mut(index).ok_or("no editor is open")?;
        tab.path = path.clone();
        tab.kind = TabKind::File;
        tab.lang = Language::from_path(&path);
        tab.loaded = true;
        tab.preview = false;
        tab.saved = String::new();
        let mut effects = vec![AppEffect::CreateDirectory {
            window,
            path: parent(&path).to_owned(),
        }];
        effects.extend(self.save_tab(window, index)?);
        effects.extend(self.relist(window));
        Ok(effects)
    }

    // ----- terminal --------------------------------------------------------------------

    fn new_terminal(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        if self.terminals.len() >= TERMINAL_LIMIT {
            return Err(format!("at most {TERMINAL_LIMIT} terminals"));
        }
        let cwd = self.workspace().map_or(self.home.clone(), str::to_owned);
        let name = match self.platform {
            Platform::Mac => "zsh",
            Platform::Windows => "powershell",
            Platform::Linux => "bash",
        };
        self.terminals.push(Terminal {
            name: name.into(),
            cwd: cwd.clone(),
            ..Terminal::default()
        });
        self.term = self.terminals.len() - 1;
        self.panel_open = true;
        self.panel = PanelTab::Terminal;
        self.focus = Focus::Terminal;
        Ok(vec![AppEffect::ShellRun {
            window,
            tag: format!("prompt:{}", self.term),
            cwd,
            command: String::new(),
        }])
    }
    fn ensure_terminal(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        if self.terminals.is_empty() {
            return self.new_terminal(window);
        }
        self.panel_open = true;
        self.panel = PanelTab::Terminal;
        Ok(vec![])
    }
    fn send_to_terminal(&mut self, window: u64, command: String, kind: &str) -> Vec<AppEffect> {
        let index = self.term;
        let Some(term) = self.terminals.get_mut(index) else {
            return vec![];
        };
        if !command.trim().is_empty() {
            term.history.push(command.clone());
            let over = term.history.len().saturating_sub(100);
            term.history.drain(..over);
        }
        term.recall = None;
        vec![AppEffect::ShellRun {
            window,
            tag: format!("{kind}:{index}"),
            cwd: term.cwd.clone(),
            command,
        }]
    }
    /// Run the active file with its language's interpreter in the terminal, saving it
    /// first as the Python and Code Runner extensions do.
    fn run_active(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let index = self.active.ok_or("open a file to run it")?;
        let tab = &self.tabs[index];
        if tab.kind != TabKind::File {
            return Err("save the file before running it".into());
        }
        let runner = tab
            .lang
            .runner(self.platform == Platform::Windows)
            .ok_or_else(|| format!("{} files cannot be run", tab.lang.name()))?;
        let path = tab.path.clone();
        let mut effects = vec![];
        if tab.dirty() {
            effects.extend(self.save_tab(window, index)?);
        }
        effects.extend(self.ensure_terminal(window)?);
        let cwd = self.terminals[self.term].cwd.clone();
        let shown = path
            .strip_prefix(&format!("{}/", cwd.trim_end_matches('/')))
            .map_or(path.clone(), str::to_owned);
        let command = format!("{runner} {}", quote(&shown, self.powershell()));
        effects.extend(self.send_to_terminal(window, command, "run"));
        self.panel_open = true;
        self.panel = PanelTab::Terminal;
        Ok(effects)
    }

    // ----- search ----------------------------------------------------------------------

    fn start_search(&mut self, window: u64) -> Vec<AppEffect> {
        self.search.results.clear();
        self.search.error = None;
        self.search.skipped = 0;
        self.search.searched = false;
        if self.search.query.is_empty() || self.workspace().is_none() {
            return vec![];
        }
        if let Err(e) = find_all("", &self.search.query, self.search.opts) {
            self.search.error = Some(e);
            return vec![];
        }
        let paths: Vec<String> = self
            .files()
            .into_iter()
            .take(SEARCH_FILES)
            .map(|f| self.abs(f))
            .collect();
        if paths.is_empty() {
            self.search.searched = true;
            return vec![];
        }
        vec![AppEffect::ReadFiles {
            window,
            tag: "search".into(),
            paths,
        }]
    }
    /// Where `word` is defined, if the workspace defines it: the first file, by name,
    /// that has a definition of it in its language's shape. The editor that is open on
    /// a file is what is searched, so an unsaved definition counts.
    fn reveal_definition(
        &mut self,
        window: u64,
        word: &str,
        files: Vec<(String, Result<String, String>)>,
    ) -> Vec<AppEffect> {
        let mut found: Option<(String, usize, usize)> = None;
        for (path, result) in files {
            let text = match self
                .tabs
                .iter()
                .find(|t| t.kind == TabKind::File && t.path == path && t.loaded)
            {
                Some(tab) => tab.doc.text.clone(),
                None => match result {
                    Ok(text) => text.replace("\r\n", "\n"),
                    Err(_) => continue,
                },
            };
            if let Some((line, col)) = definition_in(&text, word) {
                found = Some((path, line, col));
                break;
            }
        }
        match found {
            Some((path, line, col)) => {
                self.open_file(window, &path, true, Some((line, col, word.len())))
            }
            None => {
                self.notice = Some(format!("No definition found for '{word}'"));
                vec![]
            }
        }
    }
    fn search_files(&mut self, files: Vec<(String, Result<String, String>)>) {
        let mut total = 0;
        self.search.results.clear();
        for (path, result) in files {
            // An open editor's unsaved text is what is searched, as VS Code does.
            let text = match self
                .tabs
                .iter()
                .find(|t| t.kind == TabKind::File && t.path == path && t.loaded)
            {
                Some(tab) => tab.doc.text.clone(),
                None => match result {
                    Ok(text) => text.replace("\r\n", "\n"),
                    Err(_) => {
                        self.search.skipped += 1;
                        continue;
                    }
                },
            };
            let Ok(found) = find_all(&text, &self.search.query, self.search.opts) else {
                continue;
            };
            if found.is_empty() {
                continue;
            }
            let mut hits = Vec::new();
            for (a, b) in found {
                if total >= HIT_LIMIT {
                    break;
                }
                let line_start = text[..a].rfind('\n').map_or(0, |i| i + 1);
                let line_end = text[a..].find('\n').map_or(text.len(), |i| a + i);
                let line = text[..a].bytes().filter(|c| *c == b'\n').count() + 1;
                hits.push(Hit {
                    line,
                    start: a - line_start,
                    end: b.min(line_end) - line_start,
                    preview: text[line_start..line_end].to_owned(),
                });
                total += 1;
            }
            let rel = self.rel(&path).unwrap_or(path);
            self.search.results.push(FileHits { path: rel, hits });
        }
        self.search.searched = true;
    }
    fn replace_in_files(
        &mut self,
        window: u64,
        files: Vec<(String, Result<String, String>)>,
    ) -> Vec<AppEffect> {
        let mut effects = vec![];
        let mut count = 0;
        let mut changed = 0;
        for (path, result) in files {
            let open = self
                .tabs
                .iter()
                .position(|t| t.kind == TabKind::File && t.path == path && t.loaded);
            let text = match (open, result) {
                (Some(i), _) => self.tabs[i].doc.text.clone(),
                (None, Ok(t)) => t.replace("\r\n", "\n"),
                (None, Err(_)) => continue,
            };
            let Ok(found) = find_all(&text, &self.search.query, self.search.opts) else {
                continue;
            };
            if found.is_empty() {
                continue;
            }
            count += found.len();
            changed += 1;
            let mut out = String::with_capacity(text.len());
            let mut last = 0;
            for (a, b) in &found {
                out.push_str(&text[last..*a]);
                out.push_str(&self.search.replace);
                last = *b;
            }
            out.push_str(&text[last..]);
            if let Some(i) = open {
                self.tabs[i].doc.replace_all_text(&out);
                if let Ok(mut e) = self.save_tab(window, i) {
                    effects.append(&mut e);
                }
            } else {
                self.writes.push((path.clone(), out.clone()));
                effects.push(AppEffect::WriteFile {
                    window,
                    path,
                    content: out,
                });
            }
        }
        self.notice = Some(format!(
            "Replaced {count} occurrence{} across {changed} file{} with '{}'.",
            if count == 1 { "" } else { "s" },
            if changed == 1 { "" } else { "s" },
            self.search.replace
        ));
        self.search.results.clear();
        effects
    }

    // ----- quick input -----------------------------------------------------------------

    fn open_quick(&mut self, mode: QuickMode, value: &str) -> Result<Vec<AppEffect>, String> {
        self.menu = None;
        self.quick = Some(Quick {
            mode,
            value: value.to_owned(),
            selected: 0,
        });
        self.focus = Focus::Quick;
        if mode == QuickMode::OpenFolder {
            return Ok(self.browse_to(value));
        }
        Ok(vec![])
    }
    /// Point the Open Folder dialog at the folder part of what is typed.
    fn browse_to(&mut self, value: &str) -> Vec<AppEffect> {
        let dir = if value.ends_with('/') {
            value.trim_end_matches('/').to_owned()
        } else {
            parent(value).to_owned()
        };
        let dir = if dir.is_empty() { "/".to_owned() } else { dir };
        if dir == self.browse && !self.browse_entries.is_empty() {
            return vec![];
        }
        self.browse = dir.clone();
        self.browse_entries.clear();
        vec![AppEffect::ListTree {
            window: self.window,
            path: dir,
            depth: 1,
        }]
    }
    pub fn quick_items(&self) -> Vec<QuickItem> {
        let Some(quick) = &self.quick else {
            return vec![];
        };
        let item = |label: &str, detail: &str, action: String| QuickItem {
            label: label.to_owned(),
            detail: detail.to_owned(),
            keys: String::new(),
            action,
            hits: vec![],
        };
        let pick = |choices: Vec<(String, String, String)>, value: &str| -> Vec<QuickItem> {
            let mut scored: Vec<(i32, usize, QuickItem)> = choices
                .into_iter()
                .enumerate()
                .filter_map(|(i, (label, detail, action))| {
                    let (score, hits) = problems::fuzzy(value, &label)?;
                    Some((
                        score,
                        i,
                        QuickItem {
                            label,
                            detail,
                            keys: String::new(),
                            action,
                            hits,
                        },
                    ))
                })
                .collect();
            if !value.trim().is_empty() {
                scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            }
            scored.into_iter().map(|(_, _, i)| i).collect()
        };
        match quick.mode {
            QuickMode::Open => {
                if let Some(query) = quick.value.strip_prefix('>') {
                    let mut items: Vec<(i32, usize, QuickItem)> = commands::COMMANDS
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| self.enabled(c.id).is_ok())
                        .filter_map(|(i, c)| {
                            let (score, hits) = problems::fuzzy(query, c.title)?;
                            Some((
                                score,
                                i,
                                QuickItem {
                                    label: c.title.to_owned(),
                                    detail: String::new(),
                                    keys: c.keys.to_owned(),
                                    action: format!("cmd:{}", c.id),
                                    hits,
                                },
                            ))
                        })
                        .collect();
                    if !query.trim().is_empty() {
                        items.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                    } else {
                        items.sort_by(|a, b| a.2.label.cmp(&b.2.label));
                    }
                    return items.into_iter().map(|(_, _, i)| i).collect();
                }
                if let Some(target) = quick.value.strip_prefix(':') {
                    let lines = self.active_tab().map_or(0, |t| t.doc.line_count());
                    let label = match parse_line(target) {
                        Some((line, col)) if lines > 0 => {
                            if col > 1 {
                                format!("Go to line {line}, character {col}.")
                            } else {
                                format!("Go to line {line}.")
                            }
                        }
                        _ if lines > 0 => {
                            let (l, c) = self.active_tab().map_or((0, 0), |t| t.doc.position());
                            format!(
                                "Current Line: {}, Character: {}. Type a line number between 1 and {lines} to navigate to.",
                                l + 1,
                                c + 1
                            )
                        }
                        _ => "Open a text editor first to go to a line.".into(),
                    };
                    let action = match parse_line(target) {
                        Some((l, c)) if lines > 0 => format!("goto:{l}:{c}"),
                        _ => String::new(),
                    };
                    return vec![item(&label, "", action)];
                }
                // Recently opened editors first, then the workspace, best match first.
                let query = quick.value.trim();
                let mut items: Vec<(i32, String, QuickItem)> = self
                    .files()
                    .into_iter()
                    .filter_map(|rel| {
                        let score = problems::fuzzy_path(query, rel)?;
                        let name = basename(rel);
                        let hits = problems::fuzzy(query, name)
                            .map(|h| h.1)
                            .unwrap_or_default();
                        let open_bonus = if self.tabs.iter().any(|t| t.path == self.abs(rel)) {
                            50
                        } else {
                            0
                        };
                        Some((
                            score + open_bonus,
                            rel.to_owned(),
                            QuickItem {
                                label: name.to_owned(),
                                detail: parent(rel).to_owned(),
                                keys: String::new(),
                                action: format!("open:{rel}"),
                                hits,
                            },
                        ))
                    })
                    .collect();
                items.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                items.truncate(200);
                items.into_iter().map(|(_, _, i)| i).collect()
            }
            QuickMode::Theme => pick(
                vec![
                    (
                        "Dark Modern".into(),
                        "dark themes".into(),
                        "theme:dark".into(),
                    ),
                    (
                        "Light Modern".into(),
                        "light themes".into(),
                        "theme:light".into(),
                    ),
                ],
                &quick.value,
            ),
            QuickMode::Language => pick(
                Language::ALL
                    .iter()
                    .map(|l| {
                        (
                            l.name().to_owned(),
                            format!("({})", l.id()),
                            format!("lang:{}", l.id()),
                        )
                    })
                    .collect(),
                &quick.value,
            ),
            QuickMode::TabSize => pick(
                (1..=8)
                    .map(|n| {
                        (
                            n.to_string(),
                            if n == self.settings.tab_size {
                                "Configured Tab Size".into()
                            } else {
                                String::new()
                            },
                            format!("tab:{n}"),
                        )
                    })
                    .collect(),
                &quick.value,
            ),
            QuickMode::Eol => pick(
                vec![
                    ("LF".into(), String::new(), "eol:lf".into()),
                    ("CRLF".into(), String::new(), "eol:crlf".into()),
                ],
                &quick.value,
            ),
            QuickMode::Branch => pick(
                self.scm
                    .branches
                    .iter()
                    .map(|b| {
                        (
                            b.clone(),
                            if *b == self.scm.branch {
                                "current".into()
                            } else {
                                String::new()
                            },
                            format!("checkout:{b}"),
                        )
                    })
                    .collect(),
                &quick.value,
            ),
            QuickMode::OpenFolder => {
                let typed = basename(&quick.value);
                let filter = if quick.value.ends_with('/') {
                    ""
                } else {
                    typed
                };
                let mut out = vec![];
                if self.browse != "/" {
                    out.push(item("..", "", format!("browse:{}", parent(&self.browse))));
                }
                for entry in &self.browse_entries {
                    let Some(name) = entry.strip_suffix('/') else {
                        continue;
                    };
                    if name.starts_with('.') && !filter.starts_with('.') {
                        continue;
                    }
                    if !name.to_lowercase().starts_with(&filter.to_lowercase()) {
                        continue;
                    }
                    let path = format!("{}/{name}", self.browse.trim_end_matches('/'));
                    out.push(item(name, "", format!("browse:{path}")));
                }
                out
            }
            QuickMode::SaveAs => vec![item(
                "Save",
                &quick.value,
                format!("saveas:{}", quick.value),
            )],
        }
    }
    fn accept_quick(
        &mut self,
        window: u64,
        index: Option<usize>,
    ) -> Result<Vec<AppEffect>, String> {
        let items = self.quick_items();
        let quick = self.quick.clone().ok_or("no quick input is open")?;
        if quick.mode == QuickMode::OpenFolder && index.is_none() {
            // Enter in the folder dialog opens what is typed, as its OK button does.
            let path = quick.value.trim_end_matches('/').to_owned();
            return self.open_folder(window, &path);
        }
        let chosen = items
            .get(index.unwrap_or(quick.selected))
            .ok_or("nothing matches")?
            .action
            .clone();
        if chosen.is_empty() {
            return Err("type a line number".into());
        }
        if !chosen.starts_with("browse:") {
            self.quick = None;
            self.focus = Focus::Editor;
        }
        self.quick_action(window, &chosen)
    }
    fn quick_action(&mut self, window: u64, action: &str) -> Result<Vec<AppEffect>, String> {
        let (verb, arg) = action.split_once(':').unwrap_or((action, ""));
        match verb {
            "cmd" => self.run_command(window, arg),
            "open" => {
                let abs = self.abs(arg);
                Ok(self.open_file(window, &abs, true, None))
            }
            "goto" => {
                let (l, c) = arg.split_once(':').unwrap_or((arg, "1"));
                let tab = self.editor_mut()?;
                tab.doc.goto(l.parse().unwrap_or(1), c.parse().unwrap_or(1));
                self.follow_caret();
                Ok(vec![])
            }
            "theme" => {
                self.settings.dark = arg == "dark";
                Ok(self.save_settings(window))
            }
            "lang" => {
                let lang = Language::from_id(arg).ok_or("unknown language")?;
                self.active_mut().ok_or("no editor is open")?.lang = lang;
                Ok(vec![])
            }
            "tab" => {
                self.settings.tab_size = arg
                    .parse::<usize>()
                    .map_err(|_| "invalid tab size")?
                    .clamp(1, 8);
                Ok(self.save_settings(window))
            }
            "eol" => {
                let tab = self.editor_mut()?;
                let crlf = arg == "crlf";
                if tab.crlf != crlf {
                    tab.crlf = crlf;
                    // The text model is the same; the file on disk differs, so it is dirty.
                    tab.saved = format!("{}\u{0}", tab.saved);
                }
                Ok(vec![])
            }
            "checkout" => self.git_command(
                window,
                "checkout",
                format!("git checkout {}", quote(arg, self.powershell())),
            ),
            "browse" => {
                let path = if arg.is_empty() {
                    "/".to_owned()
                } else {
                    arg.to_owned()
                };
                if let Some(q) = &mut self.quick {
                    q.value = format!("{}/", path.trim_end_matches('/'));
                    q.selected = 0;
                }
                self.browse_entries.clear();
                self.browse.clear();
                let value = self
                    .quick
                    .as_ref()
                    .map(|q| q.value.clone())
                    .unwrap_or_default();
                Ok(self.browse_to(&value))
            }
            "saveas" => self.save_as(window, arg),
            _ => Err(format!("unknown quick action {verb}")),
        }
    }
    fn open_folder(&mut self, window: u64, path: &str) -> Result<Vec<AppEffect>, String> {
        let path = problems::resolve("/", path);
        if self.tabs.iter().any(Tab::dirty) {
            return Err("save or close the editors with unsaved changes first".into());
        }
        self.reset(Some(path.clone()));
        Ok(vec![self.open_folder_effect(window, &path)])
    }
    /// A fresh window on `folder`, keeping what belongs to the user rather than the folder.
    fn reset(&mut self, folder: Option<String>) {
        *self = Self {
            home: self.home.clone(),
            trash: self.trash.clone(),
            platform: self.platform,
            settings: self.settings,
            sidebar: true,
            page: self.page,
            wrap_cols: self.wrap_cols,
            untitled: self.untitled,
            window: self.window,
            output: std::mem::take(&mut self.output),
            folder,
            ..Self::default()
        };
    }

    // ----- commands --------------------------------------------------------------------

    /// Whether a command can run now, and if not, why — the reason a menu shows.
    pub fn enabled(&self, id: &str) -> Result<(), &'static str> {
        let editor = self.active_tab().is_some_and(Tab::editable);
        let tab = self.active_tab();
        let folder = self.workspace().is_some();
        let repo = self.scm.repo == Some(true);
        let runnable =
            tab.is_some_and(|t| t.kind == TabKind::File && t.lang.runner(false).is_some());
        let ok = match id {
            "workbench.action.files.save"
            | "workbench.action.files.saveAs"
            | "workbench.action.closeActiveEditor"
            | "workbench.action.keepEditor" => tab.is_some(),
            "workbench.action.files.revert" => tab.is_some_and(Tab::dirty),
            "workbench.action.files.saveAll" => self.tabs.iter().any(Tab::dirty),
            "workbench.action.closeAllEditors"
            | "workbench.action.nextEditor"
            | "workbench.action.previousEditor" => !self.tabs.is_empty(),
            "workbench.action.splitEditorRight" | "workbench.action.splitEditorDown" => {
                tab.is_some() && self.group_count() < GROUP_LIMIT
            }
            "workbench.action.moveEditorToNextGroup"
            | "workbench.action.moveEditorToPreviousGroup" => {
                tab.is_some() && (self.group_count() > 1 || self.group_count() < GROUP_LIMIT)
            }
            "workbench.action.focusNextGroup" | "workbench.action.focusPreviousGroup" => {
                self.group_count() > 1
            }
            "workbench.action.focusFirstEditorGroup" => true,
            "workbench.action.focusSecondEditorGroup" => self.group_count() > 1,
            "workbench.action.focusThirdEditorGroup" => self.group_count() > 2,
            "undo" => editor && tab.is_some_and(|t| !t.doc.undo.is_empty()),
            "redo" => editor && tab.is_some_and(|t| !t.doc.redo.is_empty()),
            "editor.action.clipboardPasteAction" => editor || self.focus != Focus::Editor,
            "editor.action.clipboardCutAction"
            | "editor.action.clipboardCopyAction"
            | "actions.find"
            | "editor.action.startFindReplaceAction"
            | "editor.action.commentLine"
            | "editor.action.selectAll"
            | "editor.action.copyLinesUpAction"
            | "editor.action.copyLinesDownAction"
            | "editor.action.moveLinesUpAction"
            | "editor.action.moveLinesDownAction"
            | "editor.action.deleteLines"
            | "editor.action.indentLines"
            | "editor.action.outdentLines"
            | "editor.action.jumpToBracket"
            | "editor.action.insertCursorAbove"
            | "editor.action.insertCursorBelow"
            | "editor.action.addSelectionToNextFindMatch"
            | "editor.action.selectHighlights"
            | "editor.action.indentUsingSpaces"
            | "workbench.action.editor.changeEOL"
            | "workbench.action.gotoLine" => editor,
            "workbench.action.editor.changeLanguageMode" => {
                tab.is_some_and(|t| t.kind != TabKind::Settings)
            }
            "explorer.newFile"
            | "explorer.newFolder"
            | "workbench.action.closeFolder"
            | "workbench.files.action.refreshFilesExplorer"
            | "workbench.files.action.collapseExplorerFolders"
            | "workbench.action.findInFiles"
            | "workbench.action.quickOpen" => folder,
            "renameFile"
            | "deleteFile"
            | "copyFilePath"
            | "revealFileInOS"
            | "openInIntegratedTerminal" => folder && self.selected.is_some(),
            // Go to Definition needs a word under the caret to look for.
            "editor.action.revealDefinition" => {
                folder && editor && tab.is_some_and(|t| !t.doc.word_at_cursor().is_empty())
            }
            "workbench.action.terminal.kill" | "workbench.action.terminal.clear" => {
                !self.terminals.is_empty()
            }
            "workbench.action.terminal.new" => self.terminals.len() < TERMINAL_LIMIT,
            "workbench.action.terminal.runActiveFile" | "workbench.action.debug.run" => runnable,
            // F5 starts a program under the debugger, carries the one that is stopped
            // on, or runs a file no debugger takes.
            "workbench.action.debug.start" => match &self.debug.session {
                Some(s) if !s.ended => !self.debug.busy,
                _ => self.debug_config().is_ok() || runnable,
            },
            "workbench.action.debug.continue"
            | "workbench.action.debug.stepOver"
            | "workbench.action.debug.stepInto"
            | "workbench.action.debug.stepOut" => {
                self.debug.session.as_ref().is_some_and(|s| !s.ended) && !self.debug.busy
            }
            "workbench.action.debug.pause" => {
                self.debug.session.as_ref().is_some_and(|s| !s.ended) && self.debug.busy
            }
            "workbench.action.debug.stop" | "workbench.action.debug.restart" => {
                self.debug.session.is_some()
            }
            "editor.debug.action.toggleBreakpoint"
            | "editor.debug.action.conditionalBreakpoint" => editor,
            "workbench.debug.viewlet.action.removeAllBreakpoints" => {
                !self.debug.breakpoints.is_empty()
            }
            "workbench.debug.viewlet.action.removeAllWatchExpressions" => {
                !self.debug.watches.is_empty()
            }
            "workbench.debug.viewlet.action.addWatchExpression" => {
                self.debug.watches.len() < debug::WATCH_LIMIT
            }
            "debug.addConfiguration" => folder && self.debug_config().is_ok(),
            "python.execInTerminal" => {
                tab.is_some_and(|t| t.kind == TabKind::File && t.lang == Language::Python)
            }
            "git.init" => folder && self.scm.repo == Some(false),
            "git.refresh" => folder,
            "git.stageAll" | "git.cleanAll" => repo && !self.scm.changes.is_empty(),
            "git.unstageAll" => repo && !self.scm.staged.is_empty(),
            "git.commit" => repo && (!self.scm.staged.is_empty() || !self.scm.changes.is_empty()),
            "git.checkout" => repo,
            _ => true,
        };
        if ok {
            return Ok(());
        }
        Err(match id {
            "undo" => "Nothing to undo",
            "redo" => "Nothing to redo",
            "workbench.action.files.revert" | "workbench.action.files.saveAll" => {
                "No unsaved changes"
            }
            "renameFile"
            | "deleteFile"
            | "copyFilePath"
            | "revealFileInOS"
            | "openInIntegratedTerminal" => "Select a file in the Explorer first",
            "editor.action.revealDefinition" => "Put the caret on a name first",
            "git.init" => "The folder already has a repository, or no folder is open",
            "git.stageAll" | "git.commit" | "git.cleanAll" => "There are no changes",
            "git.unstageAll" => "Nothing is staged",
            "git.checkout" | "git.refresh" => "No repository is open",
            "workbench.action.terminal.new" => "The terminal limit is reached",
            "workbench.action.splitEditorRight" | "workbench.action.splitEditorDown" => {
                "Open an editor first, or close a group"
            }
            "workbench.action.focusNextGroup"
            | "workbench.action.focusPreviousGroup"
            | "workbench.action.focusSecondEditorGroup"
            | "workbench.action.focusThirdEditorGroup" => "The editor is not split that far",
            "workbench.action.terminal.kill" | "workbench.action.terminal.clear" => {
                "No terminal is open"
            }
            "workbench.action.terminal.runActiveFile"
            | "workbench.action.debug.run"
            | "python.execInTerminal" => "Open a Python, JavaScript or shell file to run it",
            "workbench.action.debug.start" => "Open a Python, JavaScript or shell file to run it",
            "debug.addConfiguration" => "Open a Python or JavaScript file to debug it",
            "workbench.action.debug.continue"
            | "workbench.action.debug.stepOver"
            | "workbench.action.debug.stepInto"
            | "workbench.action.debug.stepOut" => {
                if self.debug.busy {
                    "The program is running"
                } else {
                    "Nothing is stopped in the debugger"
                }
            }
            "workbench.action.debug.pause" => "The program is not running",
            "workbench.action.debug.stop" | "workbench.action.debug.restart" => {
                "Nothing is being debugged"
            }
            "editor.debug.action.toggleBreakpoint"
            | "editor.debug.action.conditionalBreakpoint" => "Open a file to set a breakpoint in",
            "workbench.debug.viewlet.action.removeAllBreakpoints" => "There are no breakpoints",
            "workbench.debug.viewlet.action.removeAllWatchExpressions" => "There are no watches",
            "workbench.debug.viewlet.action.addWatchExpression" => "The watch list is full",
            _ if !folder => "Open a folder first",
            _ => "Open a text editor first",
        })
    }
    /// Menu commands that can run now, for a menu bar the platform draws.
    pub fn enabled_menu_commands(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = commands::MENUS
            .iter()
            .flat_map(|(_, _, items)| items.iter().copied())
            .filter(|id| *id != "-" && self.enabled(id).is_ok())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
    pub fn run_command(&mut self, window: u64, id: &str) -> Result<Vec<AppEffect>, String> {
        let out = self.command_in(window, id);
        self.sync_siblings();
        out
    }
    fn command_in(&mut self, window: u64, id: &str) -> Result<Vec<AppEffect>, String> {
        self.enabled(id).map_err(str::to_owned)?;
        // Choosing an entry closes the menu it was chosen from, as VS Code does.
        self.menu = None;
        self.context = None;
        let tab_size = self.settings.tab_size;
        let lang = self.active_tab().map_or(Language::PlainText, |t| t.lang);
        let edit = |app: &mut Self, f: &dyn Fn(&mut Doc)| -> Result<Vec<AppEffect>, String> {
            let tab = app.editor_mut()?;
            f(&mut tab.doc);
            tab.preview = false;
            app.focus = Focus::Editor;
            app.follow_caret();
            Ok(vec![])
        };
        match id {
            "workbench.action.showCommands" => self.open_quick(QuickMode::Open, ">"),
            "workbench.action.quickOpen" => self.open_quick(QuickMode::Open, ""),
            "workbench.action.gotoLine" => self.open_quick(QuickMode::Open, ":"),
            "workbench.action.files.newUntitledFile" => {
                self.untitled += 1;
                let name = format!("Untitled-{}", self.untitled);
                let at = self
                    .active
                    .map_or(self.tabs.len(), |a| a + 1)
                    .min(self.tabs.len());
                let id = self.new_tab_id();
                self.tabs.insert(
                    at,
                    Tab {
                        id,
                        group: self.focus_group,
                        path: name,
                        kind: TabKind::Untitled,
                        loaded: true,
                        lang: Language::PlainText,
                        ..Tab::default()
                    },
                );
                self.set_active(Some(at));
                self.focus = Focus::Editor;
                Ok(vec![])
            }
            "explorer.newFile" | "explorer.newFolder" => {
                let parent_dir = match &self.selected {
                    Some(s) if self.is_dir(s) => s.clone(),
                    Some(s) => parent(s).to_owned(),
                    None => String::new(),
                };
                if !parent_dir.is_empty() {
                    self.expanded.insert(parent_dir.clone());
                }
                self.view = View::Explorer;
                self.sidebar = true;
                self.inline = Some(Inline {
                    kind: if id == "explorer.newFile" {
                        InlineKind::NewFile
                    } else {
                        InlineKind::NewFolder
                    },
                    parent: parent_dir,
                    from: None,
                    value: String::new(),
                });
                self.focus = Focus::Inline;
                Ok(vec![])
            }
            "renameFile" => {
                let from = self.selected.clone().ok_or("nothing is selected")?;
                self.inline = Some(Inline {
                    kind: InlineKind::Rename,
                    parent: parent(&from).to_owned(),
                    value: basename(&from).to_owned(),
                    from: Some(from),
                });
                self.focus = Focus::Inline;
                Ok(vec![])
            }
            "deleteFile" => {
                let rel = self.selected.clone().ok_or("nothing is selected")?;
                let what = if self.is_dir(&rel) { "folder" } else { "file" };
                self.dialog = Some(Dialog {
                    message: format!("Are you sure you want to delete '{}'?", basename(&rel)),
                    detail: format!("You can restore this {what} from the Trash."),
                    buttons: vec![
                        ("Move to Trash".into(), format!("trash:{rel}")),
                        ("Cancel".into(), "cancel".into()),
                    ],
                });
                Ok(vec![])
            }
            "workbench.action.files.openFolder" => {
                let start = self
                    .workspace()
                    .map_or(self.home.clone(), |f| parent(f).to_owned());
                let start = format!("{}/", start.trim_end_matches('/'));
                self.open_quick(QuickMode::OpenFolder, &start)
            }
            "workbench.action.closeFolder" => {
                if self.tabs.iter().any(Tab::dirty) {
                    return Err("save or close the editors with unsaved changes first".into());
                }
                self.reset(None);
                Ok(vec![])
            }
            "workbench.action.files.save" => {
                let index = self.active.ok_or("no editor is open")?;
                self.save_tab(window, index)
            }
            "workbench.action.files.saveAs" => self.open_quick(
                QuickMode::SaveAs,
                &self
                    .active_tab()
                    .filter(|t| t.kind == TabKind::File)
                    .map_or_else(|| self.default_save_path(), |t| t.path.clone()),
            ),
            "workbench.action.files.saveAll" => {
                let mut effects = vec![];
                for i in 0..self.tabs.len() {
                    if self.tabs[i].dirty() && self.tabs[i].kind == TabKind::File {
                        effects.extend(self.save_tab(window, i)?);
                    }
                }
                Ok(effects)
            }
            "workbench.action.files.revert" => {
                let tab = self.active_mut().ok_or("no editor is open")?;
                let saved = tab.saved.clone();
                tab.doc.replace_all_text(&saved);
                Ok(vec![])
            }
            "workbench.action.closeActiveEditor" => {
                let index = self.active.ok_or("no editor is open")?;
                self.close_tab(index, false)?;
                Ok(vec![])
            }
            "workbench.action.closeAllEditors" => {
                if let Some(i) = self.tabs.iter().position(Tab::dirty) {
                    self.set_active(Some(i));
                    self.close_tab(i, false)?;
                    return Ok(vec![]);
                }
                self.tabs.clear();
                self.groups = vec![Group::default()];
                self.focus_group = 0;
                self.layout = Slot::default();
                self.active = None;
                self.find = None;
                Ok(vec![])
            }
            "workbench.action.keepEditor" => {
                self.active_mut().ok_or("no editor is open")?.preview = false;
                Ok(vec![])
            }
            "workbench.action.nextEditor" | "workbench.action.previousEditor" => {
                // Within the focused group, as VS Code steps through a group's tabs.
                let mine = self.group_tabs(self.focus_group);
                if mine.is_empty() {
                    return Err("this group has no editors".into());
                }
                let at = self
                    .active
                    .and_then(|a| mine.iter().position(|i| *i == a))
                    .unwrap_or(0);
                let next = if id.ends_with("nextEditor") {
                    (at + 1) % mine.len()
                } else {
                    (at + mine.len() - 1) % mine.len()
                };
                self.set_active(Some(mine[next]));
                self.focus = Focus::Editor;
                Ok(vec![])
            }
            // Splitting, moving an editor between groups, and focusing one.
            "workbench.action.splitEditorRight" => self.split_group(true),
            "workbench.action.splitEditorDown" => self.split_group(false),
            "workbench.action.moveEditorToNextGroup"
            | "workbench.action.moveEditorToPreviousGroup" => {
                let index = self.active.ok_or("no editor is open")?;
                let next = id.ends_with("NextGroup");
                let count = self.group_count();
                let to = if next {
                    (self.focus_group + 1) % count
                } else {
                    (self.focus_group + count - 1) % count
                };
                if to == self.focus_group {
                    // Moving an editor out of the only group splits first, as it does
                    // in VS Code.
                    self.split_group(true)?;
                    let to = self.focus_group;
                    self.tabs[index].group = to;
                    // The copy the split made of it is the one that was moved.
                    if let Some(extra) = self
                        .tabs
                        .iter()
                        .rposition(|t| t.group == to && t.id != self.tabs[index].id)
                    {
                        self.tabs.remove(extra);
                    }
                    self.groups[to].active = Some(self.tabs[index].id);
                    self.sync_active();
                    return Ok(vec![]);
                }
                let id = self.tabs[index].id;
                self.tabs[index].group = to;
                self.groups[to].active = Some(id);
                let from = self.focus_group;
                if !self.tabs.iter().any(|t| t.group == from) {
                    self.close_group(from);
                } else {
                    let mine = self.group_tabs(from);
                    self.groups[from].active = mine.first().map(|i| self.tabs[*i].id);
                }
                self.focus_group = self
                    .tabs
                    .iter()
                    .position(|t| t.id == id)
                    .map(|i| self.tabs[i].group)
                    .unwrap_or(0);
                self.sync_active();
                Ok(vec![])
            }
            "workbench.action.focusNextGroup" | "workbench.action.focusPreviousGroup" => {
                let count = self.group_count();
                let next = id.ends_with("NextGroup");
                let to = if next {
                    (self.focus_group + 1) % count
                } else {
                    (self.focus_group + count - 1) % count
                };
                self.go_to_group(to);
                Ok(vec![])
            }
            "workbench.action.focusFirstEditorGroup"
            | "workbench.action.focusSecondEditorGroup"
            | "workbench.action.focusThirdEditorGroup" => {
                let want = match id {
                    _ if id.ends_with("FirstEditorGroup") => 0,
                    _ if id.ends_with("SecondEditorGroup") => 1,
                    _ => 2,
                };
                if want >= self.group_count() {
                    return Err("there is no such editor group".into());
                }
                self.go_to_group(want);
                Ok(vec![])
            }
            "undo" => edit(self, &|d| {
                d.undo();
            }),
            "redo" => edit(self, &|d| {
                d.redo();
            }),
            "editor.action.clipboardCopyAction" | "editor.action.clipboardCutAction" => {
                let tab = self.editor_mut()?;
                let (a, b) = tab.doc.selection();
                // With nothing selected, VS Code copies and cuts the whole line.
                let whole_line = a == b;
                let (a, b) = if whole_line {
                    let s = tab.doc.line_start(a);
                    (s, (tab.doc.line_end(a) + 1).min(tab.doc.text.len()))
                } else {
                    (a, b)
                };
                let mut text = tab.doc.text[a..b].to_owned();
                if text.is_empty() {
                    return Ok(vec![]);
                }
                if whole_line && !text.ends_with('\n') {
                    text.push('\n');
                }
                if id.ends_with("CutAction") {
                    tab.doc.set(a, false);
                    tab.doc.set(b, true);
                    tab.doc.delete();
                    tab.preview = false;
                }
                Ok(vec![AppEffect::CopyText { window, text }])
            }
            "editor.action.clipboardPasteAction" => Ok(vec![AppEffect::Paste { window }]),
            "actions.find" | "editor.action.startFindReplaceAction" => {
                let replace = id != "actions.find";
                let seed = self.active_tab().map(|t| {
                    let s = t.doc.selected_text();
                    if !s.is_empty() && !s.contains('\n') {
                        s.to_owned()
                    } else {
                        String::new()
                    }
                });
                let find = self.find.get_or_insert_with(Find::default);
                if let Some(seed) = seed.filter(|s| !s.is_empty()) {
                    find.query = seed;
                }
                find.replace_open = replace || find.replace_open;
                self.focus = if replace && !find.query.is_empty() {
                    Focus::Replace
                } else {
                    Focus::Find
                };
                Ok(vec![])
            }
            "workbench.action.findInFiles" | "workbench.view.search" => {
                let seed = self
                    .active_tab()
                    .map(|t| t.doc.selected_text().to_owned())
                    .filter(|s| !s.is_empty() && !s.contains('\n'));
                self.view = View::Search;
                self.sidebar = true;
                self.focus = Focus::Search;
                if let Some(seed) = seed {
                    self.search.query = seed;
                    return Ok(self.start_search(window));
                }
                Ok(vec![])
            }
            "editor.action.commentLine" => edit(self, &|d| d.toggle_comment(lang)),
            // Multi-cursor: a caret on the line above or below, the next occurrence of
            // what is selected, or every occurrence of it at once.
            "editor.action.insertCursorAbove" | "editor.action.insertCursorBelow" => {
                let up = id.ends_with("Above");
                let tab = self.editor_mut()?;
                let d = &mut tab.doc;
                let (edge, _) = if up {
                    d.all_carets()
                        .into_iter()
                        .min_by_key(|(c, _)| *c)
                        .unwrap_or((d.cursor, d.anchor))
                } else {
                    d.all_carets()
                        .into_iter()
                        .max_by_key(|(c, _)| *c)
                        .unwrap_or((d.cursor, d.anchor))
                };
                let col = d.col_of(edge);
                let line = d.line_of(edge);
                if up && line == 0 {
                    return Err("there is no line above".into());
                }
                if !up && line + 1 >= d.line_count() {
                    return Err("there is no line below".into());
                }
                let (start, _) = d.line_range(if up { line - 1 } else { line + 1 });
                let at = d.at_col(start, col);
                d.add_caret(at, at);
                self.follow_caret();
                Ok(vec![])
            }
            "editor.action.addSelectionToNextFindMatch" => {
                let tab = self.editor_mut()?;
                let d = &mut tab.doc;
                // With nothing selected, the first press selects the word at the caret.
                if !d.has_selection() {
                    let start = d.word_left(d.cursor);
                    let end = d.word_right(start);
                    if start == end {
                        return Err("put the caret in a word first".into());
                    }
                    d.set(start, false);
                    d.set(end, true);
                    self.follow_caret();
                    return Ok(vec![]);
                }
                let needle = d.selected_text().to_owned();
                let carets = d.all_carets();
                let last = carets.iter().map(|(c, _)| *c).max().unwrap_or(d.cursor);
                let taken: Vec<usize> = carets.iter().map(|(c, a)| (*c).min(*a)).collect();
                let mut at = find_all(
                    &d.text,
                    &needle,
                    FindOptions {
                        case: true,
                        ..Default::default()
                    },
                )
                .unwrap_or_default();
                at.retain(|(s, _)| !taken.contains(s));
                let next = at
                    .iter()
                    .find(|(s, _)| *s >= last)
                    .or_else(|| at.first())
                    .copied();
                match next {
                    Some((s, e)) => {
                        d.add_caret(e, s);
                        self.follow_caret();
                        Ok(vec![])
                    }
                    None => Err(format!("no more occurrences of {needle}")),
                }
            }
            "editor.action.selectHighlights" => {
                let tab = self.editor_mut()?;
                let d = &mut tab.doc;
                let needle = if d.has_selection() {
                    d.selected_text().to_owned()
                } else {
                    let start = d.word_left(d.cursor);
                    let end = d.word_right(start);
                    d.text[start..end].to_owned()
                };
                if needle.trim().is_empty() {
                    return Err("select something to find first".into());
                }
                let hits = find_all(
                    &d.text,
                    &needle,
                    FindOptions {
                        case: true,
                        ..Default::default()
                    },
                )
                .unwrap_or_default();
                if hits.is_empty() {
                    return Err(format!("no occurrences of {needle}"));
                }
                d.clear_carets();
                let (first_s, first_e) = hits[0];
                d.set(first_s, false);
                d.set(first_e, true);
                for (s, e) in hits.into_iter().skip(1) {
                    d.add_caret(e, s);
                }
                self.follow_caret();
                Ok(vec![])
            }
            "removeSecondaryCursors" => {
                self.editor_mut()?.doc.clear_carets();
                Ok(vec![])
            }
            "editor.action.selectAll" => edit(self, &|d| d.select_all()),
            "editor.action.copyLinesUpAction" => edit(self, &|d| d.copy_lines(true)),
            "editor.action.copyLinesDownAction" => edit(self, &|d| d.copy_lines(false)),
            "editor.action.moveLinesUpAction" => edit(self, &|d| d.move_lines(true)),
            "editor.action.moveLinesDownAction" => edit(self, &|d| d.move_lines(false)),
            "editor.action.deleteLines" => edit(self, &|d| d.delete_lines()),
            "editor.action.indentLines" => edit(self, &|d| {
                let (a, b) = d.selection();
                let s = d.line_start(a);
                if a == b {
                    d.set(s, false);
                    d.set(d.line_end(s), true);
                }
                d.indent(tab_size.max(1));
                if d.line_of(d.cursor) == d.line_of(d.anchor) && a == b {
                    let c = d.cursor;
                    d.set(c, false);
                }
            }),
            "editor.action.outdentLines" => edit(self, &|d| d.outdent(tab_size)),
            "editor.action.jumpToBracket" => edit(self, &|d| {
                if let Some((open, close)) = d.matching_bracket() {
                    let to = if d.cursor == open || d.cursor == open + 1 {
                        close
                    } else {
                        open
                    };
                    d.set(to, false);
                }
            }),
            "workbench.view.explorer" | "workbench.view.scm" | "workbench.view.debug" => {
                self.view = match id {
                    "workbench.view.explorer" => View::Explorer,
                    "workbench.view.scm" => View::Scm,
                    _ => View::Run,
                };
                self.sidebar = true;
                if self.view == View::Explorer {
                    self.focus = Focus::Explorer;
                }
                if self.view == View::Scm {
                    self.focus = Focus::ScmMessage;
                    if self.workspace().is_some() {
                        return Ok(vec![self.git(window, "status", "git status")]);
                    }
                }
                Ok(vec![])
            }
            "workbench.action.toggleSidebarVisibility" => {
                self.sidebar = !self.sidebar;
                Ok(vec![])
            }
            "workbench.action.togglePanel" => {
                self.panel_open = !self.panel_open;
                if self.panel_open && self.panel == PanelTab::Terminal {
                    return self.ensure_terminal(window);
                }
                Ok(vec![])
            }
            "workbench.actions.view.problems" | "workbench.action.output.toggleOutput" => {
                let tab = if id.contains("problems") {
                    PanelTab::Problems
                } else {
                    PanelTab::Output
                };
                if self.panel_open && self.panel == tab {
                    self.panel_open = false;
                } else {
                    self.panel_open = true;
                    self.panel = tab;
                }
                Ok(vec![])
            }
            "workbench.action.terminal.toggleTerminal" => {
                if self.panel_open
                    && self.panel == PanelTab::Terminal
                    && self.focus == Focus::Terminal
                {
                    self.panel_open = false;
                    self.focus = Focus::Editor;
                    return Ok(vec![]);
                }
                let effects = self.ensure_terminal(window)?;
                self.focus = Focus::Terminal;
                Ok(effects)
            }
            "workbench.action.terminal.new" => self.new_terminal(window),
            "workbench.action.terminal.kill" => {
                self.terminals.remove(self.term);
                self.term = self.term.min(self.terminals.len().saturating_sub(1));
                if self.terminals.is_empty() {
                    self.panel_open = false;
                    self.focus = Focus::Editor;
                }
                Ok(vec![])
            }
            "workbench.action.terminal.clear" => {
                if let Some(t) = self.terminals.get_mut(self.term) {
                    t.transcript.clear();
                    t.scroll = 0;
                }
                Ok(vec![])
            }
            "workbench.action.terminal.runActiveFile"
            | "python.execInTerminal"
            | "workbench.action.debug.run" => self.run_active(window),
            "workbench.action.debug.start" | "workbench.action.debug.continue" => {
                self.debug_start(window)
            }
            "workbench.action.debug.stepOver" => {
                self.debug_resume(window, cw_protocol::debug::Step::Over)
            }
            "workbench.action.debug.stepInto" => {
                self.debug_resume(window, cw_protocol::debug::Step::Into)
            }
            "workbench.action.debug.stepOut" => {
                self.debug_resume(window, cw_protocol::debug::Step::Out)
            }
            "workbench.action.debug.pause" => self.debug_pause(window),
            "workbench.action.debug.stop" => self.debug_stop(window),
            "workbench.action.debug.restart" => self.debug_restart(window),
            "editor.debug.action.toggleBreakpoint" => {
                let (path, line) = self.caret_line()?;
                self.breakpoint_at(window, &path, line)
            }
            "editor.debug.action.conditionalBreakpoint" => {
                let (path, line) = self.caret_line()?;
                let value = self
                    .debug
                    .at(&path, line)
                    .and_then(|b| b.condition.clone())
                    .unwrap_or_default();
                self.view = View::Run;
                self.sidebar = true;
                self.focus = Focus::Debug;
                self.debug.prompt = Some(debug::Prompt {
                    kind: debug::PromptKind::Condition { path, line },
                    value,
                });
                Ok(vec![])
            }
            "workbench.debug.viewlet.action.removeAllBreakpoints" => {
                let paths: Vec<String> = {
                    let mut p: Vec<String> = self
                        .debug
                        .breakpoints
                        .iter()
                        .map(|b| b.path.clone())
                        .collect();
                    p.sort();
                    p.dedup();
                    p
                };
                self.debug.breakpoints.clear();
                let mut effects = vec![];
                for path in paths {
                    effects.extend(self.send_breakpoints(window, &path));
                }
                Ok(effects)
            }
            "workbench.debug.viewlet.action.addWatchExpression" => {
                self.view = View::Run;
                self.sidebar = true;
                self.debug.prompt = Some(debug::Prompt {
                    kind: debug::PromptKind::Watch,
                    value: String::new(),
                });
                self.focus = Focus::Debug;
                Ok(vec![])
            }
            "workbench.debug.viewlet.action.removeAllWatchExpressions" => {
                self.debug.watches.clear();
                Ok(vec![])
            }
            "workbench.debug.action.toggleRepl" => {
                // VS Code's toggle hides the Debug Console only when it already has the
                // focus; a console that is merely on screen is focused instead.
                let focused =
                    self.panel_open && self.panel == PanelTab::Debug && self.focus == Focus::Debug;
                self.panel_open = !focused;
                self.panel = PanelTab::Debug;
                if self.panel_open {
                    self.focus = Focus::Debug;
                }
                Ok(vec![])
            }
            "debug.addConfiguration" => self.create_launch_json(window),
            "editor.action.toggleWordWrap" => {
                self.settings.word_wrap = !self.settings.word_wrap;
                Ok(self.save_settings(window))
            }
            "editor.action.toggleRenderWhitespace" => {
                self.settings.render_whitespace = !self.settings.render_whitespace;
                Ok(self.save_settings(window))
            }
            "workbench.action.selectTheme" => {
                let effects = self.open_quick(QuickMode::Theme, "")?;
                if let Some(q) = &mut self.quick {
                    q.selected = usize::from(!self.settings.dark);
                }
                Ok(effects)
            }
            "workbench.action.openSettings" => {
                if let Some(i) = self.tabs.iter().position(|t| t.kind == TabKind::Settings) {
                    self.set_active(Some(i));
                } else {
                    let at = self
                        .active
                        .map_or(self.tabs.len(), |a| a + 1)
                        .min(self.tabs.len());
                    let id = self.new_tab_id();
                    self.tabs.insert(
                        at,
                        Tab {
                            id,
                            group: self.focus_group,
                            path: "Settings".into(),
                            kind: TabKind::Settings,
                            loaded: true,
                            ..Tab::default()
                        },
                    );
                    self.set_active(Some(at));
                }
                Ok(vec![])
            }
            "workbench.action.openSettingsJson" => {
                let path = self.settings_path();
                let content = self.settings_json();
                // Make sure the file exists before opening it, as VS Code writes an empty one.
                let mut effects = vec![AppEffect::CreateDirectory {
                    window,
                    path: parent(&path).to_owned(),
                }];
                if !self.tabs.iter().any(|t| t.path == path) {
                    self.writes.push((path.clone(), content.clone()));
                    effects.push(AppEffect::WriteFile {
                        window,
                        path: path.clone(),
                        content,
                    });
                }
                effects.extend(self.open_file(window, &path, true, None));
                Ok(effects)
            }
            "workbench.action.editor.changeLanguageMode" => {
                self.open_quick(QuickMode::Language, "")
            }
            "editor.action.indentUsingSpaces" => self.open_quick(QuickMode::TabSize, ""),
            "workbench.action.editor.changeEOL" => self.open_quick(QuickMode::Eol, ""),
            "workbench.files.action.refreshFilesExplorer" => Ok(self.relist(window)),
            "workbench.files.action.collapseExplorerFolders" => {
                self.expanded.clear();
                Ok(vec![])
            }
            "git.init" => self.git_command(window, "init", "git init".into()),
            "git.refresh" => self.git_command(window, "status", "git status".into()),
            "git.stageAll" => self.git_command(window, "add", "git add -A".into()),
            "git.unstageAll" => self.git_command(window, "reset", "git reset".into()),
            "git.cleanAll" => {
                let count = self.scm.changes.len();
                self.dialog = Some(Dialog {
                    message: format!(
                        "Are you sure you want to discard ALL changes in {count} file{}?",
                        if count == 1 { "" } else { "s" }
                    ),
                    detail: "This is IRREVERSIBLE! Your current working set will be FOREVER LOST if you proceed.".into(),
                    buttons: vec![
                        ("Discard All Changes".into(), "discard-all".into()),
                        ("Cancel".into(), "cancel".into()),
                    ],
                });
                Ok(vec![])
            }
            "git.commit" => {
                let message = self.scm.message.trim().to_owned();
                if message.is_empty() {
                    self.view = View::Scm;
                    self.sidebar = true;
                    self.focus = Focus::ScmMessage;
                    return Err("type a commit message first".into());
                }
                if self.scm.staged.is_empty() {
                    self.dialog = Some(Dialog {
                        message: "There are no staged changes to commit.".into(),
                        detail:
                            "Would you like to stage all your changes and commit them directly?"
                                .into(),
                        buttons: vec![
                            ("Yes".into(), "commit-all".into()),
                            ("Cancel".into(), "cancel".into()),
                        ],
                    });
                    return Ok(vec![]);
                }
                let command = format!("git commit -m {}", quote(&message, self.powershell()));
                self.git_command(window, "commit", command)
            }
            "git.checkout" => {
                let mut effects = self.git_command(window, "branches", "git branch".into())?;
                effects.extend(self.open_quick(QuickMode::Branch, "")?);
                Ok(effects)
            }
            // The Explorer's context menu, beyond what the Explorer already does.
            "copyFilePath" => {
                let rel = self.selected.clone().ok_or("nothing is selected")?;
                let path = self.abs(&rel);
                self.notice = Some(format!("Copied {path}"));
                Ok(vec![AppEffect::CopyText { window, text: path }])
            }
            "revealFileInOS" => {
                let rel = self.selected.clone().ok_or("nothing is selected")?;
                let folder = if self.is_dir(&rel) {
                    self.abs(&rel)
                } else {
                    self.abs(parent(&rel))
                };
                Ok(vec![AppEffect::Launch {
                    window,
                    kind: "files".into(),
                    argument: folder,
                }])
            }
            "openInIntegratedTerminal" => {
                let rel = self.selected.clone().ok_or("nothing is selected")?;
                let folder = if self.is_dir(&rel) {
                    self.abs(&rel)
                } else {
                    self.abs(parent(&rel))
                };
                let mut effects = self.new_terminal(window)?;
                // The new terminal opens in the folder that was right-clicked.
                if let Some(term) = self.terminals.get_mut(self.term) {
                    term.cwd = folder.clone();
                }
                for effect in &mut effects {
                    if let AppEffect::ShellRun { cwd, .. } = effect {
                        *cwd = folder.clone();
                    }
                }
                Ok(effects)
            }
            // Go to Definition: the workspace is searched for where the name under the
            // caret is defined, in the shapes its language defines things in.
            "editor.action.revealDefinition" => {
                let word = self
                    .active_tab()
                    .map(|t| t.doc.word_at_cursor())
                    .unwrap_or_default();
                if word.is_empty() {
                    return Err("put the caret on a name first".into());
                }
                let paths: Vec<String> = self
                    .files()
                    .into_iter()
                    .take(SEARCH_FILES)
                    .map(|f| self.abs(f))
                    .collect();
                if paths.is_empty() {
                    return Err("open a folder first".into());
                }
                Ok(vec![AppEffect::ReadFiles {
                    window,
                    tag: format!("definition:{word}"),
                    paths,
                }])
            }
            "workbench.action.showAboutDialog" => {
                self.dialog = Some(Dialog {
                    message: "Visual Studio Code".into(),
                    detail: format!(
                        "A Computerworld native application.\nEditing, search, source control and the integrated terminal all act on this machine's own files and shell.\nPlatform: {}",
                        match self.platform {
                            Platform::Mac => "macOS",
                            Platform::Windows => "Windows",
                            Platform::Linux => "Linux",
                        }
                    ),
                    buttons: vec![("OK".into(), "cancel".into())],
                });
                Ok(vec![])
            }
            other => Err(format!("unknown command {other}")),
        }
    }
    fn dialog_action(&mut self, window: u64, action: &str) -> Result<Vec<AppEffect>, String> {
        self.dialog = None;
        let (verb, arg) = action.split_once(':').unwrap_or((action, ""));
        match verb {
            "cancel" => Ok(vec![]),
            "save-close" => {
                let i: usize = arg.parse().map_err(|_| "invalid editor")?;
                let effects = self.save_tab(window, i)?;
                if self.tabs.get(i).is_some_and(|t| t.kind == TabKind::File) {
                    self.close_tab(i, true)?;
                }
                Ok(effects)
            }
            "discard-close" => {
                let i: usize = arg.parse().map_err(|_| "invalid editor")?;
                self.close_tab(i, true)?;
                Ok(vec![])
            }
            "trash" => {
                let abs = self.abs(arg);
                // Editors on what was deleted close with it.
                let prefix = format!("{abs}/");
                while let Some(i) = self.tabs.iter().position(|t| {
                    t.kind == TabKind::File && (t.path == abs || t.path.starts_with(&prefix))
                }) {
                    self.close_tab(i, true)?;
                }
                self.selected = None;
                let mut effects = vec![AppEffect::TrashPath {
                    window,
                    path: abs,
                    trash: self.trash.clone(),
                }];
                effects.extend(self.relist(window));
                if self.scm.repo == Some(true) {
                    effects.push(self.git(window, "status", "git status"));
                }
                Ok(effects)
            }
            // Discard: the file comes back from the index, or, untracked, goes away.
            "discard" => {
                let ps = self.powershell();
                self.git_command(window, "restore", format!("git restore {}", quote(arg, ps)))
            }
            "discard-new" => {
                let abs = self.abs(arg);
                let prefix = format!("{abs}/");
                while let Some(i) = self.tabs.iter().position(|t| {
                    t.kind == TabKind::File && (t.path == abs || t.path.starts_with(&prefix))
                }) {
                    self.close_tab(i, true)?;
                }
                let mut effects = vec![AppEffect::TrashPath {
                    window,
                    path: abs,
                    trash: self.trash.clone(),
                }];
                effects.extend(self.relist(window));
                effects.push(self.git(window, "status", "git status"));
                Ok(effects)
            }
            "discard-all" => {
                // Tracked files come back from the index; untracked ones are removed.
                let ps = self.powershell();
                let mut effects = vec![];
                let untracked: Vec<String> = self
                    .scm
                    .changes
                    .iter()
                    .filter(|(state, _)| *state == 'U')
                    .map(|(_, path)| path.clone())
                    .collect();
                if self.scm.changes.iter().any(|(state, _)| *state != 'U') {
                    effects.extend(self.git_command(
                        window,
                        "restore",
                        "git restore .".to_string(),
                    )?);
                }
                for path in untracked {
                    effects.push(AppEffect::TrashPath {
                        window,
                        path: self.abs(&path),
                        trash: self.trash.clone(),
                    });
                }
                let _ = ps;
                effects.extend(self.relist(window));
                effects.push(self.git(window, "status", "git status"));
                Ok(effects)
            }
            "commit-all" => {
                let message = self.scm.message.trim().to_owned();
                let ps = self.powershell();
                Ok(vec![
                    self.git(window, "add", "git add -A"),
                    self.git(
                        window,
                        "commit",
                        &format!("git commit -m {}", quote(&message, ps)),
                    ),
                ])
            }
            _ => Err(format!("unknown dialog action {verb}")),
        }
    }
    fn commit_inline(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let inline = self.inline.take().ok_or("nothing is being named")?;
        self.focus = Focus::Explorer;
        let name = inline.value.trim().trim_matches('/').to_owned();
        if name.is_empty() {
            return Ok(vec![]);
        }
        if name
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
            || name.contains('\\')
        {
            return Err(format!(
                "The name {name} is not valid as a file or folder name."
            ));
        }
        let rel = if inline.parent.is_empty() {
            name.clone()
        } else {
            format!("{}/{name}", inline.parent)
        };
        if self.exists(&rel) && inline.from.as_deref() != Some(rel.as_str()) {
            return Err(format!(
                "A file or folder {name} already exists at this location."
            ));
        }
        let abs = self.abs(&rel);
        let mut effects = vec![];
        match inline.kind {
            InlineKind::NewFolder => effects.push(AppEffect::CreateDirectory {
                window,
                path: abs.clone(),
            }),
            InlineKind::NewFile => {
                // `a/b.txt` makes the folder too, as the Explorer allows.
                if name.contains('/') {
                    effects.push(AppEffect::CreateDirectory {
                        window,
                        path: self.abs(parent(&rel)),
                    });
                }
                effects.push(AppEffect::CreateFile {
                    window,
                    path: abs.clone(),
                });
            }
            InlineKind::Rename => {
                let from = inline.from.clone().ok_or("nothing is being renamed")?;
                if from == rel {
                    return Ok(vec![]);
                }
                let from_abs = self.abs(&from);
                effects.push(AppEffect::MovePath {
                    window,
                    from: from_abs.clone(),
                    to: abs.clone(),
                });
                // Open editors follow the file to its new name.
                let prefix = format!("{from_abs}/");
                for tab in &mut self.tabs {
                    if tab.kind != TabKind::File {
                        continue;
                    }
                    if tab.path == from_abs {
                        tab.path = abs.clone();
                        tab.lang = Language::from_path(&abs);
                    } else if let Some(rest) = tab.path.strip_prefix(&prefix) {
                        tab.path = format!("{abs}/{rest}");
                    }
                }
            }
        }
        self.selected = Some(rel.clone());
        if !parent(&rel).is_empty() {
            let mut dir = parent(&rel).to_owned();
            while !dir.is_empty() {
                self.expanded.insert(dir.clone());
                dir = parent(&dir).to_owned();
            }
        }
        effects.extend(self.relist(window));
        if inline.kind == InlineKind::NewFile {
            effects.extend(self.open_file(window, &abs, true, None));
        }
        if self.scm.repo == Some(true) {
            effects.push(self.git(window, "status", "git status"));
        }
        Ok(effects)
    }

    /// Keep the caret on screen after it moved, as far as the model knows the view.
    fn follow_caret(&mut self) {
        let page = self.page.max(1);
        let wrap = if self.settings.word_wrap {
            self.wrap_cols
        } else {
            0
        };
        if let Some(tab) = self.active.and_then(|i| self.tabs.get_mut(i)) {
            tab.doc.sanitize();
            let (row, _) = crate::editor_caret_cell(&tab.doc.text, tab.doc.cursor, wrap);
            tab.follow = true;
            if row < tab.scroll {
                tab.scroll = row;
            } else if row >= tab.scroll + page {
                tab.scroll = row + 1 - page;
            }
        }
    }

    // ----- input -----------------------------------------------------------------------

    pub fn text(&mut self, text: &str) -> Result<(), String> {
        self.text_effects(0, text).map(|_| ())
    }
    /// Typed text goes to whatever has focus.
    pub fn text_effects(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
        let out = self.text_in(window, text);
        self.sync_siblings();
        out
    }
    fn text_in(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
        self.menu = None;
        // `Ctrl+K` then a typed `f` is the chord, not an `f` in the document.
        if self.chord && text.chars().count() == 1 {
            return self.key(window, text, 0);
        }
        self.chord = false;
        match self.focus {
            Focus::Quick => {
                let quick = self.quick.as_mut().ok_or("no quick input is open")?;
                bounded(&mut quick.value, text, FIELD_LIMIT)?;
                quick.selected = 0;
                if quick.mode == QuickMode::OpenFolder {
                    let value = quick.value.clone();
                    return Ok(self.browse_to(&value));
                }
                Ok(vec![])
            }
            Focus::Find | Focus::Replace => {
                let find = self.find.as_mut().ok_or("the find widget is closed")?;
                let field = if self.focus == Focus::Find {
                    &mut find.query
                } else {
                    &mut find.replace
                };
                bounded(field, text, FIELD_LIMIT)?;
                if self.focus == Focus::Find {
                    self.find_step(true, true);
                }
                Ok(vec![])
            }
            Focus::Search | Focus::SearchReplace => {
                let field = if self.focus == Focus::Search {
                    &mut self.search.query
                } else {
                    &mut self.search.replace
                };
                bounded(field, text, FIELD_LIMIT)?;
                if self.focus == Focus::Search {
                    return Ok(self.start_search(window));
                }
                Ok(vec![])
            }
            Focus::ScmMessage => {
                bounded(&mut self.scm.message, text, 4096)?;
                Ok(vec![])
            }
            Focus::Inline => {
                let inline = self.inline.as_mut().ok_or("nothing is being named")?;
                bounded(&mut inline.value, text, FIELD_LIMIT).map(|_| vec![])
            }
            Focus::Debug => {
                // A box that is open takes the typing; otherwise it is the console's
                // own input line.
                match &mut self.debug.prompt {
                    Some(prompt) => bounded(&mut prompt.value, text, FIELD_LIMIT)?,
                    None => bounded(&mut self.debug.input, text, FIELD_LIMIT)?,
                }
                Ok(vec![])
            }
            Focus::Terminal => {
                let term = self
                    .terminals
                    .get_mut(self.term)
                    .ok_or("no terminal is open")?;
                if term.input.len() + text.len() > 4096 {
                    return Err("the command line is full".into());
                }
                let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                term.input
                    .insert_str(term.cursor.min(term.input.len()), &clean);
                term.cursor += clean.len();
                term.recall = None;
                Ok(vec![])
            }
            Focus::Editor | Focus::Explorer => {
                self.focus = Focus::Editor;
                let tab_size = self.settings.tab_size;
                let tab = self.editor_mut()?;
                if tab.doc.text.len() + text.len() > TEXT_LIMIT {
                    return Err("the document is too large to type into".into());
                }
                let lang = tab.lang;
                // Typed at every caret, as VS Code types at every cursor.
                tab.doc.at_each(|d| {
                    let mut chars = text.chars();
                    match (chars.next(), chars.next()) {
                        (Some('\n'), None) => d.newline(lang, tab_size),
                        (Some('\t'), None) => d.indent(tab_size),
                        // One character is a keystroke: brackets close and are typed over.
                        (Some(c), None) => d.type_char(c, lang),
                        // A run of text arrives as it was written, the way a paste does,
                        // so its own indentation is not indented again.
                        _ => d.insert(&text.replace("\r\n", "\n")),
                    }
                });
                tab.preview = false;
                self.follow_caret();
                Ok(vec![])
            }
        }
    }
    pub fn paste(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
        let out = self.paste_in(window, text);
        self.sync_siblings();
        out
    }
    fn paste_in(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
        match self.focus {
            Focus::Editor | Focus::Explorer => {
                self.focus = Focus::Editor;
                let tab = self.editor_mut()?;
                let pasted = text.replace("\r\n", "\n");
                tab.doc.at_each(|d| d.insert(&pasted));
                tab.preview = false;
                self.follow_caret();
                Ok(vec![])
            }
            _ => self.text_effects(window, &text.replace(['\n', '\r'], " ")),
        }
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        let out = self.key_in(window, key, clock_us);
        self.sync_siblings();
        out
    }
    fn key_in(&mut self, window: u64, key: &str, _clock_us: u64) -> Result<Vec<AppEffect>, String> {
        let key = commands::normalize(key);
        // A printable key with no modifier is typing, unless it finishes a chord.
        if !self.chord {
            if key.chars().count() == 1 {
                return self.text_effects(window, &key);
            }
            if let Some(c) = key
                .strip_prefix("shift+")
                .filter(|k| k.chars().count() == 1)
            {
                return self.text_effects(window, &c.to_uppercase());
            }
        }
        if self.chord {
            self.chord = false;
            let id = match key.as_str() {
                "ctrl+o" | "o" => "workbench.action.files.openFolder",
                "ctrl+t" => "workbench.action.selectTheme",
                "m" | "ctrl+m" => "workbench.action.editor.changeLanguageMode",
                "f" | "ctrl+f" => "workbench.action.closeFolder",
                "s" | "ctrl+s" => "workbench.action.files.saveAll",
                "enter" | "ctrl+enter" => "workbench.action.keepEditor",
                _ => return Err(format!("({key}) is not a command")),
            };
            return self.run_command(window, id);
        }
        if key == "escape" {
            return Ok(self.escape());
        }
        if self.dialog.is_some() {
            if key == "enter" {
                let action = self
                    .dialog
                    .as_ref()
                    .and_then(|d| d.buttons.first())
                    .map(|b| b.1.clone())
                    .unwrap_or_default();
                return self.dialog_action(window, &action);
            }
            return Err("a dialog is open".into());
        }
        // Keys a focused field consumes before any global binding sees them.
        match self.focus {
            Focus::Quick => {
                if let Some(effects) = self.quick_key(window, &key)? {
                    return Ok(effects);
                }
            }
            Focus::Find | Focus::Replace => {
                if let Some(effects) = self.find_key(window, &key)? {
                    return Ok(effects);
                }
            }
            Focus::Search
            | Focus::SearchReplace
            | Focus::ScmMessage
            | Focus::Inline
            | Focus::Debug => {
                if let Some(effects) = self.field_key(window, &key)? {
                    return Ok(effects);
                }
            }
            Focus::Terminal => {
                if let Some(effects) = self.terminal_key(window, &key)? {
                    return Ok(effects);
                }
            }
            Focus::Explorer => {
                if let Some(effects) = self.explorer_key(window, &key)? {
                    return Ok(effects);
                }
            }
            Focus::Editor => {}
        }
        if key == "ctrl+k" {
            self.chord = true;
            return Ok(vec![]);
        }
        if let Some(id) = self.global_binding(&key) {
            return self.run_command(window, id);
        }
        if self.focus == Focus::Editor || self.focus == Focus::Explorer {
            return self.editor_key(&key);
        }
        Err(format!("unsupported key {key}"))
    }
    fn global_binding(&self, key: &str) -> Option<&'static str> {
        // Bindings that belong to one surface: the editor's own, and the Explorer's and
        // commit box's, which their key handlers take before this table is consulted.
        let editor_only = [
            "undo",
            "redo",
            "editor.action.clipboardCutAction",
            "editor.action.clipboardCopyAction",
            "editor.action.selectAll",
        ];
        let never = ["deleteFile", "renameFile", "git.commit"];
        match key {
            "f1" => return Some("workbench.action.showCommands"),
            "ctrl+e" => return Some("workbench.action.quickOpen"),
            "ctrl+shift+z" => return Some("redo"),
            "ctrl+f4" => return Some("workbench.action.closeActiveEditor"),
            "ctrl+tab" => return Some("workbench.action.nextEditor"),
            "ctrl+shift+tab" => return Some("workbench.action.previousEditor"),
            "ctrl+shift+f" => return Some("workbench.action.findInFiles"),
            "f3" | "shift+f3" => return None,
            _ => {}
        }
        commands::COMMANDS
            .iter()
            .filter(|c| !never.contains(&c.id))
            .filter(|c| !editor_only.contains(&c.id) || self.focus == Focus::Editor)
            .find(|c| commands::binding(c.keys).as_deref() == Some(key) && !c.keys.contains(' '))
            .map(|c| c.id)
    }
    fn escape(&mut self) -> Vec<AppEffect> {
        if self.dialog.take().is_some() || self.menu.take().is_some() {
            return vec![];
        }
        if self.quick.take().is_some() {
            self.focus = Focus::Editor;
            return vec![];
        }
        if self.inline.take().is_some() {
            self.focus = Focus::Explorer;
            return vec![];
        }
        if matches!(self.focus, Focus::Find | Focus::Replace)
            || (self.focus == Focus::Editor && self.find.is_some())
        {
            self.find = None;
            self.focus = Focus::Editor;
            return vec![];
        }
        if self.notice.take().is_some() {
            return vec![];
        }
        if let Some(tab) = self.active_mut() {
            // Escape drops the extra cursors first, as it does in VS Code.
            if !tab.doc.carets.is_empty() {
                tab.doc.clear_carets();
                return vec![];
            }
            let c = tab.doc.cursor;
            tab.doc.set(c, false);
        }
        self.focus = Focus::Editor;
        vec![]
    }
    fn quick_key(&mut self, window: u64, key: &str) -> Result<Option<Vec<AppEffect>>, String> {
        let count = self.quick_items().len();
        let quick = self.quick.as_mut().ok_or("no quick input is open")?;
        match key {
            "arrowdown" => quick.selected = (quick.selected + 1).min(count.saturating_sub(1)),
            "arrowup" => quick.selected = quick.selected.saturating_sub(1),
            "backspace" => {
                pop_char(&mut quick.value);
                quick.selected = 0;
                if quick.mode == QuickMode::OpenFolder {
                    let value = quick.value.clone();
                    return Ok(Some(self.browse_to(&value)));
                }
            }
            "enter" => return self.accept_quick(window, None).map(Some),
            "tab" if quick.mode == QuickMode::OpenFolder => {
                let items = self.quick_items();
                if let Some(item) = items.iter().find(|i| i.label != "..") {
                    let action = item.action.clone();
                    return self.quick_action(window, &action).map(Some);
                }
            }
            _ => return Ok(None),
        }
        Ok(Some(vec![]))
    }
    fn find_matches(&self) -> Vec<(usize, usize)> {
        match (&self.find, self.active_tab()) {
            (Some(find), Some(tab)) => {
                find_all(&tab.doc.text, &find.query, find.opts).unwrap_or_default()
            }
            _ => vec![],
        }
    }
    /// Move to the next (or previous) match; `stay` keeps a match already under the caret.
    fn find_step(&mut self, forward: bool, stay: bool) {
        let matches = self.find_matches();
        let Some(tab) = self.active.and_then(|i| self.tabs.get_mut(i)) else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        let (a, _) = tab.doc.selection();
        let next = if forward {
            matches
                .iter()
                .find(|(s, _)| if stay { *s >= a } else { *s > a })
                .or_else(|| matches.first())
        } else {
            matches
                .iter()
                .rev()
                .find(|(s, _)| *s < a)
                .or_else(|| matches.last())
        };
        if let Some(&(s, e)) = next {
            tab.doc.set(s, false);
            tab.doc.set(e, true);
        }
        self.follow_caret();
    }
    fn replace_current(&mut self) -> Result<(), String> {
        let matches = self.find_matches();
        let replace = self
            .find
            .as_ref()
            .map(|f| f.replace.clone())
            .unwrap_or_default();
        let tab = self.editor_mut()?;
        let sel = tab.doc.selection();
        if matches.contains(&sel) {
            tab.doc.replace_range(sel, &replace);
        }
        self.find_step(true, true);
        Ok(())
    }
    fn replace_all_in_editor(&mut self) -> Result<usize, String> {
        let matches = self.find_matches();
        let replace = self
            .find
            .as_ref()
            .map(|f| f.replace.clone())
            .unwrap_or_default();
        let tab = self.editor_mut()?;
        if matches.is_empty() {
            return Ok(0);
        }
        let mut out = String::new();
        let mut last = 0;
        for (a, b) in &matches {
            out.push_str(&tab.doc.text[last..*a]);
            out.push_str(&replace);
            last = *b;
        }
        out.push_str(&tab.doc.text[last..]);
        tab.doc.replace_all_text(&out);
        Ok(matches.len())
    }
    fn find_key(&mut self, window: u64, key: &str) -> Result<Option<Vec<AppEffect>>, String> {
        let replace_focus = self.focus == Focus::Replace;
        let find = self.find.as_mut().ok_or("the find widget is closed")?;
        match key {
            "backspace" => {
                if replace_focus {
                    pop_char(&mut find.replace);
                } else {
                    pop_char(&mut find.query);
                    self.find_step(true, true);
                }
            }
            "enter" | "f3" if !replace_focus => self.find_step(true, false),
            "shift+enter" | "shift+f3" => self.find_step(false, false),
            "enter" => self.replace_current()?,
            "ctrl+alt+enter" => {
                self.replace_all_in_editor()?;
            }
            "tab" | "shift+tab" => {
                if find.replace_open {
                    self.focus = if replace_focus {
                        Focus::Find
                    } else {
                        Focus::Replace
                    };
                }
            }
            "alt+c" => find.opts.case = !find.opts.case,
            "alt+w" => find.opts.word = !find.opts.word,
            "alt+r" => find.opts.regex = !find.opts.regex,
            _ => {
                let _ = window;
                return Ok(None);
            }
        }
        Ok(Some(vec![]))
    }
    fn field_key(&mut self, window: u64, key: &str) -> Result<Option<Vec<AppEffect>>, String> {
        match (self.focus, key) {
            (Focus::Search, "backspace") => {
                pop_char(&mut self.search.query);
                Ok(Some(self.start_search(window)))
            }
            (Focus::SearchReplace, "backspace") => {
                pop_char(&mut self.search.replace);
                Ok(Some(vec![]))
            }
            (Focus::Search, "enter") => Ok(Some(self.start_search(window))),
            (Focus::SearchReplace, "ctrl+alt+enter") | (Focus::SearchReplace, "enter") => {
                self.click(window, "code:search-replace-all", 0).map(Some)
            }
            (Focus::Search | Focus::SearchReplace, "tab") => {
                if self.search.show_replace {
                    self.focus = if self.focus == Focus::Search {
                        Focus::SearchReplace
                    } else {
                        Focus::Search
                    };
                }
                Ok(Some(vec![]))
            }
            (Focus::ScmMessage, "backspace") => {
                pop_char(&mut self.scm.message);
                Ok(Some(vec![]))
            }
            (Focus::ScmMessage, "enter") => {
                bounded(&mut self.scm.message, "\n", 4096)?;
                Ok(Some(vec![]))
            }
            (Focus::ScmMessage, "ctrl+enter") => self.run_command(window, "git.commit").map(Some),
            (Focus::Inline, "backspace") => {
                if let Some(i) = &mut self.inline {
                    pop_char(&mut i.value);
                }
                Ok(Some(vec![]))
            }
            (Focus::Inline, "enter") => self.commit_inline(window).map(Some),
            (Focus::Debug, "backspace") => {
                match &mut self.debug.prompt {
                    Some(prompt) => pop_char(&mut prompt.value),
                    None => pop_char(&mut self.debug.input),
                }
                Ok(Some(vec![]))
            }
            (Focus::Debug, "enter") => self.debug_commit(window).map(Some),
            (Focus::Debug, "escape") => {
                self.debug.prompt = None;
                self.focus = Focus::Editor;
                Ok(Some(vec![]))
            }
            _ => Ok(None),
        }
    }
    fn terminal_key(&mut self, window: u64, key: &str) -> Result<Option<Vec<AppEffect>>, String> {
        let index = self.term;
        let term = self.terminals.get_mut(index).ok_or("no terminal is open")?;
        term.cursor = term.cursor.min(term.input.len());
        while !term.input.is_char_boundary(term.cursor) {
            term.cursor -= 1;
        }
        let prev = |s: &str, c: usize| s[..c].char_indices().next_back().map_or(0, |(i, _)| i);
        match key {
            "enter" => {
                let command = std::mem::take(&mut term.input);
                term.cursor = 0;
                return Ok(Some(self.send_to_terminal(window, command, "term")));
            }
            "backspace" => {
                if term.cursor > 0 {
                    let at = prev(&term.input, term.cursor);
                    term.input.drain(at..term.cursor);
                    term.cursor = at;
                }
            }
            "delete" => {
                if let Some(c) = term.input[term.cursor..].chars().next() {
                    let end = term.cursor + c.len_utf8();
                    term.input.drain(term.cursor..end);
                }
            }
            "arrowleft" => term.cursor = prev(&term.input, term.cursor),
            "arrowright" => {
                if let Some(c) = term.input[term.cursor..].chars().next() {
                    term.cursor += c.len_utf8();
                }
            }
            "home" => term.cursor = 0,
            "ctrl+a" if self.platform != Platform::Windows => term.cursor = 0,
            "end" => term.cursor = term.input.len(),
            "arrowup" | "arrowdown" => {
                if term.history.is_empty() {
                    return Ok(Some(vec![]));
                }
                let last = term.history.len() - 1;
                let next = match (term.recall, key == "arrowup") {
                    (None, true) => Some(last),
                    (None, false) => None,
                    (Some(i), true) => Some(i.saturating_sub(1)),
                    (Some(i), false) if i < last => Some(i + 1),
                    (Some(_), false) => None,
                };
                term.recall = next;
                term.input = next.map(|i| term.history[i].clone()).unwrap_or_default();
                term.cursor = term.input.len();
            }
            // ^C abandons the line being typed, as the shell does.
            "ctrl+c" => {
                term.input.clear();
                term.cursor = 0;
            }
            "ctrl+l" => {
                term.transcript.clear();
                term.scroll = 0;
            }
            _ => return Ok(None),
        }
        Ok(Some(vec![]))
    }
    /// Rows of the Explorer as shown: (relative path, depth, is folder).
    pub fn tree_rows(&self) -> Vec<(String, usize, bool)> {
        let mut rows = Vec::new();
        self.tree_level("", 0, &mut rows);
        rows
    }
    fn tree_level(&self, dir: &str, depth: usize, rows: &mut Vec<(String, usize, bool)>) {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut children: Vec<(&str, bool)> = self
            .entries
            .iter()
            .filter_map(|e| {
                let rest = e.strip_prefix(&prefix)?;
                let name = rest.trim_end_matches('/');
                (!name.is_empty() && !name.contains('/')).then_some((name, e.ends_with('/')))
            })
            .collect();
        // Folders first, then case-insensitive by name, as the Explorer sorts.
        children.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
                .then(a.0.cmp(b.0))
        });
        for (name, is_dir) in children {
            let rel = format!("{prefix}{name}");
            rows.push((rel.clone(), depth, is_dir));
            if is_dir && self.expanded.contains(&rel) {
                self.tree_level(&rel, depth + 1, rows);
            }
        }
    }
    fn explorer_key(&mut self, window: u64, key: &str) -> Result<Option<Vec<AppEffect>>, String> {
        let rows = self.tree_rows();
        let at = self
            .selected
            .as_ref()
            .and_then(|s| rows.iter().position(|r| &r.0 == s));
        match key {
            "arrowdown" | "arrowup" => {
                let next = match (at, key == "arrowdown") {
                    (None, _) => 0,
                    (Some(i), true) => (i + 1).min(rows.len().saturating_sub(1)),
                    (Some(i), false) => i.saturating_sub(1),
                };
                self.selected = rows.get(next).map(|r| r.0.clone());
            }
            "arrowright" | "arrowleft" => {
                if let Some((rel, _, true)) = at.and_then(|i| rows.get(i)).cloned() {
                    if key == "arrowright" {
                        self.expanded.insert(rel);
                    } else {
                        self.expanded.remove(&rel);
                    }
                }
            }
            "enter" => {
                let Some((rel, _, dir)) = at.and_then(|i| rows.get(i)).cloned() else {
                    return Ok(Some(vec![]));
                };
                if dir {
                    if !self.expanded.remove(&rel) {
                        self.expanded.insert(rel);
                    }
                    return Ok(Some(vec![]));
                }
                let abs = self.abs(&rel);
                return Ok(Some(self.open_file(window, &abs, true, None)));
            }
            "f2" => return self.run_command(window, "renameFile").map(Some),
            "delete" => return self.run_command(window, "deleteFile").map(Some),
            _ => return Ok(None),
        }
        Ok(Some(vec![]))
    }
    fn editor_key(&mut self, key: &str) -> Result<Vec<AppEffect>, String> {
        let tab_size = self.settings.tab_size;
        let page = self.page.max(1) as isize;
        self.focus = Focus::Editor;
        let tab = self.editor_mut()?;
        let lang = tab.lang;
        let d = &mut tab.doc;
        d.sanitize();
        let shift = key.contains("shift+");
        let base = key.replace("shift+", "");
        let mut edited = true;
        // Every caret moves, and every caret edits: one keystroke, one undo step.
        match base.as_str() {
            "arrowleft" => d.move_each(|d| d.left(shift)),
            "arrowright" => d.move_each(|d| d.right(shift)),
            "arrowup" => d.move_each(|d| d.vertical(-1, shift)),
            "arrowdown" => d.move_each(|d| d.vertical(1, shift)),
            "ctrl+arrowleft" => d.move_each(|d| d.set(d.word_left(d.cursor), shift)),
            "ctrl+arrowright" => d.move_each(|d| d.set(d.word_right(d.cursor), shift)),
            "home" => d.move_each(|d| d.home(shift)),
            "end" => d.move_each(|d| d.end(shift)),
            "ctrl+home" => {
                d.clear_carets();
                d.set(0, shift)
            }
            "ctrl+end" => {
                d.clear_carets();
                d.set(d.text.len(), shift)
            }
            "pageup" => {
                d.clear_carets();
                d.vertical(-page, shift)
            }
            "pagedown" => {
                d.clear_carets();
                d.vertical(page, shift)
            }
            "backspace" => d.at_each(|d| d.backspace(tab_size)),
            "delete" => d.at_each(|d| d.delete()),
            "ctrl+backspace" => d.at_each(|d| d.delete_word_left()),
            "ctrl+delete" => d.at_each(|d| d.delete_word_right()),
            "enter" => d.at_each(|d| d.newline(lang, tab_size)),
            "ctrl+enter" if !shift => d.at_each(|d| d.insert_line_below(tab_size, lang)),
            "ctrl+enter" => d.at_each(|d| {
                let start = d.line_start(d.cursor);
                d.set(start, false);
                d.insert("\n");
                d.set(start, false);
            }),
            "tab" if !shift => d.at_each(|d| d.indent(tab_size)),
            "tab" => d.at_each(|d| d.outdent(tab_size)),
            "ctrl+l" => {
                d.clear_carets();
                let (a, b) = d.selection();
                let s = d.line_start(a);
                let e = (d.line_end(b) + 1).min(d.text.len());
                d.set(s, false);
                d.set(e, true);
            }
            "f3" if !shift => {
                edited = false;
                self.find_step(true, false);
            }
            "f3" => {
                edited = false;
                self.find_step(false, false);
            }
            _ => return Err(format!("unsupported editor key {key}")),
        }
        if edited {
            if let Some(tab) = self.active_mut() {
                if tab.doc.text != tab.saved {
                    tab.preview = false;
                }
            }
        }
        self.follow_caret();
        Ok(vec![])
    }

    // ----- pointer ---------------------------------------------------------------------

    /// Map a point in the editor's text area to a byte offset. `code:editor:<group>:
    /// <first row>:<first column>:<wrap columns>:<visible rows>` is how the view
    /// painted it, and the group it names takes the focus.
    fn editor_point(&mut self, target: &str, dx: i32, dy: i32) -> Option<usize> {
        let mut parts = target.strip_prefix("code:editor:")?.split(':');
        let mut num = || {
            parts
                .next()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
        };
        let (group, first, hscroll, wrap, rows) = (num(), num(), num(), num(), num());
        if group < self.group_count() && group != self.focus_group {
            self.go_to_group(group);
        }
        if rows > 0 {
            self.page = rows;
        }
        if wrap > 0 {
            self.wrap_cols = wrap;
        }
        let (cell_w, row_h) = render::cell(self.settings.font_size, self.platform);
        let tab = self.active.and_then(|i| self.tabs.get_mut(i))?;
        tab.scroll = first;
        let row = first + (dy.max(0) as u32 / row_h) as usize;
        let col = hscroll + ((dx.max(0) as u32 + cell_w / 2) / cell_w) as usize;
        let text = &tab.doc.text;
        let rows = crate::editor_rows(text, wrap);
        let Some(&(start, end)) = rows.get(row) else {
            return Some(text.len());
        };
        let content = &text[start..end];
        // Columns on screen count tab stops, so a click past a tab lands where it looks.
        let width = render::columns(content, self.settings.tab_size);
        Some(if col < width {
            start + render::byte_at_column(content, col, self.settings.tab_size)
        } else if rows.get(row + 1).is_some_and(|(next, _)| *next == end) {
            // A soft-wrapped row ends before its last character, which belongs to it.
            content
                .char_indices()
                .next_back()
                .map_or(start, |(i, _)| start + i)
        } else {
            end
        })
    }
    /// Whether `target` follows the pointer while it is held: the minimap.
    pub fn drags(&self, target: &str) -> bool {
        target.starts_with("code:minimap:")
    }
    /// The pointer on the minimap: press or drag anywhere on it and the editor scrolls
    /// to the row under the pointer, which is what VS Code's minimap does.
    pub fn pointer(
        &mut self,
        target: &str,
        phase: crate::PointerPhase,
        _x: i32,
        y: i32,
    ) -> Result<Vec<AppEffect>, String> {
        let mut parts = target
            .strip_prefix("code:minimap:")
            .ok_or("that surface is not the minimap")?
            .split(':');
        let mut num = || {
            parts
                .next()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
        };
        let (group, map_first, _holds) = (num(), num(), num());
        if group < self.group_count() && group != self.focus_group {
            self.go_to_group(group);
        }
        if phase == crate::PointerPhase::Cancel {
            return Ok(vec![]);
        }
        let page = self.page.max(1);
        let wrap = if self.settings.word_wrap {
            self.wrap_cols
        } else {
            0
        };
        let tab = self.active_mut().ok_or("no editor is open")?;
        let rows = crate::editor_rows(&tab.doc.text, wrap);
        let row = map_first + (y.max(0) as u32 / render::MINIMAP_ROW) as usize;
        // The pointer marks the middle of the view, as dragging the map does.
        tab.scroll = row
            .saturating_sub(page / 2)
            .min(rows.len().saturating_sub(1));
        tab.follow = false;
        self.focus = Focus::Editor;
        Ok(vec![])
    }
    /// A wheel turn over the editor or the terminal moves it by whole rows (the view
    /// stops following the caret, as a scrollbar drag does); the Explorer and Search
    /// lists are panes the platform scrolls. Returns whether anything moved.
    pub fn wheel(&mut self, target: &str, wheel: crate::Wheel) -> Result<bool, String> {
        if let Some(group) = target
            .strip_prefix("code:editor:")
            .and_then(|rest| rest.split(':').next())
            .and_then(|g| g.parse::<usize>().ok())
        {
            // The wheel turns the group it is over, which takes the focus with it.
            if group < self.group_count() && group != self.focus_group {
                self.go_to_group(group);
            }
        }
        if target.starts_with("code:editor") {
            let (_, row_h) = render::cell(self.settings.font_size, self.platform);
            let lines = wheel.lines(row_h as i32);
            let wrap = self.wrap_cols;
            let Some(tab) = self.active_mut() else {
                return Ok(false);
            };
            let last = crate::editor_rows(&tab.doc.text, wrap)
                .len()
                .saturating_sub(1);
            let next = (tab.scroll as i64 + i64::from(lines)).clamp(0, last as i64) as usize;
            if next == tab.scroll {
                return Ok(false);
            }
            tab.scroll = next;
            tab.follow = false;
            return Ok(true);
        }
        if target == "code:terminal"
            || target == "code:terminal-line"
            || target.starts_with("code:term-scroll")
        {
            let (_, row_h) = render::terminal_cell();
            let lines = wheel.lines(row_h as i32);
            let Some(term) = self.terminals.get_mut(self.term) else {
                return Ok(false);
            };
            // The terminal counts lines lifted off its tail: rolling back lifts more.
            let next = (term.scroll as i64 - i64::from(lines)).clamp(0, 4096) as usize;
            if next == term.scroll {
                return Ok(false);
            }
            term.scroll = next;
            return Ok(true);
        }
        Ok(false)
    }
    /// Whether a right press on `target` belongs to Visual Studio Code: its own context
    /// menus, rather than the desktop's.
    pub fn takes_secondary(&self, target: &str) -> bool {
        target.starts_with("code:tree:") || target.starts_with("code:editor:")
    }
    /// A press in the text area puts the caret there and anchors a drag selection;
    /// held with Alt it adds a cursor there instead, as VS Code's Alt+click does. The
    /// right button opens the context menu for what was pressed.
    pub fn press_at(&mut self, target: &str, dx: i32, dy: i32) -> Result<(), String> {
        if self.button == 2 {
            if let Some(rel) = target.strip_prefix("code:tree:") {
                if self.exists(rel) {
                    self.selected = Some(rel.to_owned());
                    self.focus = Focus::Explorer;
                    self.context = Some(ContextKind::Explorer);
                }
                return Ok(());
            }
            if target.starts_with("code:editor:") {
                // The caret goes where the press landed, then the menu opens there.
                if let Some(pos) = self.editor_point(target, dx, dy) {
                    if let Some(tab) = self.active_mut() {
                        let inside = {
                            let (a, b) = tab.doc.selection();
                            pos >= a && pos < b
                        };
                        if !inside {
                            tab.doc.clear_carets();
                            tab.doc.set(pos, false);
                        }
                    }
                }
                self.focus = Focus::Editor;
                self.context = Some(ContextKind::Editor);
                return Ok(());
            }
            return Ok(());
        }
        self.context = None;
        if !target.starts_with("code:editor:") {
            return Ok(());
        }
        let pos = self
            .editor_point(target, dx, dy)
            .ok_or("no editor is open")?;
        self.focus = Focus::Editor;
        self.menu = None;
        let alt = self.modifiers & crate::apps::imaging::MOD_ALT != 0;
        if let Some(tab) = self.active_mut() {
            if alt {
                tab.doc.add_caret(pos, pos);
            } else {
                tab.doc.clear_carets();
                tab.doc.set(pos, false);
            }
        }
        self.press = if alt { None } else { Some(pos) };
        Ok(())
    }
    pub fn click_at(
        &mut self,
        window: u64,
        target: &str,
        dx: i32,
        dy: i32,
        clock: u64,
    ) -> Result<Vec<AppEffect>, String> {
        if target.starts_with("code:editor:") {
            let pos = self
                .editor_point(target, dx, dy)
                .ok_or("no editor is open")?;
            self.focus = Focus::Editor;
            self.menu = None;
            self.chord = false;
            let anchor = self.press.take();
            let alt = self.modifiers & crate::apps::imaging::MOD_ALT != 0;
            if let Some(tab) = self.active_mut() {
                match anchor {
                    Some(a) => {
                        tab.doc.set(a, false);
                        tab.doc.set(pos, true);
                    }
                    // The release of an Alt+click leaves the cursor it added alone.
                    None if alt => {
                        tab.doc.add_caret(pos, pos);
                    }
                    None => {
                        tab.doc.clear_carets();
                        tab.doc.set(pos, false);
                    }
                }
            }
            return Ok(vec![]);
        }
        if target == "code:terminal-line" {
            let (cell_w, _) = render::terminal_cell();
            if let Some(term) = self.terminals.get_mut(self.term) {
                term.cursor = crate::caret_for_column(&term.input, dx * 8 / cell_w as i32);
            }
            self.focus = Focus::Terminal;
            return Ok(vec![]);
        }
        self.click(window, target, clock)
    }
    /// A double click: pins a preview editor, opens a file for keeps, selects a word.
    pub fn activate(
        &mut self,
        window: u64,
        target: &str,
        clock: u64,
    ) -> Result<Vec<AppEffect>, String> {
        if let Some(rel) = target.strip_prefix("code:tree:") {
            if !self.is_dir(rel) {
                let abs = self.abs(rel);
                return Ok(self.open_file(window, &abs, true, None));
            }
        }
        if let Some(i) = target
            .strip_prefix("code:tab:")
            .and_then(|i| i.parse::<usize>().ok())
        {
            let tab = self.tabs.get_mut(i).ok_or("no such editor")?;
            tab.preview = false;
            self.set_active(Some(i));
            return Ok(vec![]);
        }
        if target.starts_with("code:editor:") {
            if let Some(tab) = self.active_mut() {
                let start = tab.doc.word_left(tab.doc.cursor);
                let end = tab.doc.word_right(start);
                tab.doc.set(start, false);
                tab.doc.set(end, true);
            }
            return Ok(vec![]);
        }
        self.click(window, target, clock)
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        _clock: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("code:")
            .ok_or("interaction does not belong to Visual Studio Code")?;
        self.chord = false;
        let (verb, arg) = command.split_once(':').unwrap_or((command, ""));
        // Anything but a menu entry closes an open menu, and any click at all closes
        // the context menu, exactly as a click does anywhere in VS Code.
        if verb != "menu" && verb != "cmd" {
            self.menu = None;
        }
        if verb != "cmd" {
            self.context = None;
        }
        if self.dialog.is_some() && verb != "dialog" {
            return Err("a dialog is open".into());
        }
        match verb {
            "cmd" => self.run_command(window, arg),
            "menu" => {
                if commands::MENUS.iter().all(|(name, _, _)| *name != arg) {
                    return Err(format!("unknown menu {arg}"));
                }
                self.menu = if self.menu.as_deref() == Some(arg) {
                    None
                } else {
                    Some(arg.to_owned())
                };
                Ok(vec![])
            }
            "menu-close" => {
                self.menu = None;
                self.context = None;
                Ok(vec![])
            }
            "activity" => {
                let view = match arg {
                    "explorer" => View::Explorer,
                    "search" => View::Search,
                    "scm" => View::Scm,
                    "run" => View::Run,
                    _ => return Err(format!("unknown view {arg}")),
                };
                if self.sidebar && self.view == view {
                    self.sidebar = false;
                    return Ok(vec![]);
                }
                let id = match view {
                    View::Explorer => "workbench.view.explorer",
                    View::Search => "workbench.view.search",
                    View::Scm => "workbench.view.scm",
                    View::Run => "workbench.view.debug",
                };
                self.run_command(window, id)
            }
            "tree" => {
                if !self.exists(arg) {
                    return Err("that entry is no longer in the folder".into());
                }
                self.selected = Some(arg.to_owned());
                self.focus = Focus::Explorer;
                if self.is_dir(arg) {
                    if !self.expanded.remove(arg) {
                        self.expanded.insert(arg.to_owned());
                    }
                    return Ok(vec![]);
                }
                let abs = self.abs(arg);
                let effects = self.open_file(window, &abs, false, None);
                self.focus = Focus::Explorer;
                Ok(effects)
            }
            "explorer" => {
                self.focus = Focus::Explorer;
                Ok(vec![])
            }
            "inline" => {
                self.focus = Focus::Inline;
                Ok(vec![])
            }
            "tab" => {
                let i: usize = arg.parse().map_err(|_| "invalid editor")?;
                if i >= self.tabs.len() {
                    return Err("no such editor".into());
                }
                self.set_active(Some(i));
                self.focus = Focus::Editor;
                Ok(vec![])
            }
            // A click anywhere in a group's editor area gives that group the focus.
            "group" => {
                let g: usize = arg.parse().map_err(|_| "invalid editor group")?;
                if g >= self.group_count() {
                    return Err("no such editor group".into());
                }
                self.go_to_group(g);
                Ok(vec![])
            }
            "tab-close" => {
                let i: usize = arg.parse().map_err(|_| "invalid editor")?;
                self.close_tab(i, false)?;
                Ok(vec![])
            }
            "crumb" => {
                if !self.is_dir(arg) {
                    return Err("that folder is not in the workspace".into());
                }
                let mut dir = arg.to_owned();
                while !dir.is_empty() {
                    self.expanded.insert(dir.clone());
                    dir = parent(&dir).to_owned();
                }
                self.selected = Some(arg.to_owned()).filter(|a| !a.is_empty());
                self.view = View::Explorer;
                self.sidebar = true;
                self.focus = Focus::Explorer;
                Ok(vec![])
            }
            "editor" => {
                self.focus = Focus::Editor;
                Ok(vec![])
            }
            "scroll" => {
                let (group, row) = arg.split_once(':').unwrap_or(("0", arg));
                let group: usize = group.parse().map_err(|_| "invalid editor group")?;
                if group < self.group_count() && group != self.focus_group {
                    self.go_to_group(group);
                }
                let row: usize = row.parse().map_err(|_| "invalid scroll position")?;
                let tab = self.active_mut().ok_or("no editor is open")?;
                tab.scroll = row.min(tab.doc.line_count() * 4);
                tab.follow = false;
                Ok(vec![])
            }
            "find-input" => {
                self.find.get_or_insert_with(Find::default);
                self.focus = Focus::Find;
                Ok(vec![])
            }
            "replace-input" => {
                self.find.get_or_insert_with(Find::default).replace_open = true;
                self.focus = Focus::Replace;
                Ok(vec![])
            }
            "find" => {
                let find = self.find.as_mut().ok_or("the find widget is closed")?;
                match arg {
                    "case" => find.opts.case = !find.opts.case,
                    "word" => find.opts.word = !find.opts.word,
                    "regex" => find.opts.regex = !find.opts.regex,
                    "toggle-replace" => find.replace_open = !find.replace_open,
                    "next" => self.find_step(true, false),
                    "prev" => self.find_step(false, false),
                    "close" => {
                        self.find = None;
                        self.focus = Focus::Editor;
                    }
                    "replace" => self.replace_current()?,
                    "replace-all" => {
                        self.replace_all_in_editor()?;
                    }
                    _ => return Err(format!("unknown find control {arg}")),
                }
                Ok(vec![])
            }
            "search-input" => {
                self.focus = Focus::Search;
                Ok(vec![])
            }
            "search-replace-input" => {
                self.search.show_replace = true;
                self.focus = Focus::SearchReplace;
                Ok(vec![])
            }
            "search" => {
                match arg {
                    "case" => self.search.opts.case = !self.search.opts.case,
                    "word" => self.search.opts.word = !self.search.opts.word,
                    "regex" => self.search.opts.regex = !self.search.opts.regex,
                    "toggle-replace" => {
                        self.search.show_replace = !self.search.show_replace;
                        return Ok(vec![]);
                    }
                    "clear" => {
                        self.search.query.clear();
                        self.search.results.clear();
                        self.search.searched = false;
                        return Ok(vec![]);
                    }
                    "collapse" => {
                        self.search.collapsed =
                            self.search.results.iter().map(|f| f.path.clone()).collect();
                        return Ok(vec![]);
                    }
                    _ => return Err(format!("unknown search control {arg}")),
                }
                Ok(self.start_search(window))
            }
            "search-file" => {
                if !self.search.collapsed.remove(arg) {
                    self.search.collapsed.insert(arg.to_owned());
                }
                Ok(vec![])
            }
            "search-result" => {
                let (f, h) = arg.split_once(':').ok_or("invalid result")?;
                let file = self
                    .search
                    .results
                    .get(f.parse::<usize>().map_err(|_| "invalid result")?)
                    .ok_or("no such result")?;
                let hit = file
                    .hits
                    .get(h.parse::<usize>().map_err(|_| "invalid result")?)
                    .ok_or("no such result")?;
                let col = hit.preview[..hit.start.min(hit.preview.len())]
                    .chars()
                    .count()
                    + 1;
                let len = hit.preview.get(hit.start..hit.end).map_or(0, |s| s.len());
                let (line, abs) = (hit.line, self.abs(&file.path));
                // The match itself is selected, now or once the editor has the text.
                Ok(self.open_file(window, &abs, false, Some((line, col, len))))
            }
            "search-replace-all" => {
                if self.search.query.is_empty() || self.search.results.is_empty() {
                    return Err("there is nothing to replace".into());
                }
                let paths = self
                    .search
                    .results
                    .iter()
                    .map(|f| self.abs(&f.path))
                    .collect();
                Ok(vec![AppEffect::ReadFiles {
                    window,
                    tag: "replace".into(),
                    paths,
                }])
            }
            "scm-message" => {
                self.focus = Focus::ScmMessage;
                Ok(vec![])
            }
            // ----- Run and Debug -------------------------------------------------------
            // The gutter: a click where VS Code puts its breakpoint dots.
            "gutter" => {
                let (group, line) = arg.split_once(':').ok_or("invalid gutter target")?;
                let group: usize = group.parse().map_err(|_| "invalid editor group")?;
                let line: u32 = line.parse().map_err(|_| "invalid line")?;
                if group < self.group_count() && group != self.focus_group {
                    self.go_to_group(group);
                }
                let path = self
                    .active_tab()
                    .filter(|t| t.kind == TabKind::File)
                    .map(|t| t.path.clone())
                    .ok_or("that editor holds no file")?;
                self.breakpoint_at(window, &path, line)
            }
            "bp" => {
                let i: usize = arg.parse().map_err(|_| "invalid breakpoint")?;
                let point = self
                    .debug
                    .breakpoints
                    .get_mut(i)
                    .ok_or("no such breakpoint")?;
                point.enabled = !point.enabled;
                let path = point.path.clone();
                Ok(self.send_breakpoints(window, &path))
            }
            "bp-remove" => {
                let i: usize = arg.parse().map_err(|_| "invalid breakpoint")?;
                if i >= self.debug.breakpoints.len() {
                    return Err("no such breakpoint".into());
                }
                let path = self.debug.breakpoints.remove(i).path;
                Ok(self.send_breakpoints(window, &path))
            }
            "bp-open" => {
                let i: usize = arg.parse().map_err(|_| "invalid breakpoint")?;
                let point = self.debug.breakpoints.get(i).ok_or("no such breakpoint")?;
                let (path, line) = (point.path.clone(), point.line as usize);
                Ok(self.open_file(window, &path, false, Some((line, 1, 0))))
            }
            "exception" => {
                match arg {
                    "uncaught" => {
                        self.debug.exceptions.uncaught = !self.debug.exceptions.uncaught;
                    }
                    "raised" => self.debug.exceptions.raised = !self.debug.exceptions.raised,
                    _ => return Err(format!("unknown exception filter {arg}")),
                }
                Ok(self.send_exceptions(window))
            }
            "frame" => {
                let i: usize = arg.parse().map_err(|_| "invalid frame")?;
                self.select_frame(window, i)
            }
            "var" => {
                let reference: u64 = arg.parse().map_err(|_| "invalid variable")?;
                Ok(self.debug_expand(window, reference))
            }
            "watch-remove" => {
                let i: usize = arg.parse().map_err(|_| "invalid watch")?;
                if i >= self.debug.watches.len() {
                    return Err("no such watch".into());
                }
                self.debug.watches.remove(i);
                Ok(vec![])
            }
            "debug-config" => {
                let i: usize = arg.parse().map_err(|_| "invalid configuration")?;
                if i >= self.debug.configs.len() {
                    return Err("no such launch configuration".into());
                }
                self.debug.config = i;
                Ok(vec![])
            }
            "repl-input" => {
                self.focus = Focus::Debug;
                self.debug.prompt = None;
                Ok(vec![])
            }
            "debug-prompt" => {
                self.focus = Focus::Debug;
                Ok(vec![])
            }
            "debug-prompt-ok" => self.debug_commit(window),
            "debug-prompt-close" => {
                self.debug.prompt = None;
                self.focus = Focus::Editor;
                Ok(vec![])
            }
            "scm-stage" => {
                let ps = self.powershell();
                self.git_command(window, "add", format!("git add {}", quote(arg, ps)))
            }
            "scm-unstage" => {
                let ps = self.powershell();
                self.git_command(
                    window,
                    "reset",
                    format!("git restore --staged {}", quote(arg, ps)),
                )
            }
            // Discarding is destructive, so it asks first, exactly as VS Code does.
            "scm-discard" => {
                let untracked = self
                    .scm
                    .changes
                    .iter()
                    .any(|(state, path)| path == arg && *state == 'U');
                let name = basename(arg).to_owned();
                self.dialog = Some(if untracked {
                    Dialog {
                        message: format!("Are you sure you want to delete {name}?"),
                        detail: "This file is not tracked by Git; it is moved to the trash.".into(),
                        buttons: vec![
                            ("Delete file".into(), format!("discard-new:{arg}")),
                            ("Cancel".into(), "cancel".into()),
                        ],
                    }
                } else {
                    Dialog {
                        message: format!("Are you sure you want to discard changes in {name}?"),
                        detail: "This is IRREVERSIBLE! Your current working set will be FOREVER LOST if you proceed.".into(),
                        buttons: vec![
                            ("Discard Changes".into(), format!("discard:{arg}")),
                            ("Cancel".into(), "cancel".into()),
                        ],
                    }
                });
                Ok(vec![])
            }
            "scm-open" => {
                let abs = self.abs(arg);
                Ok(self.open_file(window, &abs, false, None))
            }
            "panel" => {
                self.panel = match arg {
                    "problems" => PanelTab::Problems,
                    "output" => PanelTab::Output,
                    "terminal" => PanelTab::Terminal,
                    "debug" => PanelTab::Debug,
                    _ => return Err(format!("unknown panel {arg}")),
                };
                self.panel_open = true;
                if self.panel == PanelTab::Debug {
                    self.focus = Focus::Debug;
                }
                if self.panel == PanelTab::Terminal {
                    self.focus = Focus::Terminal;
                    return self.ensure_terminal(window);
                }
                Ok(vec![])
            }
            "panel-close" => {
                self.panel_open = false;
                if self.focus == Focus::Terminal {
                    self.focus = Focus::Editor;
                }
                Ok(vec![])
            }
            "terminal" | "terminal-line" => {
                self.focus = Focus::Terminal;
                Ok(vec![])
            }
            "term-tab" => {
                let i: usize = arg.parse().map_err(|_| "invalid terminal")?;
                if i >= self.terminals.len() {
                    return Err("no such terminal".into());
                }
                self.term = i;
                self.focus = Focus::Terminal;
                Ok(vec![])
            }
            "term-scroll" => {
                let lines: usize = arg.parse().map_err(|_| "invalid scroll position")?;
                let term = self
                    .terminals
                    .get_mut(self.term)
                    .ok_or("no terminal is open")?;
                term.scroll = lines.min(4096);
                Ok(vec![])
            }
            "problem" => {
                let i: usize = arg.parse().map_err(|_| "invalid problem")?;
                let p = self
                    .problems_all()
                    .get(i)
                    .cloned()
                    .ok_or("no such problem")?;
                Ok(self.open_file(window, &p.path, true, Some((p.line, p.col, 0))))
            }
            "quick-close" => {
                self.quick = None;
                self.focus = Focus::Editor;
                Ok(vec![])
            }
            "quick-input" => {
                self.focus = Focus::Quick;
                Ok(vec![])
            }
            "quick" => {
                let i: usize = arg.parse().map_err(|_| "invalid choice")?;
                self.accept_quick(window, Some(i))
            }
            "quick-ok" => {
                let value = self
                    .quick
                    .as_ref()
                    .map(|q| q.value.trim_end_matches('/').to_owned())
                    .ok_or("no quick input is open")?;
                match self.quick.as_ref().map(|q| q.mode) {
                    Some(QuickMode::OpenFolder) => self.open_folder(window, &value),
                    _ => self.accept_quick(window, None),
                }
            }
            "dialog" => {
                let i: usize = arg.parse().map_err(|_| "invalid button")?;
                let action = self
                    .dialog
                    .as_ref()
                    .and_then(|d| d.buttons.get(i))
                    .map(|b| b.1.clone())
                    .ok_or("no such button")?;
                self.dialog_action(window, &action)
            }
            "notice-close" => {
                self.notice = None;
                Ok(vec![])
            }
            "settings" => {
                let (key, value) = arg.split_once(':').unwrap_or((arg, ""));
                match (key, value) {
                    ("theme", v) => self.settings.dark = v == "dark",
                    ("font", "+") => {
                        self.settings.font_size = (self.settings.font_size + 1).min(32)
                    }
                    ("font", "-") => {
                        self.settings.font_size = self.settings.font_size.saturating_sub(1).max(8)
                    }
                    ("wrap", _) => self.settings.word_wrap = !self.settings.word_wrap,
                    ("whitespace", _) => {
                        self.settings.render_whitespace = !self.settings.render_whitespace
                    }
                    ("tab", v) => {
                        self.settings.tab_size = v
                            .parse::<usize>()
                            .map_err(|_| "invalid tab size")?
                            .clamp(1, 8)
                    }
                    _ => return Err(format!("unknown setting {key}")),
                }
                Ok(self.save_settings(window))
            }
            "status" => {
                let id = match arg {
                    "branch" => "git.checkout",
                    "problems" => "workbench.actions.view.problems",
                    "position" => "workbench.action.gotoLine",
                    "indent" => "editor.action.indentUsingSpaces",
                    "eol" => "workbench.action.editor.changeEOL",
                    "language" => "workbench.action.editor.changeLanguageMode",
                    _ => return Err(format!("unknown status item {arg}")),
                };
                self.run_command(window, id)
            }
            "welcome" => {
                self.focus = Focus::Editor;
                Ok(vec![])
            }
            _ => Err(format!("unknown Visual Studio Code control {verb}")),
        }
    }
    /// Problems from runs plus what the JSON parser says about open JSON editors.
    pub fn problems_all(&self) -> Vec<Problem> {
        let mut out: Vec<Problem> = self
            .problems
            .iter()
            .filter(|p| p.source != "json")
            .cloned()
            .collect();
        for tab in &self.tabs {
            if tab.lang == Language::Json && tab.loaded && tab.kind != TabKind::Settings {
                let comments = tab.path.ends_with(".jsonc")
                    || basename(&tab.path) == "settings.json"
                    || basename(&tab.path).starts_with("tsconfig")
                    || tab.path.contains("/.vscode/");
                out.extend(problems::json(&tab.path, &tab.doc.text, comments));
            }
        }
        out
    }
    pub fn counts(&self) -> (usize, usize) {
        let all = self.problems_all();
        let errors = all.iter().filter(|p| p.severity == Severity::Error).count();
        (errors, all.len() - errors)
    }

    // ----- projection ------------------------------------------------------------------

    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: String| cw_protocol::PageAction {
            method: "APP".into(),
            url,
            fields: Default::default(),
        };
        let button = |id: String, text: String| E::Button {
            action: act(id.clone()),
            id,
            text,
        };
        page.elements.push(E::Heading {
            id: "code-workspace".into(),
            text: self
                .workspace()
                .map_or("Welcome".into(), |f| format!("{} — {f}", basename(f))),
            level: 1,
        });
        for (i, tab) in self.tabs.iter().enumerate() {
            let mark = if Some(i) == self.active { "● " } else { "" };
            let dirty = if tab.dirty() { " (unsaved)" } else { "" };
            let preview = if tab.preview { " (preview)" } else { "" };
            page.elements.push(button(
                format!("code:tab:{i}"),
                format!("{mark}{}{dirty}{preview}", tab.name()),
            ));
        }
        if self.sidebar && self.view == View::Explorer {
            for (rel, depth, dir) in self.tree_rows() {
                let open = if dir {
                    if self.expanded.contains(&rel) {
                        "▾ "
                    } else {
                        "▸ "
                    }
                } else {
                    ""
                };
                page.elements.push(button(
                    format!("code:tree:{rel}"),
                    format!("{}{open}{}", "  ".repeat(depth), basename(&rel)),
                ));
            }
        }
        if let Some(tab) = self.active_tab() {
            let (line, col) = tab.doc.position();
            page.elements.push(E::Heading {
                id: "code-editor-path".into(),
                text: format!(
                    "{} — Ln {}, Col {} — {}",
                    tab.path,
                    line + 1,
                    col + 1,
                    tab.lang.name()
                ),
                level: 2,
            });
            if let Some(error) = &tab.error {
                page.elements.push(E::Text {
                    id: "code-editor-error".into(),
                    text: error.clone(),
                });
            } else if tab.kind != TabKind::Settings {
                page.elements.push(E::Input {
                    id: "code-editor".into(),
                    label: tab.name(),
                    value: tab.doc.text.clone(),
                    placeholder: String::new(),
                });
            }
        }
        if self.view == View::Search && self.sidebar {
            page.elements.push(E::Input {
                id: "code-search".into(),
                label: "Search".into(),
                value: self.search.query.clone(),
                placeholder: "Search".into(),
            });
            for (f, file) in self.search.results.iter().enumerate() {
                for (h, hit) in file.hits.iter().enumerate() {
                    page.elements.push(button(
                        format!("code:search-result:{f}:{h}"),
                        format!("{}:{} {}", file.path, hit.line, hit.preview.trim()),
                    ));
                }
            }
        }
        if self.view == View::Scm && self.sidebar {
            page.elements.push(E::Input {
                id: "code-scm-message".into(),
                label: "Message".into(),
                value: self.scm.message.clone(),
                placeholder: String::new(),
            });
            for (s, p) in &self.scm.staged {
                page.elements.push(E::Text {
                    id: format!("code-scm-staged:{p}"),
                    text: format!("{s} {p} (staged)"),
                });
            }
            for (s, p) in &self.scm.changes {
                page.elements
                    .push(button(format!("code:scm-stage:{p}"), format!("{s} {p}")));
            }
        }
        if self.panel_open {
            match self.panel {
                PanelTab::Problems => {
                    for (i, p) in self.problems_all().iter().enumerate() {
                        page.elements.push(button(
                            format!("code:problem:{i}"),
                            format!("{}:{}:{} {}", p.path, p.line, p.col, p.message),
                        ));
                    }
                }
                PanelTab::Debug => {
                    for (i, line) in self.debug.console.iter().enumerate() {
                        page.elements.push(E::Text {
                            id: format!("code-repl-{i}"),
                            text: line.text.clone(),
                        });
                    }
                }
                PanelTab::Output => page.elements.push(E::Text {
                    id: "code-output".into(),
                    text: self.output.join("\n"),
                }),
                PanelTab::Terminal => {
                    if let Some(term) = self.terminal() {
                        for (i, entry) in term.transcript.iter().enumerate() {
                            let mut text = entry.echo();
                            for s in [&entry.stdout, &entry.stderr] {
                                if !s.is_empty() {
                                    text.push('\n');
                                    text.push_str(s.trim_end());
                                }
                            }
                            text.push('\n');
                            text.push_str(&entry.status());
                            page.elements.push(E::Text {
                                id: format!("code-terminal-entry:{i}"),
                                text,
                            });
                        }
                        page.elements.push(E::Input {
                            id: "code-terminal-input".into(),
                            label: term.prompt.clone(),
                            value: term.input.clone(),
                            placeholder: String::new(),
                        });
                    }
                }
            }
        }
        if let Some(quick) = &self.quick {
            page.elements.push(E::Input {
                id: "code-quick".into(),
                label: "Quick input".into(),
                value: quick.value.clone(),
                placeholder: String::new(),
            });
            for (i, item) in self.quick_items().iter().enumerate().take(50) {
                page.elements.push(button(
                    format!("code:quick:{i}"),
                    format!("{} {}", item.label, item.detail).trim().to_owned(),
                ));
            }
        }
        if let Some(dialog) = &self.dialog {
            page.elements.push(E::Text {
                id: "code-dialog".into(),
                text: format!("{}\n{}", dialog.message, dialog.detail),
            });
            for (i, (label, _)) in dialog.buttons.iter().enumerate() {
                page.elements
                    .push(button(format!("code:dialog:{i}"), label.clone()));
            }
        }
        if let Some(notice) = &self.notice {
            page.elements.push(E::Text {
                id: "code-notice".into(),
                text: notice.clone(),
            });
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        render::render(self, p, env);
    }
}
/// `files.exclude` defaults: version-control internals are not part of the workspace.
fn excluded(entry: &str) -> bool {
    let e = entry.trim_end_matches('/');
    e == ".git"
        || e.starts_with(".git/")
        || e.contains("/.git/")
        || e.ends_with("/.git")
        || e == ".DS_Store"
        || e.ends_with("/.DS_Store")
}
/// Where `text` defines `word`, as a 1-based line and column, in the shapes the
/// languages this editor knows define things in: `def`, `class`, `function`, `fn`,
/// `struct`, `enum`, `trait`, `type`, `const`, `let`, `var`, a shell function, or a
/// plain assignment at the left margin. Purely syntactic, and honest about it: it finds
/// what a person reading the file would point at.
pub fn definition_in(text: &str, word: &str) -> Option<(usize, usize)> {
    const KEYWORDS: [&str; 14] = [
        "def", "class", "fn", "function", "struct", "enum", "trait", "type", "const", "let", "var",
        "static", "mod", "impl",
    ];
    if word.is_empty() {
        return None;
    }
    for (n, line) in text.lines().enumerate() {
        let mut from = 0;
        while let Some(at) = line[from..].find(word).map(|i| i + from) {
            from = at + word.len();
            let before = &line[..at];
            let after = &line[at + word.len()..];
            let bounded = !before.ends_with(buffer::is_word) && !after.starts_with(buffer::is_word);
            if !bounded {
                continue;
            }
            let keyword = before
                .trim_end()
                .rsplit(|c: char| !buffer::is_word(c))
                .next()
                .unwrap_or("");
            let head = before.trim().is_empty();
            let assignment = head && {
                let rest = after.trim_start();
                rest.starts_with('=') && !rest.starts_with("==") && !rest.starts_with("=>")
            };
            let shell_function = head && after.trim_start().starts_with("()");
            if KEYWORDS.contains(&keyword) || assignment || shell_function {
                return Some((n + 1, before.chars().count() + 1));
            }
        }
    }
    None
}
/// Put the caret at a 1-based line and column, selecting `len` bytes from there.
fn reveal(doc: &mut Doc, (line, col, len): (usize, usize, usize)) {
    doc.goto(line, col);
    if len > 0 {
        let end = (doc.cursor + len).min(doc.text.len());
        doc.set(end, true);
    }
}
fn parse_line(s: &str) -> Option<(usize, usize)> {
    let s = s.trim();
    let (l, c) = s.split_once([':', ',']).unwrap_or((s, "1"));
    let line = l.trim().parse::<usize>().ok().filter(|l| *l > 0)?;
    let col = c.trim().parse::<usize>().unwrap_or(1).max(1);
    Some((line, col))
}

#[cfg(test)]
mod tests;
