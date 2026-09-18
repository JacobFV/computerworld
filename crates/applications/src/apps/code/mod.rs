//! Visual Studio Code. A real editor over the machine's own filesystem: the Explorer is
//! a listing of a real folder, a tab is a real file's text, Save writes it back, the
//! integrated terminal is a session of the machine's shell, Run executes the file with
//! that shell's own interpreters, and Source Control drives the machine's git. Nothing
//! on screen is a stand-in: a control either does what it says or is shown disabled.
pub mod buffer;
pub mod commands;
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
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Focus {
    #[default]
    Editor,
    Explorer,
    Terminal,
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
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            dark: true,
            font_size: 14,
            word_wrap: false,
            tab_size: 4,
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
    pub active: Option<usize>,
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
    }
    pub fn settings_json(&self) -> String {
        let value = serde_json::json!({
            "workbench.colorTheme": if self.settings.dark { "Default Dark Modern" } else { "Default Light Modern" },
            "editor.fontSize": self.settings.font_size,
            "editor.wordWrap": if self.settings.word_wrap { "on" } else { "off" },
            "editor.tabSize": self.settings.tab_size,
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
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.kind == TabKind::File && t.path == path)
        {
            self.active = Some(i);
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
        let tab = Tab {
            path: path.to_owned(),
            kind: TabKind::File,
            preview: !pin,
            lang: Language::from_path(path),
            reveal: reveal_at,
            ..Tab::default()
        };
        let slot = self
            .tabs
            .iter()
            .position(|t| t.preview && !t.dirty())
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
        self.active = Some(index);
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
        self.tabs.remove(index);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(a) if a > index => Some(a - 1),
            Some(a) if a == index => Some(index.min(self.tabs.len() - 1)),
            other => other,
        };
        if self.tabs.is_empty() {
            self.find = None;
        }
        Ok(())
    }
    fn save_tab(&mut self, window: u64, index: usize) -> Result<Vec<AppEffect>, String> {
        let tab = self.tabs.get(index).ok_or("no editor is open")?;
        match tab.kind {
            TabKind::Settings => return Ok(vec![]),
            TabKind::Untitled => {
                self.active = Some(index);
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
            "renameFile" | "deleteFile" => folder && self.selected.is_some(),
            "workbench.action.terminal.kill" | "workbench.action.terminal.clear" => {
                !self.terminals.is_empty()
            }
            "workbench.action.terminal.new" => self.terminals.len() < TERMINAL_LIMIT,
            "workbench.action.terminal.runActiveFile"
            | "workbench.action.debug.start"
            | "workbench.action.debug.run" => runnable,
            "python.execInTerminal" => {
                tab.is_some_and(|t| t.kind == TabKind::File && t.lang == Language::Python)
            }
            "git.init" => folder && self.scm.repo == Some(false),
            "git.refresh" => folder,
            "git.stageAll" => repo && !self.scm.changes.is_empty(),
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
            "renameFile" | "deleteFile" => "Select a file in the Explorer first",
            "git.init" => "The folder already has a repository, or no folder is open",
            "git.stageAll" | "git.commit" => "There are no changes",
            "git.checkout" | "git.refresh" => "No repository is open",
            "workbench.action.terminal.new" => "The terminal limit is reached",
            "workbench.action.terminal.kill" | "workbench.action.terminal.clear" => {
                "No terminal is open"
            }
            "workbench.action.terminal.runActiveFile"
            | "workbench.action.debug.start"
            | "workbench.action.debug.run"
            | "python.execInTerminal" => "Open a Python, JavaScript or shell file to run it",
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
        self.enabled(id).map_err(str::to_owned)?;
        self.menu = None;
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
                self.tabs.insert(
                    at,
                    Tab {
                        path: name,
                        kind: TabKind::Untitled,
                        loaded: true,
                        lang: Language::PlainText,
                        ..Tab::default()
                    },
                );
                self.active = Some(at);
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
                    self.active = Some(i);
                    self.close_tab(i, false)?;
                    return Ok(vec![]);
                }
                self.tabs.clear();
                self.active = None;
                self.find = None;
                Ok(vec![])
            }
            "workbench.action.keepEditor" => {
                self.active_mut().ok_or("no editor is open")?.preview = false;
                Ok(vec![])
            }
            "workbench.action.nextEditor" | "workbench.action.previousEditor" => {
                let n = self.tabs.len();
                let a = self.active.unwrap_or(0);
                self.active = Some(if id.ends_with("nextEditor") {
                    (a + 1) % n
                } else {
                    (a + n - 1) % n
                });
                self.focus = Focus::Editor;
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
            | "workbench.action.debug.start"
            | "workbench.action.debug.run" => self.run_active(window),
            "editor.action.toggleWordWrap" => {
                self.settings.word_wrap = !self.settings.word_wrap;
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
                    self.active = Some(i);
                } else {
                    let at = self
                        .active
                        .map_or(self.tabs.len(), |a| a + 1)
                        .min(self.tabs.len());
                    self.tabs.insert(
                        at,
                        Tab {
                            path: "Settings".into(),
                            kind: TabKind::Settings,
                            loaded: true,
                            ..Tab::default()
                        },
                    );
                    self.active = Some(at);
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
                let mut chars = text.chars();
                match (chars.next(), chars.next()) {
                    (Some('\n'), None) => tab.doc.newline(lang, tab_size),
                    (Some('\t'), None) => tab.doc.indent(tab_size),
                    // One character is a keystroke: brackets close and are typed over.
                    (Some(c), None) => tab.doc.type_char(c, lang),
                    // A run of text arrives as it was written, the way a paste does, so
                    // its own indentation is not indented again.
                    _ => tab.doc.insert(&text.replace("\r\n", "\n")),
                }
                tab.preview = false;
                self.follow_caret();
                Ok(vec![])
            }
        }
    }
    pub fn paste(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
        match self.focus {
            Focus::Editor | Focus::Explorer => {
                self.focus = Focus::Editor;
                let tab = self.editor_mut()?;
                tab.doc.insert(&text.replace("\r\n", "\n"));
                tab.preview = false;
                self.follow_caret();
                Ok(vec![])
            }
            _ => self.text_effects(window, &text.replace(['\n', '\r'], " ")),
        }
    }
    pub fn key(
        &mut self,
        window: u64,
        key: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
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
            Focus::Search | Focus::SearchReplace | Focus::ScmMessage | Focus::Inline => {
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
        match base.as_str() {
            "arrowleft" => d.left(shift),
            "arrowright" => d.right(shift),
            "arrowup" => d.vertical(-1, shift),
            "arrowdown" => d.vertical(1, shift),
            "ctrl+arrowleft" => d.set(d.word_left(d.cursor), shift),
            "ctrl+arrowright" => d.set(d.word_right(d.cursor), shift),
            "home" => d.home(shift),
            "end" => d.end(shift),
            "ctrl+home" => d.set(0, shift),
            "ctrl+end" => d.set(d.text.len(), shift),
            "pageup" => d.vertical(-page, shift),
            "pagedown" => d.vertical(page, shift),
            "backspace" => d.backspace(tab_size),
            "delete" => d.delete(),
            "ctrl+backspace" => d.delete_word_left(),
            "ctrl+delete" => d.delete_word_right(),
            "enter" if !shift => d.newline(lang, tab_size),
            "enter" => d.newline(lang, tab_size),
            "ctrl+enter" if !shift => d.insert_line_below(tab_size, lang),
            "ctrl+enter" => {
                let start = d.line_start(d.cursor);
                d.set(start, false);
                d.insert("\n");
                d.set(start, false);
            }
            "tab" if !shift => d.indent(tab_size),
            "tab" => d.outdent(tab_size),
            "ctrl+l" => {
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

    /// Map a point in the editor's text area to a byte offset. `code:editor:<first
    /// row>:<first column>:<wrap columns>:<visible rows>` is how the view painted it.
    fn editor_point(&mut self, target: &str, dx: i32, dy: i32) -> Option<usize> {
        let mut parts = target.strip_prefix("code:editor:")?.split(':');
        let mut num = || {
            parts
                .next()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
        };
        let (first, hscroll, wrap, rows) = (num(), num(), num(), num());
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
        Some(match content.char_indices().nth(col) {
            Some((i, _)) => start + i,
            None if rows.get(row + 1).is_some_and(|(next, _)| *next == end) => content
                .char_indices()
                .next_back()
                .map_or(start, |(i, _)| start + i),
            None => end,
        })
    }
    /// A press in the text area puts the caret there and anchors a drag selection.
    pub fn press_at(&mut self, target: &str, dx: i32, dy: i32) -> Result<(), String> {
        if !target.starts_with("code:editor:") {
            return Ok(());
        }
        let pos = self
            .editor_point(target, dx, dy)
            .ok_or("no editor is open")?;
        self.focus = Focus::Editor;
        self.menu = None;
        if let Some(tab) = self.active_mut() {
            tab.doc.set(pos, false);
        }
        self.press = Some(pos);
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
            if let Some(tab) = self.active_mut() {
                match anchor {
                    Some(a) => {
                        tab.doc.set(a, false);
                        tab.doc.set(pos, true);
                    }
                    None => tab.doc.set(pos, false),
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
            self.active = Some(i);
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
        // Anything but a menu entry closes an open menu.
        if verb != "menu" && verb != "cmd" {
            self.menu = None;
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
                self.active = Some(i);
                self.focus = Focus::Editor;
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
                let row: usize = arg.parse().map_err(|_| "invalid scroll position")?;
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
            "scm-stage" => {
                let ps = self.powershell();
                self.git_command(window, "add", format!("git add {}", quote(arg, ps)))
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
                    _ => return Err(format!("unknown panel {arg}")),
                };
                self.panel_open = true;
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
