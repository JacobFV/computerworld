//! Serializable desktop applications. Effects are requests to the environment,
//! never ambient filesystem access or subprocess execution.
pub mod apps;
pub mod desktop_scene;
pub use apps::{AppEnv, NativeApp};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-tab back/forward depth, and file manager tabs per window.
const HISTORY_LIMIT: usize = 64;
/// Retained shell collections. A long session must not grow a snapshot without limit.
const BOOKMARK_LIMIT: usize = 64;
const DOWNLOAD_LIMIT: usize = 64;
const NOTICE_LIMIT: usize = 64;
/// Virtual desktops. More than this is a filing system, not a workspace switcher.
const WORKSPACE_LIMIT: u32 = 8;
const TAB_LIMIT: usize = 16;
/// Terminal scrollback bounds. A session runs as long as the actor likes; the snapshot
/// must not grow with it, so the transcript keeps the newest entries and truncates
/// oversized streams instead of holding everything a command ever printed.
const TRANSCRIPT_LIMIT: usize = 200;
const STREAM_LIMIT: usize = 4096;
/// Everything a file manager retains grows with use, so each of these is capped the
/// way the transcript is: a search query, a rename buffer, the clipboard and the
/// recent-files list must never make a snapshot a function of session length.
const FIELD_LIMIT: usize = 64;
const CLIPBOARD_LIMIT: usize = 32;
const RECENT_LIMIT: usize = 16;
/// Lines a terminal may be scrolled back by. The view clamps to the output it actually
/// has; this only stops a stored offset growing without bound.
const SCROLL_LIMIT: usize = 4096;
/// Text the clipboard holds. A copy of a whole large file is refused, not truncated.
const TEXT_CLIPBOARD_LIMIT: usize = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEffect {
    ReadFile {
        window: u64,
        path: String,
    },
    WriteFile {
        window: u64,
        path: String,
        content: String,
    },
    ListDirectory {
        window: u64,
        tab: usize,
        path: String,
    },
    Execute {
        window: u64,
        command: String,
    },
    Navigate {
        window: u64,
        url: String,
    },
    /// Application request against the simulated network. `tag` routes the reply back to
    /// the request that made it, so one window may have several in flight.
    Http {
        window: u64,
        tag: String,
        method: String,
        url: String,
        body: String,
    },
    /// Create a folder, and any missing parent, before writing into it. An application
    /// that keeps its own store has to be able to make it on a fresh machine.
    CreateDirectory {
        window: u64,
        path: String,
    },
    /// Write the page a browser window is showing to the machine's Downloads folder.
    /// The shell asks; the environment, which owns the network and the filesystem, does it.
    Download {
        window: u64,
        url: String,
    },
    /// Rasterise what is on screen and save it. A screenshot of a simulated screen is a
    /// real file, not a pretence.
    Screenshot {
        window: u64,
        path: String,
    },
    /// Decode an image file on this machine into pixels an application can draw. Plain
    /// `ReadFile` cannot carry it: it delivers lossy UTF-8, which destroys image bytes.
    ReadImage {
        window: u64,
        path: String,
    },
    /// Open another application; the shell owns window creation and focus.
    Launch {
        window: u64,
        kind: String,
        argument: String,
    },
    /// Create an empty file. Refused when something is already at `path`: a New
    /// command must never overwrite what the folder already holds.
    CreateFile {
        window: u64,
        path: String,
    },
    /// Copy a file, or a whole folder tree. Refused when `to` exists.
    CopyPath {
        window: u64,
        from: String,
        to: String,
    },
    /// Move or rename. Refused when `to` exists, so a move cannot silently replace.
    MovePath {
        window: u64,
        from: String,
        to: String,
    },
    /// Move into `trash`. The file manager never hard-deletes: what a user throws away
    /// is still in the snapshot, under a suffixed name if that one is taken.
    TrashPath {
        window: u64,
        path: String,
        trash: String,
    },
    /// List a folder tree `depth` levels deep, as paths relative to `path` with folders
    /// marked by a trailing `/`. Delivered to `DesktopState::tree_listed`, path in hand,
    /// so an application can have several folders in flight.
    ListTree {
        window: u64,
        path: String,
        depth: u32,
    },
    /// Read several files at once; each answer (content or the reason it could not be
    /// read) comes back with its path to `DesktopState::files_read`, under `tag`.
    ReadFiles {
        window: u64,
        tag: String,
        paths: Vec<String>,
    },
    /// Run a command line in the machine's own shell, as a session whose working
    /// directory is `cwd`. The machine's global shell stays where it was; `cd` inside
    /// the session moves only the session. An empty command runs nothing and only asks
    /// for the prompt. The result goes to `DesktopState::shell_ran` under `tag`.
    ShellRun {
        window: u64,
        tag: String,
        cwd: String,
        command: String,
    },
    /// Put text on the machine's clipboard.
    CopyText {
        window: u64,
        text: String,
    },
    /// Ask for the clipboard's text to be pasted into the window that asked.
    Paste {
        window: u64,
    },
}
/// What a `ShellRun` produced: the finished command (`None` when only the prompt was
/// asked for), where the session stands afterwards and the prompt it would print next.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellOutcome {
    pub entry: Option<TerminalEntry>,
    pub cwd: String,
    pub prompt: String,
    /// The command was `clear`: the session's screen is wiped rather than written to.
    pub clear: bool,
}
impl AppEffect {
    /// Applications may only speak the methods the services implement.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Http { method, url, .. } => {
                if !matches!(method.as_str(), "GET" | "POST" | "PATCH" | "PUT" | "DELETE") {
                    return Err(format!("unsupported application method {method}"));
                }
                if url.is_empty() {
                    return Err("application request needs a URL".into());
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
/// List or icon grid. Purely how the same rows are laid out; the order and the click
/// targets are identical in both, so a view change can never move a file under a click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileView {
    #[default]
    List,
    Grid,
}
/// The two columns a listing really has. There is deliberately no size or date key: a
/// listing carries neither, and a column that sorted on invented metadata would be a
/// lie an observer could not see through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Name,
    Kind,
}
impl SortKey {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "name" => Some(Self::Name),
            "kind" => Some(Self::Kind),
            _ => None,
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Kind => "kind",
        }
    }
}
/// What a tab is listing. `Recents` holds absolute paths taken from
/// `DesktopState::recents`, which is why an entry there is not a child of `path`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileScope {
    #[default]
    Folder,
    Recents,
}
/// A rename in progress: the entry it started from, and the name being typed. Held
/// separately from the entry so a listing arriving mid-edit cannot retarget it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rename {
    pub from: String,
    pub name: String,
}
/// Paths cut or copied in a file manager. Lives on the desktop, not the tab, because a
/// clipboard is shared: copy in one window, paste in another.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clipboard {
    pub paths: Vec<String>,
    /// A cut moves on paste; a copy duplicates.
    pub cut: bool,
}
impl Clipboard {
    /// Bounded at `CLIPBOARD_LIMIT`: a clipboard holds a handful of paths, not a tree.
    pub fn new(mut paths: Vec<String>, cut: bool) -> Self {
        paths.truncate(CLIPBOARD_LIMIT);
        Self { paths, cut }
    }
}
/// One folder view inside a file manager window, with its own listing,
/// selection and back/forward history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTab {
    pub path: String,
    #[serde(default)]
    pub entries: Vec<String>,
    /// Index into `entries`, not into the displayed rows: a file stays selected when
    /// the sort or the filter moves it.
    #[serde(default)]
    pub selected: Option<usize>,
    /// Visited folders, oldest first; `position` is the one on screen.
    #[serde(default)]
    pub history: Vec<String>,
    #[serde(default)]
    pub position: usize,
    #[serde(default)]
    pub view: FileView,
    #[serde(default)]
    pub sort: SortKey,
    #[serde(default)]
    pub descending: bool,
    /// Substring filter applied to the listing, capped at `FIELD_LIMIT`.
    #[serde(default)]
    pub query: String,
    /// True while the search field is collecting keystrokes.
    #[serde(default)]
    pub searching: bool,
    #[serde(default)]
    pub rename: Option<Rename>,
    #[serde(default)]
    pub scope: FileScope,
}
impl FileTab {
    pub fn new(path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            entries: vec![],
            selected: None,
            history: vec![path.clone()],
            position: 0,
            view: FileView::default(),
            sort: SortKey::default(),
            descending: false,
            query: String::new(),
            searching: false,
            rename: None,
            scope: FileScope::default(),
            path,
        }
    }
    pub fn can_go_back(&self) -> bool {
        self.position > 0
    }
    pub fn can_go_forward(&self) -> bool {
        self.position + 1 < self.history.len()
    }
    /// Folder name for the tab label; the root keeps its separator.
    pub fn name(&self) -> &str {
        self.path
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("/")
    }
    pub fn selection(&self) -> Option<&String> {
        self.entries.get(self.selected?)
    }
    /// Absolute path of `entry` inside this folder.
    pub fn child(&self, entry: &str) -> String {
        format!("{}/{}", self.path.trim_end_matches('/'), entry)
    }
    /// The rows on screen, in the order the screen shows them, as indices into
    /// `entries`. This is the one place the filter and the sort are applied, and it is
    /// what `open:<i>` indexes: `i` is a position on screen, never a position in the
    /// raw listing, so reordering the view can never open the file next to the one
    /// that was clicked. Both the painter and the semantic page walk this list.
    pub fn display(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        let mut rows: Vec<usize> = (0..self.entries.len())
            .filter(|i| query.is_empty() || self.entries[*i].to_lowercase().contains(&query))
            .collect();
        rows.sort_by(|a, b| {
            let (x, y) = (self.entries[*a].as_str(), self.entries[*b].as_str());
            let order = match self.sort {
                // Folders before files, then by name, so the Kind column really groups.
                SortKey::Kind => x.ends_with('/').cmp(&y.ends_with('/')).reverse(),
                SortKey::Name => std::cmp::Ordering::Equal,
            }
            .then_with(|| entry_name(x).cmp(entry_name(y)));
            if self.descending {
                order.reverse()
            } else {
                order
            }
            // Index last: a total order keeps the projection deterministic even if a
            // listing ever repeats a name.
            .then(a.cmp(b))
        });
        rows
    }
    /// Screen row of `entries[index]`, or `None` when the filter hides it.
    pub fn row_of(&self, index: usize) -> Option<usize> {
        self.display().into_iter().position(|i| i == index)
    }
    /// Absolute path of the selected entry, in either scope.
    pub fn selected_path(&self) -> Option<String> {
        let entry = entry_name(self.selection()?).to_owned();
        Some(match self.scope {
            FileScope::Recents => entry,
            FileScope::Folder => self.child(&entry),
        })
    }
    /// The field collecting keystrokes, if any. A file manager opens one at a time.
    /// Whether this tab currently has a text field open, so the router and the shell
    /// agree about where a keystroke goes without either guessing.
    pub fn editing_text(&self) -> bool {
        self.rename.is_some() || self.searching
    }
    fn field_mut(&mut self) -> Option<&mut String> {
        match (&mut self.rename, self.searching) {
            (Some(rename), _) => Some(&mut rename.name),
            (None, true) => Some(&mut self.query),
            (None, false) => None,
        }
    }
    /// Leave every text field. Any navigation ends an edit rather than carrying a
    /// half-typed name to a folder it was never meant for.
    fn stop_editing(&mut self) {
        self.rename = None;
        self.searching = false;
    }
}
/// An entry as a name: the listing marks folders with a trailing separator, which is
/// display, not part of the path.
pub fn entry_name(entry: &str) -> &str {
    entry.trim_end_matches('/')
}
/// First `stem<suffix>extension` the listing does not already hold. Deterministic, and
/// derived only from what the tab was told is there; the kernel still refuses a
/// destination that exists, so a stale listing cannot overwrite anything.
fn free_name(entries: &[String], stem: &str, extension: &str, joiner: &str) -> String {
    let taken = |name: &str| entries.iter().any(|e| entry_name(e) == name);
    let first = format!("{stem}{extension}");
    if !taken(&first) {
        return first;
    }
    for n in 2..=99 {
        let candidate = format!("{stem}{joiner}{n}{extension}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    format!("{stem}{joiner}100{extension}")
}
/// The shell prompt this machine would print, in its own dialect. Derived from machine
/// facts so an observer reading the screen sees what the machine says, not what the
/// harness guessed: there is no synthesised prompt left to label as an efference copy.
pub fn shell_prompt(user: &str, host: &str, cwd: &str, dialect: &str) -> String {
    if dialect == "powershell" {
        // The VFS is posix underneath; powershell shows the drive path it would show.
        let path = cwd.trim_start_matches('/').replace('/', "\\");
        return if path.as_bytes().get(1) == Some(&b':') {
            format!("PS {path}>")
        } else {
            format!("PS C:\\{path}>")
        };
    }
    format!("{user}@{host}:{cwd}$")
}
/// One finished command in a terminal session: the prompt as it stood when the command
/// ran, the command itself, both streams and the exit status. `exit_code` is the point —
/// success is a number here, not a regex over `stderr`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEntry {
    pub prompt: String,
    pub command: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub exit_code: i32,
}
impl TerminalEntry {
    pub fn new(
        prompt: &str,
        command: impl Into<String>,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> Self {
        Self {
            prompt: prompt_or_sigil(prompt).into(),
            command: command.into(),
            stdout: clamp_stream(stdout),
            stderr: clamp_stream(stderr),
            exit_code,
        }
    }
    /// The echoed command line, exactly as the screen shows it.
    pub fn echo(&self) -> String {
        format!("{} {}", self.prompt, self.command)
    }
    pub fn failed(&self) -> bool {
        self.exit_code != 0
    }
    /// Unambiguous end-of-command marker; `[exit 0]` and `[exit 1]` differ by a character
    /// an observer can match, where error prose differs by wording.
    pub fn status(&self) -> String {
        format!("[exit {}]", self.exit_code)
    }
}
/// Shared fallback so the screen, the semantic page and stored entries never disagree.
pub fn prompt_or_sigil(prompt: &str) -> &str {
    if prompt.is_empty() {
        "$"
    } else {
        prompt
    }
}
/// Keep the head of a stream and say how much was dropped: a listing is read from the
/// top, and a silent cut would look like a shorter result.
fn clamp_stream(text: &str) -> String {
    if text.len() <= STREAM_LIMIT {
        return text.to_owned();
    }
    let mut cut = STREAM_LIMIT;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n[… {} bytes truncated]", &text[..cut], text.len() - cut)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppState {
    Terminal {
        input: String,
        /// Prompt on the line being typed now; mirrors `DesktopState::prompt` so the
        /// renderer and the semantic page agree without reaching back to the machine.
        #[serde(default)]
        prompt: String,
        /// Finished commands, oldest first, capped at `TRANSCRIPT_LIMIT`.
        #[serde(default)]
        transcript: Vec<TerminalEntry>,
        history: Vec<String>,
        /// Byte offset of the caret within `input`. Clicking the prompt line moves it,
        /// so typing and Backspace act where the pointer landed, not at the end.
        #[serde(default)]
        cursor: usize,
        /// Wrapped lines the view is lifted above the tail of the output. 0 follows new
        /// output, and any output pins it back to 0 the way a shell does.
        #[serde(default)]
        scroll: usize,
    },
    Editor {
        path: String,
        text: String,
        cursor: usize,
        dirty: bool,
    },
    Files {
        tabs: Vec<FileTab>,
        active: usize,
    },
    Browser {
        address: String,
    },
    /// Applications that ship with the simulator and draw their own platform chrome.
    Native(NativeApp),
}
impl AppState {
    pub fn file_tab(&self) -> Option<&FileTab> {
        match self {
            Self::Files { tabs, active } => tabs.get(*active).or_else(|| tabs.first()),
            _ => None,
        }
    }
    pub fn file_tab_mut(&mut self) -> Option<&mut FileTab> {
        match self {
            Self::Files { tabs, active } => {
                let index = (*active).min(tabs.len().checked_sub(1)?);
                tabs.get_mut(index)
            }
            _ => None,
        }
    }
    /// Folder shown by a file manager; empty for every other application.
    pub fn file_path(&self) -> &str {
        self.file_tab().map(|t| t.path.as_str()).unwrap_or("")
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub app_id: String,
    pub state: AppState,
    #[serde(default)]
    pub minimized: bool,
    #[serde(default)]
    pub frame: Option<cw_scene::Rect>,
    #[serde(default)]
    pub restored_frame: Option<cw_scene::Rect>,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default)]
    pub snapped: Option<WindowSnap>,
    /// Virtual desktop this window lives on.
    #[serde(default)]
    pub workspace: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopState {
    pub windows: BTreeMap<u64, Window>,
    pub focused: Option<u64>,
    next_id: u64,
    #[serde(default)]
    pub launcher_open: bool,
    #[serde(default)]
    pub panel: Option<String>,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub maximized: bool,
    /// Bottom to top. Unlike IDs, this order changes whenever a window is raised.
    #[serde(default)]
    pub stacking: Vec<u64>,
    #[serde(default)]
    pub pointer_capture: Option<PointerCapture>,
    /// Home folder of this machine's user; empty falls back to the root.
    #[serde(default)]
    pub home: String,
    /// Prompt this machine's shell would print next, refreshed after every command.
    /// Set from the machine like `home`; empty falls back to a bare sigil.
    #[serde(default)]
    pub prompt: String,
    /// World clock at the start of the current action, mirrored from `Runtime::tick()`.
    /// Applications read time from here and never from the host.
    #[serde(default)]
    pub clock_us: u64,
    /// Desktop icon the user has selected but not yet opened.
    #[serde(default)]
    pub desktop_selection: Option<String>,
    #[serde(default)]
    pub settings: SystemSettings,
    /// What the display is showing. Power controls really move between these.
    #[serde(default)]
    pub screen: ScreenState,
    /// Months the calendar panel is showing away from the world's current month, so a
    /// shell can page a month grid without inventing a clock of its own.
    #[serde(default)]
    pub panel_month: i32,
    /// Modifier and plane of the on-screen keyboard, so a painted shift key is a real one.
    #[serde(default)]
    pub keyboard: KeyboardState,
    /// What Cut and Copy put down and Paste picks up, shared by every file manager
    /// window. Capped at `CLIPBOARD_LIMIT` paths.
    #[serde(default)]
    pub clipboard: Option<Clipboard>,
    /// Text cut or copied in an editor, shared by every application on the machine.
    /// Capped at `TEXT_CLIPBOARD_LIMIT` bytes.
    #[serde(default)]
    pub clipboard_text: Option<String>,
    /// Documents opened from a file manager, newest first, capped at `RECENT_LIMIT`.
    /// Real history, not a guess: an entry is only here because it was opened.
    #[serde(default)]
    pub recents: Vec<String>,
    /// Pages the user really saved, shared by every browser window on the machine.
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    /// Files fetched out of the browser and written to the machine, newest first.
    #[serde(default)]
    pub downloads: Vec<Download>,
    /// Things the machine wants to tell the user. Posted by applications that saw them,
    /// never invented by the shell that draws them.
    #[serde(default)]
    pub notifications: Vec<Notice>,
    /// Virtual desktops. Index 0 always exists; `workspace` is the one on screen and
    /// every window records the one it belongs to.
    #[serde(default)]
    pub workspaces: u32,
    #[serde(default)]
    pub workspace: u32,
    /// App-library or launcher category the user has expanded, if any.
    #[serde(default)]
    pub library_group: Option<String>,
    /// A panel opened over the launcher rather than instead of it, the way a power
    /// flyout sits over an open Start menu.
    #[serde(default)]
    pub panel_over_launcher: bool,
    /// Page of a paged phone home screen (SpringBoard) on display, 0 first. Swipes and
    /// the page dots move it; shells clamp it to the pages the screen really has.
    #[serde(default)]
    pub home_page: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub name: String,
    pub path: String,
    pub url: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// Application that posted it, so a shell can draw its icon.
    pub app: String,
    pub title: String,
    pub body: String,
    pub time_us: u64,
    /// What opening it does, as an interaction id the shell can dispatch.
    pub action: Option<String>,
    pub seen: bool,
}
/// Modifier and plane of an on-screen keyboard. A soft keyboard that cannot shift can
/// only type lowercase, so this is what lets a painted shift key be a real one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardState {
    pub shift: Shift,
    pub plane: Plane,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shift {
    #[default]
    Off,
    /// Applies to the next character only, then releases.
    Once,
    Lock,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plane {
    #[default]
    Letters,
    Numbers,
    Symbols,
}
impl KeyboardState {
    /// Tap order of a real shift key: off, then next-character-only, then locked.
    pub fn cycle_shift(&mut self) {
        self.shift = match self.shift {
            Shift::Off => Shift::Once,
            Shift::Once => Shift::Lock,
            Shift::Lock => Shift::Off,
        };
    }
    pub fn upper(&self) -> bool {
        self.shift != Shift::Off
    }
    /// The character a letter key types. Typing releases a one-shot shift.
    pub fn apply(&mut self, ch: char) -> String {
        let out = if self.upper() {
            ch.to_uppercase().to_string()
        } else {
            ch.to_string()
        };
        if self.shift == Shift::Once {
            self.shift = Shift::Off;
        }
        out
    }
    pub fn set_plane(&mut self, plane: Plane) {
        self.plane = plane;
        // Leaving the letter plane drops a pending shift, as every phone keyboard does.
        if plane != Plane::Letters {
            self.shift = Shift::Off;
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenState {
    #[default]
    Active,
    Locked,
    Off,
}
/// Device state behind the quick settings, control centre and shade. These are real
/// switches: the shells read them back and every control that draws one flips one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSettings {
    /// Percentages, 0 to 100.
    pub brightness: u8,
    pub volume: u8,
    pub wifi: bool,
    pub bluetooth: bool,
    pub airplane_mode: bool,
    pub do_not_disturb: bool,
    pub night_light: bool,
    pub dark_mode: bool,
    pub rotation_lock: bool,
    pub flashlight: bool,
    pub battery_saver: bool,
    pub hotspot: bool,
    /// Text editors soft-wrap long lines at the window edge instead of running them
    /// off it, as Notepad's Word wrap and TextEdit's Wrap to Window do.
    #[serde(default)]
    pub word_wrap: bool,
}
impl SystemSettings {
    pub const DEFAULT: Self = Self {
        brightness: 80,
        volume: 60,
        wifi: true,
        bluetooth: true,
        airplane_mode: false,
        do_not_disturb: false,
        night_light: false,
        dark_mode: false,
        rotation_lock: false,
        flashlight: false,
        battery_saver: false,
        hotspot: false,
        word_wrap: false,
    };
}
impl Default for SystemSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}
impl SystemSettings {
    pub fn toggle(&mut self, name: &str) -> Result<bool, String> {
        let flag = self.flag_mut(name)?;
        *flag = !*flag;
        let value = *flag;
        // Flight mode owns the radios, exactly as a phone does.
        if name == "airplane_mode" && value {
            self.wifi = false;
            self.bluetooth = false;
            self.hotspot = false;
        }
        if matches!(name, "wifi" | "bluetooth" | "hotspot") && value {
            self.airplane_mode = false;
        }
        Ok(value)
    }
    pub fn flag(&self, name: &str) -> Result<bool, String> {
        Ok(*Self::flag_of(self, name)?)
    }
    fn flag_of<'a>(&'a self, name: &str) -> Result<&'a bool, String> {
        Ok(match name {
            "wifi" => &self.wifi,
            "bluetooth" => &self.bluetooth,
            "airplane_mode" => &self.airplane_mode,
            "do_not_disturb" => &self.do_not_disturb,
            "night_light" => &self.night_light,
            "dark_mode" => &self.dark_mode,
            "rotation_lock" => &self.rotation_lock,
            "flashlight" => &self.flashlight,
            "battery_saver" => &self.battery_saver,
            "hotspot" => &self.hotspot,
            "word_wrap" => &self.word_wrap,
            _ => return Err(format!("unknown system switch {name}")),
        })
    }
    fn flag_mut(&mut self, name: &str) -> Result<&mut bool, String> {
        Ok(match name {
            "wifi" => &mut self.wifi,
            "bluetooth" => &mut self.bluetooth,
            "airplane_mode" => &mut self.airplane_mode,
            "do_not_disturb" => &mut self.do_not_disturb,
            "night_light" => &mut self.night_light,
            "dark_mode" => &mut self.dark_mode,
            "rotation_lock" => &mut self.rotation_lock,
            "flashlight" => &mut self.flashlight,
            "battery_saver" => &mut self.battery_saver,
            "hotspot" => &mut self.hotspot,
            "word_wrap" => &mut self.word_wrap,
            _ => return Err(format!("unknown system switch {name}")),
        })
    }
    /// Sliders are drawn as discrete steps so a click lands on an exact level.
    pub fn set_level(&mut self, name: &str, percent: u8) -> Result<(), String> {
        let percent = percent.min(100);
        match name {
            "brightness" => self.brightness = percent,
            "volume" => self.volume = percent,
            _ => return Err(format!("unknown system level {name}")),
        }
        Ok(())
    }
    pub fn level(&self, name: &str) -> Result<u8, String> {
        match name {
            "brightness" => Ok(self.brightness),
            "volume" => Ok(self.volume),
            _ => Err(format!("unknown system level {name}")),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowSnap {
    Left,
    Right,
    Full,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerCapture {
    pub window: u64,
    pub operation: String,
    pub start_x: i32,
    pub start_y: i32,
    pub original: cw_scene::Rect,
    pub moved: bool,
}
impl DesktopState {
    pub fn ordered_windows(&self) -> Vec<u64> {
        let mut ids: Vec<_> = self
            .stacking
            .iter()
            .copied()
            .filter(|id| self.windows.contains_key(id))
            .collect();
        for id in self.windows.keys() {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        ids
    }
    pub fn effective_frame(&self, id: u64, area: cw_scene::Rect) -> cw_scene::Rect {
        let Some(window) = self.windows.get(&id) else {
            return area;
        };
        if window.maximized {
            return area;
        }
        if let Some(snap) = window.snapped {
            return snap_frame(snap, area);
        }
        let offset = (id % 7) as i32 * 28;
        clamp_frame(
            window.frame.unwrap_or(cw_scene::Rect::new(
                area.x + 56 + offset,
                area.y + 38 + offset,
                (area.width * 4 / 5).min(1000),
                (area.height * 4 / 5).min(680),
            )),
            area,
        )
    }
    pub fn maximize(&mut self, id: u64, area: cw_scene::Rect) -> Result<(), String> {
        let frame = self.effective_frame(id, area);
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        if window.maximized {
            window.frame = Some(clamp_frame(
                window.restored_frame.take().unwrap_or(frame),
                area,
            ));
            window.maximized = false;
            window.snapped = None;
        } else {
            if window.restored_frame.is_none() {
                window.restored_frame = Some(frame);
            }
            window.maximized = true;
            window.snapped = Some(WindowSnap::Full);
        }
        self.focus(id)
    }
    pub fn snap(&mut self, id: u64, snap: WindowSnap, area: cw_scene::Rect) -> Result<(), String> {
        let frame = self.effective_frame(id, area);
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        if window.restored_frame.is_none() {
            window.restored_frame = Some(frame);
        }
        window.snapped = Some(snap);
        window.maximized = snap == WindowSnap::Full;
        self.focus(id)
    }
    pub fn pointer_down(
        &mut self,
        id: u64,
        operation: &str,
        x: i32,
        y: i32,
        area: cw_scene::Rect,
    ) -> Result<(), String> {
        if operation != "drag"
            && !matches!(
                operation,
                "resize:n"
                    | "resize:ne"
                    | "resize:e"
                    | "resize:se"
                    | "resize:s"
                    | "resize:sw"
                    | "resize:w"
                    | "resize:nw"
            )
        {
            return Err("invalid window pointer operation".into());
        }
        let frame = self.effective_frame(id, area);
        self.focus(id)?;
        self.pointer_capture = Some(PointerCapture {
            window: id,
            operation: operation.into(),
            start_x: x,
            start_y: y,
            original: frame,
            moved: false,
        });
        Ok(())
    }
    pub fn pointer_move(&mut self, x: i32, y: i32, area: cw_scene::Rect) -> Result<bool, String> {
        let Some(mut capture) = self.pointer_capture.take() else {
            return Ok(false);
        };
        let dx = x.saturating_sub(capture.start_x);
        let dy = y.saturating_sub(capture.start_y);
        if !capture.moved && dx.unsigned_abs() < 3 && dy.unsigned_abs() < 3 {
            self.pointer_capture = Some(capture);
            return Ok(true);
        }
        let window = self
            .windows
            .get_mut(&capture.window)
            .ok_or("window not found")?;
        if !capture.moved
            && capture.operation == "drag"
            && (window.maximized || window.snapped.is_some())
        {
            let restored = window.restored_frame.take().unwrap_or(capture.original);
            let fraction =
                (capture.start_x - capture.original.x).clamp(0, capture.original.width as i32);
            capture.original = clamp_frame(
                cw_scene::Rect::new(
                    capture.start_x
                        - ((i64::from(fraction) * i64::from(restored.width))
                            / i64::from(capture.original.width.max(1)))
                            as i32,
                    capture.start_y - 16,
                    restored.width,
                    restored.height,
                ),
                area,
            );
        }
        capture.moved = true;
        window.maximized = false;
        window.snapped = None;
        window.restored_frame = None;
        let mut frame = capture.original;
        if capture.operation == "drag" {
            frame.x = frame.x.saturating_add(dx);
            frame.y = frame.y.saturating_add(dy);
        } else {
            let edge = capture.operation.trim_start_matches("resize:");
            let min_w = 320.min(area.width).min(frame.width) as i32;
            let min_h = 180.min(area.height).min(frame.height) as i32;
            let right = frame.x + frame.width as i32;
            let bottom = frame.y + frame.height as i32;
            if edge.contains('w') {
                frame.x = (frame.x + dx).clamp(area.x, right - min_w);
                frame.width = (right - frame.x) as u32;
            }
            if edge.contains('n') {
                frame.y = (frame.y + dy).clamp(area.y, bottom - min_h);
                frame.height = (bottom - frame.y) as u32;
            }
            if edge.contains('e') {
                frame.width = (frame.width as i32 + dx)
                    .clamp(min_w, (area.x + area.width as i32 - frame.x).max(min_w))
                    as u32;
            }
            if edge.contains('s') {
                frame.height = (frame.height as i32 + dy)
                    .clamp(min_h, (area.y + area.height as i32 - frame.y).max(min_h))
                    as u32;
            }
        }
        window.frame = Some(clamp_frame(frame, area));
        self.pointer_capture = Some(capture);
        Ok(true)
    }
    pub fn pointer_up(&mut self, x: i32, y: i32, area: cw_scene::Rect) -> Result<bool, String> {
        self.pointer_move(x, y, area)?;
        let Some(capture) = self.pointer_capture.take() else {
            return Ok(false);
        };
        if capture.moved && capture.operation == "drag" {
            let snap = if x <= area.x + 12 {
                Some(WindowSnap::Left)
            } else if x >= area.x + area.width as i32 - 12 {
                Some(WindowSnap::Right)
            } else if y <= area.y + 10 {
                Some(WindowSnap::Full)
            } else {
                None
            };
            if let Some(snap) = snap {
                self.snap(capture.window, snap, area)?;
            }
        }
        Ok(true)
    }
}
fn snap_frame(snap: WindowSnap, area: cw_scene::Rect) -> cw_scene::Rect {
    match snap {
        WindowSnap::Full => area,
        WindowSnap::Left => cw_scene::Rect::new(area.x, area.y, area.width / 2, area.height),
        WindowSnap::Right => cw_scene::Rect::new(
            area.x + (area.width / 2) as i32,
            area.y,
            area.width - area.width / 2,
            area.height,
        ),
    }
}
fn clamp_frame(frame: cw_scene::Rect, area: cw_scene::Rect) -> cw_scene::Rect {
    let width = frame.width.max(1).min(area.width.max(1));
    let height = frame.height.max(1).min(area.height.max(1));
    cw_scene::Rect::new(
        frame
            .x
            .clamp(area.x, area.x + area.width.saturating_sub(width) as i32),
        frame
            .y
            .clamp(area.y, area.y + area.height.saturating_sub(height) as i32),
        width,
        height,
    )
}

impl DesktopState {
    pub fn launch(&mut self, kind: &str, argument: &str) -> Result<(u64, Vec<AppEffect>), String> {
        let id = self.next_id;
        let (state, effects) = match kind {
            "terminal" => (
                AppState::Terminal {
                    input: String::new(),
                    prompt: self.prompt_line().to_owned(),
                    transcript: vec![],
                    history: vec![],
                    cursor: 0,
                    scroll: 0,
                },
                vec![],
            ),
            "editor" | "text_editor" => (
                AppState::Editor {
                    path: argument.into(),
                    text: String::new(),
                    cursor: 0,
                    dirty: false,
                },
                if argument.is_empty() {
                    vec![]
                } else {
                    vec![AppEffect::ReadFile {
                        window: id,
                        path: argument.into(),
                    }]
                },
            ),
            "files" | "file_manager" => {
                let path = if argument.is_empty() { "/" } else { argument };
                (
                    AppState::Files {
                        tabs: vec![FileTab::new(path)],
                        active: 0,
                    },
                    vec![AppEffect::ListDirectory {
                        window: id,
                        tab: 0,
                        path: path.into(),
                    }],
                )
            }
            "browser" => (
                AppState::Browser {
                    address: argument.into(),
                },
                if argument.is_empty() {
                    vec![]
                } else {
                    vec![AppEffect::Navigate {
                        window: id,
                        url: argument.into(),
                    }]
                },
            ),
            _ => {
                let clock = self.clock_us;
                let (mut app, mut effects) = NativeApp::launch(kind, argument, id, clock)
                    .ok_or_else(|| format!("unknown application: {kind}"))?;
                // Visual Studio Code keeps its settings under the user's home and opens
                // `~/project` when it is launched on nothing.
                if let NativeApp::Code(code) = &mut app {
                    let mut first = code.attach(&self.home_folder(), &self.trash_folder(), id);
                    first.append(&mut effects);
                    effects = first;
                }
                (AppState::Native(app), effects)
            }
        };
        self.next_id += 1;
        self.windows.insert(
            id,
            Window {
                id,
                title: kind.into(),
                app_id: kind.into(),
                state,
                minimized: false,
                frame: None,
                restored_frame: None,
                maximized: false,
                snapped: None,
                // A new window opens on the desktop the user is looking at.
                workspace: self.workspace,
            },
        );
        self.stacking.retain(|window| *window != id);
        self.stacking.push(id);
        self.focused = Some(id);
        self.launcher_open = false;
        self.panel = None;
        self.search.clear();
        Ok((id, effects))
    }
    pub fn focus(&mut self, id: u64) -> Result<(), String> {
        if !self.windows.contains_key(&id) {
            return Err("window not found".into());
        }
        self.windows.get_mut(&id).unwrap().minimized = false;
        self.stacking.retain(|window| *window != id);
        self.stacking.push(id);
        self.focused = Some(id);
        self.launcher_open = false;
        self.panel = None;
        self.search.clear();
        Ok(())
    }
    pub fn close(&mut self, id: u64) -> Result<(), String> {
        self.windows.remove(&id).ok_or("window not found")?;
        self.stacking.retain(|window| *window != id);
        if self
            .pointer_capture
            .as_ref()
            .is_some_and(|capture| capture.window == id)
        {
            self.pointer_capture = None;
        }
        if self.focused == Some(id) {
            self.focused = self
                .ordered_windows()
                .into_iter()
                .rev()
                .find(|id| self.windows.get(id).is_some_and(|w| !w.minimized));
        }
        Ok(())
    }
    pub fn minimize(&mut self, id: u64) -> Result<(), String> {
        if self
            .pointer_capture
            .as_ref()
            .is_some_and(|capture| capture.window == id)
        {
            self.pointer_capture = None;
        }
        self.windows
            .get_mut(&id)
            .ok_or("window not found")?
            .minimized = true;
        if self.focused == Some(id) {
            self.focused = self
                .ordered_windows()
                .into_iter()
                .rev()
                .find(|id| self.windows.get(id).is_some_and(|w| !w.minimized));
        }
        Ok(())
    }
    pub fn home(&mut self) {
        // Going home from the home screen itself returns to its first page, as the
        // gesture does on a phone; from an application it keeps the page it left.
        if self.focused.is_none() && !self.launcher_open && self.panel.is_none() {
            self.home_page = 0;
        }
        self.pointer_capture = None;
        for window in self.windows.values_mut() {
            window.minimized = true;
        }
        self.focused = None;
        self.launcher_open = false;
        self.panel = None;
        self.search.clear();
    }
    pub fn cycle(&mut self) -> Result<(), String> {
        let ids = self.ordered_windows();
        let target = ids
            .iter()
            .rev()
            .copied()
            .find(|id| Some(*id) != self.focused)
            .or_else(|| ids.last().copied());
        if let Some(target) = target {
            self.focus(target)?;
        }
        Ok(())
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        let window = self
            .focused
            .and_then(|id| self.windows.get_mut(&id))
            .ok_or("no focused window")?;
        match &mut window.state {
            AppState::Terminal { input, cursor, .. } => {
                if *cursor > input.len() || !input.is_char_boundary(*cursor) {
                    return Err("invalid terminal cursor".into());
                }
                input.insert_str(*cursor, text);
                *cursor += text.len();
            }
            AppState::Editor {
                text: content,
                cursor,
                dirty,
                ..
            } => {
                if *cursor > content.len() || !content.is_char_boundary(*cursor) {
                    return Err("invalid text cursor".into());
                }
                content.insert_str(*cursor, text);
                *cursor += text.len();
                *dirty = true;
            }
            AppState::Browser { address } => address.push_str(text),
            AppState::Files { tabs, active } => {
                let tab = tabs
                    .get_mut(*active)
                    .ok_or("tab not found")
                    .map_err(str::to_owned)?;
                let field = tab.field_mut().ok_or("file manager has no text focus")?;
                if field.chars().count() + text.chars().count() > FIELD_LIMIT {
                    return Err("file manager field is full".into());
                }
                field.push_str(text);
            }
            AppState::Native(app) => app.text(text)?,
        }
        Ok(())
    }
    pub fn key(&mut self, key: &str) -> Result<Vec<AppEffect>, String> {
        let id = self.focused.ok_or("no focused window")?;
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        if let AppState::Editor { text, cursor, .. } = &window.state {
            if *cursor > text.len() || !text.is_char_boundary(*cursor) {
                return Err("invalid text cursor".into());
            }
        }
        let mut effects = vec![];
        match &mut window.state {
            AppState::Terminal {
                input,
                history,
                cursor,
                scroll,
                ..
            } => {
                if *cursor > input.len() || !input.is_char_boundary(*cursor) {
                    return Err("invalid terminal cursor".into());
                }
                match key {
                    "Enter" => {
                        let command = std::mem::take(input);
                        *cursor = 0;
                        // Running something brings the view back to the tail: the answer
                        // must not appear off screen below a scrolled-up frame.
                        *scroll = 0;
                        history.push(command.clone());
                        effects.push(AppEffect::Execute {
                            window: id,
                            command,
                        });
                    }
                    "Backspace" => {
                        if *cursor > 0 {
                            let at = input[..*cursor]
                                .char_indices()
                                .next_back()
                                .map(|(i, _)| i)
                                .unwrap_or(0);
                            input.drain(at..*cursor);
                            *cursor = at;
                        }
                    }
                    "Delete" => {
                        if let Some(ch) = input[*cursor..].chars().next() {
                            let end = *cursor + ch.len_utf8();
                            input.drain(*cursor..end);
                        }
                    }
                    "ArrowLeft" => {
                        *cursor = input[..*cursor]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                    }
                    "ArrowRight" => {
                        if let Some(ch) = input[*cursor..].chars().next() {
                            *cursor += ch.len_utf8();
                        }
                    }
                    "Home" => *cursor = 0,
                    "End" => *cursor = input.len(),
                    _ => return Err(format!("unsupported terminal key {key}")),
                }
            }
            AppState::Editor {
                path,
                text,
                cursor,
                dirty,
            } => match key {
                "Ctrl+s" | "Meta+s" => {
                    if path.is_empty() {
                        return Err("editor has no save path".into());
                    }
                    effects.push(AppEffect::WriteFile {
                        window: id,
                        path: path.clone(),
                        content: text.clone(),
                    });
                }
                "Backspace" => {
                    if *cursor > 0 {
                        let p = text[..*cursor]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        text.drain(p..*cursor);
                        *cursor = p;
                        *dirty = true;
                    }
                }
                "Delete" => {
                    if *cursor < text.len() {
                        let end = *cursor + text[*cursor..].chars().next().unwrap().len_utf8();
                        text.drain(*cursor..end);
                        *dirty = true;
                    }
                }
                "ArrowLeft" => {
                    *cursor = text[..*cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                "ArrowRight" => {
                    if let Some(ch) = text[*cursor..].chars().next() {
                        *cursor += ch.len_utf8();
                    }
                }
                // Up and Down keep the column, clamped to a shorter line's end.
                "ArrowUp" | "ArrowDown" => {
                    let start = text[..*cursor].rfind('\n').map_or(0, |i| i + 1);
                    let column = text[start..*cursor].chars().count();
                    let target = if key == "ArrowUp" {
                        (start > 0).then(|| text[..start - 1].rfind('\n').map_or(0, |i| i + 1))
                    } else {
                        text[*cursor..].find('\n').map(|i| *cursor + i + 1)
                    };
                    if let Some(line) = target {
                        let stop = text[line..].find('\n').map_or(text.len(), |i| line + i);
                        *cursor = text[line..stop]
                            .char_indices()
                            .nth(column)
                            .map_or(stop, |(i, _)| line + i);
                    }
                }
                "Home" => {
                    *cursor = text[..*cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
                }
                "End" => {
                    *cursor += text[*cursor..].find('\n').unwrap_or(text.len() - *cursor);
                }
                "Enter" => {
                    text.insert(*cursor, '\n');
                    *cursor += 1;
                    *dirty = true;
                }
                _ => return Err(format!("unsupported editor key {key}")),
            },
            AppState::Browser { address } => match key {
                "Enter" => effects.push(AppEffect::Navigate {
                    window: id,
                    url: address.clone(),
                }),
                "Backspace" => {
                    address.pop();
                }
                _ => return Err(format!("unsupported address key {key}")),
            },
            // Only the open field takes keys; a file manager has no general text focus.
            AppState::Files { tabs, active } => {
                let tab = tabs
                    .get_mut(*active)
                    .ok_or("tab not found")
                    .map_err(str::to_owned)?;
                match key {
                    "Escape" => {
                        // Cancelling a search clears the filter: a hidden filter and an
                        // empty folder look the same, and one of them is a lie.
                        if tab.rename.is_none() {
                            tab.query.clear();
                        }
                        tab.stop_editing();
                    }
                    "Backspace" => {
                        tab.field_mut()
                            .ok_or("file manager has no text focus")?
                            .pop();
                    }
                    "Enter" if tab.rename.is_some() => return self.commit_rename(),
                    "Enter" if tab.searching => tab.searching = false,
                    _ => return Err(format!("unsupported file manager key {key}")),
                }
            }
            AppState::Native(app) => {
                let clock = self.clock_us;
                let more = app.key(id, key, clock)?;
                return self.native_effects(more);
            }
        }
        Ok(effects)
    }
    /// Apply the effects an application asks of the desktop itself — the clipboard —
    /// and hand the rest on to the environment.
    fn native_effects(&mut self, effects: Vec<AppEffect>) -> Result<Vec<AppEffect>, String> {
        let mut out = Vec::with_capacity(effects.len());
        for effect in effects {
            match effect {
                AppEffect::CopyText { text, .. } => self.copy_text(&text)?,
                AppEffect::Paste { window } => {
                    // Pasting an empty clipboard pastes nothing, as it does everywhere.
                    let Some(text) = self.clipboard_text.clone() else {
                        continue;
                    };
                    let app = match self.windows.get_mut(&window).map(|w| &mut w.state) {
                        Some(AppState::Native(app)) => app,
                        _ => return Err("window is not a native application".into()),
                    };
                    out.extend(app.paste(window, &text)?);
                }
                other => out.push(other),
            }
        }
        Ok(out)
    }
    /// Put text on the machine's clipboard.
    pub fn copy_text(&mut self, text: &str) -> Result<(), String> {
        if text.len() > TEXT_CLIPBOARD_LIMIT {
            return Err("the selection is too large to copy".into());
        }
        self.clipboard_text = Some(text.to_owned());
        Ok(())
    }
    /// Typed text, and whatever the focused application needs done because of it.
    pub fn type_text(&mut self, text: &str) -> Result<Vec<AppEffect>, String> {
        let id = self.focused.ok_or("no focused window")?;
        if let Some(AppState::Native(app)) = self.windows.get_mut(&id).map(|w| &mut w.state) {
            let effects = app.text_effects(id, text)?;
            return self.native_effects(effects);
        }
        self.text(text).map(|()| vec![])
    }
    /// A pointer pressed on a control of the focused window, before it is released.
    pub fn press_at(&mut self, target: &str, dx: i32, dy: i32) -> Result<(), String> {
        let id = self.focused.ok_or("no focused window")?;
        match self.windows.get_mut(&id).map(|w| &mut w.state) {
            Some(AppState::Native(app)) => app.press_at(target, dx, dy),
            _ => Ok(()),
        }
    }
    fn code_mut(&mut self, id: u64) -> Result<&mut apps::code::Code, String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(NativeApp::Code(code)) => Ok(code),
            _ => Err("window is not Visual Studio Code".into()),
        }
    }
    /// A folder tree an application asked for arrived, or could not be listed.
    pub fn tree_listed(
        &mut self,
        id: u64,
        path: &str,
        depth: u32,
        result: Result<Vec<String>, String>,
    ) -> Result<Vec<AppEffect>, String> {
        Ok(self.code_mut(id)?.tree_listed(id, path, depth, result))
    }
    /// Files an application asked to read, each with its content or why it failed.
    pub fn files_read(
        &mut self,
        id: u64,
        tag: &str,
        files: Vec<(String, Result<String, String>)>,
    ) -> Result<Vec<AppEffect>, String> {
        Ok(self.code_mut(id)?.files_read(id, tag, files))
    }
    /// A shell session command an application ran finished.
    pub fn shell_ran(
        &mut self,
        id: u64,
        tag: &str,
        outcome: ShellOutcome,
    ) -> Result<Vec<AppEffect>, String> {
        Ok(self.code_mut(id)?.shell_ran(id, tag, outcome))
    }
    /// A write reached the disk. Editors learn which file, so the right one turns clean.
    pub fn file_written(
        &mut self,
        id: u64,
        path: &str,
        content: &str,
    ) -> Result<Vec<AppEffect>, String> {
        if let Ok(code) = self.code_mut(id) {
            return Ok(code.written(id, path, content));
        }
        self.file_saved(id, content).map(|()| vec![])
    }
    /// Deliver successful effect results. A failed save must not mark an editor clean.
    pub fn file_loaded(&mut self, id: u64, content: String) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(NativeApp::Notes(notes)) => {
                notes.loaded(content);
                Ok(())
            }
            AppState::Editor {
                text,
                cursor,
                dirty,
                ..
            } => {
                *text = content;
                *cursor = text.len();
                *dirty = false;
                Ok(())
            }
            _ => Err("window is not an editor".into()),
        }
    }
    pub fn file_saved(&mut self, id: u64, saved_content: &str) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Editor { text, dirty, .. } => {
                if text == saved_content {
                    *dirty = false;
                }
                Ok(())
            }
            AppState::Native(NativeApp::Notes(_) | NativeApp::Code(_)) => Ok(()),
            _ => Err("window is not an editor".into()),
        }
    }
    /// Deliver decoded pixels for a file the application asked to see.
    pub fn image_loaded(
        &mut self,
        id: u64,
        path: &str,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => app.image(path, width, height, rgba),
            _ => Err("window is not a native application".into()),
        }
    }
    /// The file could not be decoded. The application says so rather than showing a gap.
    pub fn image_failed(&mut self, id: u64, path: &str, reason: &str) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => {
                app.image_failed(path, reason);
                Ok(())
            }
            _ => Err("window is not a native application".into()),
        }
    }
    /// Deliver an application HTTP reply. Returns follow-up effects, so a successful
    /// mutation can refetch without the shell knowing what the application wanted.
    pub fn http_response(
        &mut self,
        id: u64,
        tag: &str,
        status: u16,
        body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => app.http(id, tag, status, body),
            _ => Err("window is not a native application".into()),
        }
    }
    /// A folder could not be listed. Native applications show that in place of content;
    /// the file manager treats it as a real failure, so `false` asks the caller to raise it.
    pub fn directory_failed(&mut self, id: u64, reason: &str) -> Result<bool, String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => {
                app.offline("listing", reason);
                Ok(true)
            }
            _ => Ok(false),
        }
    }
    /// The request never reached a service. The application shows its own offline state.
    pub fn http_failed(&mut self, id: u64, tag: &str, reason: &str) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => {
                app.offline(tag, reason);
                Ok(())
            }
            _ => Err("window is not a native application".into()),
        }
    }
    /// Deliver a finished command to the terminal that asked for it.
    pub fn terminal_output(&mut self, id: u64, entry: TerminalEntry) -> Result<(), String> {
        self.deliver_to_terminal(id, Some(entry))
    }
    /// `clear` wipes the frame only: the cwd lives on the machine and recall history is
    /// a separate field, so neither is touched.
    pub fn terminal_clear(&mut self, id: u64) -> Result<(), String> {
        self.deliver_to_terminal(id, None)
    }
    /// `None` clears the frame. Every terminal's pending prompt is refreshed, not just
    /// this one: the cwd belongs to the machine, so a stale prompt elsewhere would lie.
    fn deliver_to_terminal(&mut self, id: u64, entry: Option<TerminalEntry>) -> Result<(), String> {
        match self.windows.get(&id).map(|w| &w.state) {
            Some(AppState::Terminal { .. }) => {}
            Some(_) => return Err("window is not a terminal".into()),
            None => return Err("window not found".into()),
        }
        let next = self.prompt_line().to_owned();
        for window in self.windows.values_mut() {
            let target = window.id == id;
            if let AppState::Terminal {
                transcript,
                prompt,
                scroll,
                ..
            } = &mut window.state
            {
                prompt.clone_from(&next);
                if !target {
                    continue;
                }
                // New output pins the view to the tail, so what just ran is on screen.
                *scroll = 0;
                match &entry {
                    Some(entry) => {
                        transcript.push(entry.clone());
                        let over = transcript.len().saturating_sub(TRANSCRIPT_LIMIT);
                        transcript.drain(..over);
                    }
                    None => transcript.clear(),
                }
            }
        }
        Ok(())
    }
    pub fn directory_loaded(
        &mut self,
        id: u64,
        tab: usize,
        mut values: Vec<String>,
    ) -> Result<(), String> {
        values.sort();
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Files { tabs, .. } => {
                let tab = tabs.get_mut(tab).ok_or("tab not found")?;
                // A listing that arrives while the tab has moved to Recents belongs to
                // the folder it left; dropping it beats overwriting what is on screen.
                if tab.scope != FileScope::Folder {
                    return Ok(());
                }
                tab.entries = values;
                tab.selected = None;
                // The entry being renamed may not have survived the refresh.
                tab.rename = None;
                Ok(())
            }
            AppState::Native(NativeApp::Notes(notes)) => {
                notes.listed(values);
                Ok(())
            }
            AppState::Native(NativeApp::Photos(photos)) => {
                photos.listed(values);
                Ok(())
            }
            _ => Err("window is not a file manager".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn soft_wrapped_rows_break_after_a_space_and_clicks_land_on_the_row_painted() {
        let text = "the quick brown fox jumps\nshort";
        let rows = editor_rows(text, 10);
        let painted: Vec<&str> = rows.iter().map(|(a, b)| &text[*a..*b]).collect();
        assert_eq!(painted, ["the quick ", "brown fox ", "jumps", "short"]);
        // A word longer than the row breaks at the edge.
        let long = editor_rows("abcdefghijkl", 5);
        assert_eq!(long, vec![(0, 5), (5, 10), (10, 12)]);
        // Unwrapped, a row is a line.
        assert_eq!(editor_rows(text, 0).len(), 2);
        // Row 1, column 2 is the "o" of "brown".
        let at = caret_for_point_wrapped(text, 0, 10, 2 * 8, 18);
        assert_eq!(&text[at..at + 1], "o");
        // The same point with a scrolled view one row down is on row 2.
        let at = caret_for_point_wrapped(text, 1, 10, 0, 18);
        assert_eq!(&text[at..], "jumps\nshort");
        // The caret at a soft wrap sits at the start of the next row.
        assert_eq!(editor_caret_cell(text, 10, 10), (1, 0));
        // At the end of a hard line it stays on that line.
        assert_eq!(editor_caret_cell(text, 25, 10), (2, 5));
        assert_eq!(editor_caret_cell(text, text.len(), 10), (3, 5));
        // Clicking past the end of a soft-wrapped row stays on that row.
        let at = caret_for_point_wrapped(text, 0, 10, 40 * 8, 0);
        assert_eq!(editor_caret_cell(text, at, 10).0, 0);
    }
    #[test]
    fn up_and_down_keep_the_column() {
        let mut d = DesktopState::default();
        let (window, _) = d.launch("editor", "").unwrap();
        d.focus(window).unwrap();
        d.text("abcdef\nxy\nlonger line").unwrap();
        d.key("ArrowUp").unwrap();
        let cursor = |d: &DesktopState| match &d.windows[&window].state {
            AppState::Editor { cursor, .. } => *cursor,
            _ => unreachable!(),
        };
        // From column 11 of line 3 to the end of the two-character line 2.
        assert_eq!(cursor(&d), "abcdef\nxy".len());
        d.key("ArrowUp").unwrap();
        assert_eq!(cursor(&d), 2);
        d.key("ArrowDown").unwrap();
        assert_eq!(cursor(&d), "abcdef\nxy".len());
    }
    #[test]
    fn unicode_editor_effects_and_focus() {
        let mut d = DesktopState::default();
        let (editor, _) = d.launch("editor", "/note").unwrap();
        d.file_loaded(editor, "café".into()).unwrap();
        d.key("Backspace").unwrap();
        d.text("ø").unwrap();
        assert_eq!(
            d.key("Ctrl+s").unwrap(),
            vec![AppEffect::WriteFile {
                window: editor,
                path: "/note".into(),
                content: "cafø".into()
            }]
        );
        let (terminal, _) = d.launch("terminal", "").unwrap();
        d.text("pwd").unwrap();
        assert_eq!(
            d.key("Enter").unwrap(),
            vec![AppEffect::Execute {
                window: terminal,
                command: "pwd".into()
            }]
        );
        d.close(terminal).unwrap();
        assert_eq!(d.focused, Some(editor));
    }
    #[test]
    fn terminal_transcript_is_legible_bounded_and_clearable() {
        assert_eq!(
            shell_prompt("alice", "workstation", "/home/alice", "posix"),
            "alice@workstation:/home/alice$"
        );
        assert_eq!(
            shell_prompt("alice", "win", "/C:/Users/alice", "powershell"),
            r"PS C:\Users\alice>"
        );
        let mut d = DesktopState {
            prompt: "alice@box:/home/alice$".into(),
            ..DesktopState::default()
        };
        let (id, _) = d.launch("terminal", "").unwrap();
        d.terminal_output(
            id,
            TerminalEntry::new("alice@box:/home/alice$", "cd /tmp", "", "", 0),
        )
        .unwrap();
        // The machine moved, so the pending prompt moves with it.
        d.prompt = "alice@box:/tmp$".into();
        d.terminal_output(
            id,
            TerminalEntry::new("alice@box:/tmp$", "nope", "", "nope: not found", 127),
        )
        .unwrap();
        let page = d.page();
        let ids: Vec<&str> = page.elements.iter().map(element_id).collect();
        assert!(ids.contains(&"terminal-entry:1"));
        assert!(matches!(
            page.elements.iter().find(|e| element_id(e) == "terminal-entry:1"),
            Some(cw_protocol::PageElement::Group { children, .. })
                if children.iter().any(|c| matches!(c, cw_protocol::PageElement::Text { text, .. } if text == "[exit 127]"))
        ));
        assert!(matches!(
            page.elements.last(),
            Some(cw_protocol::PageElement::Input { label, .. }) if label == "alice@box:/tmp$"
        ));
        // Recall history survives a clear; only the frame goes.
        d.text("echo hi").unwrap();
        d.key("Enter").unwrap();
        d.terminal_clear(id).unwrap();
        match &d.windows[&id].state {
            AppState::Terminal {
                transcript,
                history,
                ..
            } => {
                assert!(transcript.is_empty());
                assert_eq!(history, &vec!["echo hi".to_string()]);
            }
            _ => panic!("not a terminal"),
        }
        for i in 0..TRANSCRIPT_LIMIT + 10 {
            d.terminal_output(id, TerminalEntry::new("$", format!("echo {i}"), "", "", 0))
                .unwrap();
        }
        match &d.windows[&id].state {
            AppState::Terminal { transcript, .. } => {
                assert_eq!(transcript.len(), TRANSCRIPT_LIMIT);
                assert_eq!(transcript[0].command, "echo 10");
                // Oversized streams are cut with a stated byte count, never silently.
                let long = TerminalEntry::new("$", "yes", &"x".repeat(STREAM_LIMIT + 100), "", 0);
                assert!(long.stdout.ends_with("[… 100 bytes truncated]"));
            }
            _ => panic!("not a terminal"),
        }
    }
    fn element_id(e: &cw_protocol::PageElement) -> &str {
        use cw_protocol::PageElement as E;
        match e {
            E::Text { id, .. }
            | E::Group { id, .. }
            | E::Input { id, .. }
            | E::Button { id, .. }
            | E::Heading { id, .. } => id,
            _ => "",
        }
    }
    #[test]
    fn snapshot_restores_cursor_and_pending_edit() {
        let mut d = DesktopState::default();
        d.launch("editor", "/note").unwrap();
        d.text("😀hello").unwrap();
        d.key("Home").unwrap();
        d.key("Delete").unwrap();
        let restored: DesktopState =
            serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(d, restored);
    }
}

impl DesktopState {
    /// Pure semantic projection; effects are completed separately by the caller.
    pub fn page(&self) -> cw_protocol::Page {
        use cw_protocol::PageElement as E;
        let mut page = cw_protocol::Page::new("Desktop");
        for window in self.windows.values() {
            page.elements.push(E::Button {
                id: format!("focus:{}", window.id),
                text: format!(
                    "{}{}",
                    if self.focused == Some(window.id) {
                        "● "
                    } else {
                        ""
                    },
                    window.title
                ),
                action: cw_protocol::PageAction {
                    method: "APP".into(),
                    url: format!("focus:{}", window.id),
                    fields: BTreeMap::new(),
                },
            });
        }
        if let Some(window) = self.focused.and_then(|id| self.windows.get(&id)) {
            match &window.state {
                AppState::Terminal {
                    input,
                    prompt,
                    transcript,
                    ..
                } => {
                    // One group per command, so a consumer reads boundaries and status as
                    // fields instead of parsing a screen or matching error text.
                    for (index, entry) in transcript.iter().enumerate() {
                        let mut children = vec![E::Text {
                            id: format!("terminal-entry:{index}:command"),
                            text: entry.echo(),
                        }];
                        if !entry.stdout.is_empty() {
                            children.push(E::Text {
                                id: format!("terminal-entry:{index}:stdout"),
                                text: entry.stdout.clone(),
                            });
                        }
                        if !entry.stderr.is_empty() {
                            children.push(E::Text {
                                id: format!("terminal-entry:{index}:stderr"),
                                text: entry.stderr.clone(),
                            });
                        }
                        // Always emitted, unlike the screen marker: a reader should never
                        // have to infer success from the absence of a line.
                        children.push(E::Text {
                            id: format!("terminal-entry:{index}:exit"),
                            text: entry.status(),
                        });
                        page.elements.push(E::Group {
                            id: format!("terminal-entry:{index}"),
                            children,
                        });
                    }
                    page.elements.push(E::Input {
                        id: "terminal-input".into(),
                        label: prompt_or_sigil(prompt).into(),
                        value: input.clone(),
                        placeholder: String::new(),
                    });
                }
                AppState::Editor {
                    path, text, dirty, ..
                } => {
                    page.elements.push(E::Heading {
                        id: "editor-path".into(),
                        text: format!("{path}{}", if *dirty { " *" } else { "" }),
                        level: 2,
                    });
                    page.elements.push(E::Input {
                        id: "editor-text".into(),
                        label: "Document".into(),
                        value: text.clone(),
                        placeholder: String::new(),
                    });
                }
                AppState::Files { tabs, active } => {
                    let action = |url: &str| cw_protocol::PageAction {
                        method: "APP".into(),
                        url: url.into(),
                        fields: BTreeMap::new(),
                    };
                    for (index, tab) in tabs.iter().enumerate() {
                        page.elements.push(E::Button {
                            id: format!("files-tab:{index}"),
                            text: format!(
                                "{}{}",
                                if index == *active { "\u{25cf} " } else { "" },
                                tab.name()
                            ),
                            action: action(&format!("files-tab:{index}")),
                        });
                    }
                    page.elements.push(E::Button {
                        id: "files-newtab".into(),
                        text: "New tab".into(),
                        action: action("files-newtab"),
                    });
                    let Some(tab) = tabs.get(*active) else {
                        return page;
                    };
                    page.elements.push(E::Heading {
                        id: "files-path".into(),
                        text: tab.path.clone(),
                        level: 2,
                    });
                    for (id, text, enabled) in [
                        ("files-back", "Back", tab.can_go_back()),
                        ("files-forward", "Forward", tab.can_go_forward()),
                        ("files-up", "Up", !tab.path.trim_matches('/').is_empty()),
                        ("files-reload", "Reload", true),
                        ("files-view", "View", true),
                        ("files-sort:name", "Sort by name", true),
                        ("files-sort:kind", "Sort by kind", true),
                    ] {
                        if enabled {
                            page.elements.push(E::Button {
                                id: id.into(),
                                text: text.into(),
                                action: action(id),
                            });
                        }
                    }
                    page.elements.push(E::Input {
                        id: "files-search".into(),
                        label: "Search this folder".into(),
                        value: tab.query.clone(),
                        placeholder: String::new(),
                    });
                    // Screen order, so `open:<i>` means the same thing here as it does
                    // to a pointer: the row a reader counts is the row a click hits.
                    for (row, index) in tab.display().into_iter().enumerate() {
                        let entry = &tab.entries[index];
                        page.elements.push(E::Button {
                            id: format!("open:{row}"),
                            text: format!(
                                "{}{entry}",
                                if tab.selected == Some(index) {
                                    "\u{25cf} "
                                } else {
                                    ""
                                }
                            ),
                            action: action(entry),
                        });
                    }
                }
                AppState::Browser { address } => page.elements.push(E::Input {
                    id: "browser-address".into(),
                    label: "Address".into(),
                    value: address.clone(),
                    placeholder: "https://".into(),
                }),
                AppState::Native(app) => app.page(&mut page),
            }
        }
        page
    }
    /// Prompt for the line being typed. Falls back to a bare sigil only before the
    /// machine has said who and where we are.
    pub fn prompt_line(&self) -> &str {
        prompt_or_sigil(&self.prompt)
    }
    /// Windows on the desktop currently on screen. Another workspace's windows are not
    /// minimised, they are simply elsewhere.
    pub fn on_this_workspace(&self, id: u64) -> bool {
        self.windows
            .get(&id)
            .is_some_and(|w| w.workspace == self.workspace)
    }
    pub fn workspace_count(&self) -> u32 {
        self.workspaces.max(1)
    }
    /// Add a desktop and move to it. Returns the new index.
    /// Panels that are drop-down menus rather than flyouts: choosing an entry in one
    /// closes it, as a menu does.
    pub const MENUS: &'static [&'static str] = &["file", "edit", "view", "format", "app-menu"];
    pub fn close_menu(&mut self) {
        if self
            .panel
            .as_deref()
            .is_some_and(|panel| Self::MENUS.contains(&panel))
        {
            self.panel = None;
        }
    }
    pub fn add_workspace(&mut self) -> Result<u32, String> {
        if self.workspace_count() >= WORKSPACE_LIMIT {
            return Err(format!("at most {WORKSPACE_LIMIT} desktops"));
        }
        self.workspaces = self.workspace_count() + 1;
        self.workspace = self.workspaces - 1;
        self.focused = None;
        Ok(self.workspace)
    }
    pub fn switch_workspace(&mut self, index: u32) -> Result<(), String> {
        if index >= self.workspace_count() {
            return Err("no such desktop".into());
        }
        self.workspace = index;
        // Focus follows the screen: a window you cannot see cannot be focused.
        self.focused = self
            .ordered_windows()
            .into_iter()
            .rev()
            .find(|id| self.on_this_workspace(*id) && !self.windows[id].minimized);
        Ok(())
    }
    /// Remove a desktop, moving its windows to the one before it so nothing is lost.
    pub fn close_workspace(&mut self, index: u32) -> Result<(), String> {
        if self.workspace_count() <= 1 {
            return Err("the last desktop cannot be closed".into());
        }
        if index >= self.workspace_count() {
            return Err("no such desktop".into());
        }
        for window in self.windows.values_mut() {
            if window.workspace == index {
                window.workspace = index.saturating_sub(1);
            } else if window.workspace > index {
                window.workspace -= 1;
            }
        }
        self.workspaces = self.workspace_count() - 1;
        self.switch_workspace(self.workspace.min(self.workspaces - 1))
    }
    /// Move a window to another desktop and follow it there.
    pub fn move_to_workspace(&mut self, id: u64, index: u32) -> Result<(), String> {
        if index >= self.workspace_count() {
            return Err("no such desktop".into());
        }
        self.windows
            .get_mut(&id)
            .ok_or("window not found")?
            .workspace = index;
        self.switch_workspace(index)
    }
    /// Save the page a browser window is showing. Saving the same URL twice is one
    /// bookmark, as every browser does.
    pub fn bookmark(&mut self, title: &str, url: &str) -> Result<(), String> {
        if url.is_empty() {
            return Err("an empty page cannot be saved".into());
        }
        if let Some(existing) = self.bookmarks.iter_mut().find(|b| b.url == url) {
            existing.title = title.into();
            return Ok(());
        }
        if self.bookmarks.len() >= BOOKMARK_LIMIT {
            return Err(format!("at most {BOOKMARK_LIMIT} bookmarks"));
        }
        self.bookmarks.push(Bookmark {
            title: title.into(),
            url: url.into(),
        });
        Ok(())
    }
    pub fn bookmarked(&self, url: &str) -> bool {
        self.bookmarks.iter().any(|b| b.url == url)
    }
    pub fn remove_bookmark(&mut self, url: &str) -> Result<(), String> {
        let before = self.bookmarks.len();
        self.bookmarks.retain(|b| b.url != url);
        if self.bookmarks.len() == before {
            return Err("no such bookmark".into());
        }
        Ok(())
    }
    /// Record a file the browser really wrote to the machine.
    pub fn record_download(&mut self, name: &str, path: &str, url: &str, bytes: u64) {
        self.downloads.retain(|d| d.path != path);
        self.downloads.insert(
            0,
            Download {
                name: name.into(),
                path: path.into(),
                url: url.into(),
                bytes,
            },
        );
        self.downloads.truncate(DOWNLOAD_LIMIT);
    }
    /// Post a notice. Applications call this when they *see* something; the shell that
    /// draws the shade never invents one.
    pub fn notify(&mut self, app: &str, title: &str, body: &str, action: Option<String>) {
        let time_us = self.clock_us;
        self.notifications
            .retain(|n| n.title != title || n.app != app);
        self.notifications.insert(
            0,
            Notice {
                app: app.into(),
                title: title.into(),
                body: body.into(),
                time_us,
                action,
                seen: false,
            },
        );
        self.notifications.truncate(NOTICE_LIMIT);
    }
    pub fn unseen_notices(&self) -> usize {
        self.notifications.iter().filter(|n| !n.seen).count()
    }
    pub fn mark_notices_seen(&mut self) {
        for notice in &mut self.notifications {
            notice.seen = true;
        }
    }
    pub fn home_folder(&self) -> String {
        if self.home.is_empty() {
            "/".into()
        } else {
            normalize_folder(&self.home)
        }
    }
    /// Active folder view of the focused window.
    fn focused_files(&mut self) -> Result<(u64, &mut Vec<FileTab>, &mut usize), String> {
        let id = self.focused.ok_or("no focused window")?;
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        match &mut window.state {
            AppState::Files { tabs, active } => Ok((id, tabs, active)),
            _ => Err("not a file manager".into()),
        }
    }
    fn focused_tab(&self) -> Result<&FileTab, String> {
        self.focused
            .and_then(|id| self.windows.get(&id))
            .ok_or("no focused window")?
            .state
            .file_tab()
            .ok_or_else(|| "not a file manager".into())
    }
    fn focused_tab_mut(&mut self) -> Result<&mut FileTab, String> {
        let id = self.focused.ok_or("no focused window")?;
        self.windows
            .get_mut(&id)
            .ok_or("window not found")?
            .state
            .file_tab_mut()
            .ok_or_else(|| "not a file manager".into())
    }
    /// Where this machine files what a file manager deletes. Deleting is a move here,
    /// never a hard remove, so a snapshot still holds what was thrown away.
    pub fn trash_folder(&self) -> String {
        format!(
            "{}/.local/share/Trash/files",
            self.home_folder().trim_end_matches('/')
        )
    }
    /// Remember a document that was opened, newest first and without duplicates.
    fn remember(&mut self, path: &str) {
        self.recents.retain(|p| p != path);
        self.recents.insert(0, path.to_owned());
        self.recents.truncate(RECENT_LIMIT);
    }
    /// Paste the clipboard into the active folder. A copy that would collide is given a
    /// `(copy)` name from the listing the tab already has; a move that would collide is
    /// refused, because a move that renames itself is a move you did not ask for.
    fn paste(&mut self) -> Result<Vec<AppEffect>, String> {
        let Some(clipboard) = self.clipboard.clone() else {
            return Err("the clipboard is empty".into());
        };
        let (id, tabs, active) = self.focused_files()?;
        let index = *active;
        let tab = tabs.get_mut(index).ok_or("tab not found")?;
        if tab.scope != FileScope::Folder {
            return Err("the Recents list is not a folder".into());
        }
        let folder = tab.path.clone();
        let mut effects = vec![];
        for from in &clipboard.paths {
            let name = entry_name(from.rsplit('/').next().unwrap_or_default());
            if name.is_empty() {
                return Err("clipboard entry has no name".into());
            }
            let (stem, extension) = match name.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
                _ => (name, String::new()),
            };
            let target = if clipboard.cut {
                if tab.entries.iter().any(|e| entry_name(e) == name) {
                    return Err("a file of that name is already here".into());
                }
                tab.child(name)
            } else {
                tab.child(&free_name(
                    &tab.entries,
                    &format!("{stem} (copy)"),
                    &extension,
                    " ",
                ))
            };
            if &target == from {
                return Err("that is already here".into());
            }
            effects.push(if clipboard.cut {
                AppEffect::MovePath {
                    window: id,
                    from: from.clone(),
                    to: target,
                }
            } else {
                AppEffect::CopyPath {
                    window: id,
                    from: from.clone(),
                    to: target,
                }
            });
        }
        // A cut is consumed by its paste; a copy stays on the clipboard.
        if clipboard.cut {
            self.clipboard = None;
        }
        effects.push(AppEffect::ListDirectory {
            window: id,
            tab: index,
            path: folder,
        });
        Ok(effects)
    }
    /// Commit the name being typed over the entry the rename started from.
    fn commit_rename(&mut self) -> Result<Vec<AppEffect>, String> {
        let (id, tabs, active) = self.focused_files()?;
        let index = *active;
        let tab = tabs.get_mut(index).ok_or("tab not found")?;
        let rename = tab.rename.take().ok_or("nothing is being renamed")?;
        let name = rename.name.trim();
        if name.is_empty()
            || name.contains(['/', '\\'])
            || name == "."
            || name == ".."
            || name.chars().count() > FIELD_LIMIT
        {
            return Err("that is not a usable file name".into());
        }
        if name == rename.from {
            return Ok(vec![]);
        }
        if tab.entries.iter().any(|e| entry_name(e) == name) {
            return Err("a file of that name is already here".into());
        }
        let (from, to) = (tab.child(&rename.from), tab.child(name));
        let path = tab.path.clone();
        Ok(vec![
            AppEffect::MovePath {
                window: id,
                from,
                to,
            },
            AppEffect::ListDirectory {
                window: id,
                tab: index,
                path,
            },
        ])
    }
    /// Show `path` in the active tab. `record` appends to the tab's history; the
    /// Back and Forward commands move within it instead.
    fn show_folder(&mut self, path: &str, record: bool) -> Result<Vec<AppEffect>, String> {
        let path = normalize_folder(path);
        let (id, tabs, active) = self.focused_files()?;
        let index = *active;
        let tab = tabs.get_mut(index).ok_or("tab not found")?;
        tab.path = path.clone();
        tab.entries.clear();
        tab.selected = None;
        // Moving folders leaves Recents and every text field: a filter typed for one
        // folder must not silently hide the contents of the next.
        tab.scope = FileScope::Folder;
        tab.query.clear();
        tab.stop_editing();
        if record {
            tab.history.truncate(tab.position + 1);
            if tab.history.last() != Some(&path) {
                tab.history.push(path.clone());
                tab.position = tab.history.len() - 1;
            }
            // Bound retained history so a long session cannot grow a snapshot without limit.
            if tab.history.len() > HISTORY_LIMIT {
                let excess = tab.history.len() - HISTORY_LIMIT;
                tab.history.drain(..excess);
                tab.position = tab.position.saturating_sub(excess);
            }
        }
        Ok(vec![AppEffect::ListDirectory {
            window: id,
            tab: index,
            path,
        }])
    }
    /// File manager commands. Every one of these is reachable from a real control.
    fn files_command(&mut self, command: &str) -> Result<Vec<AppEffect>, String> {
        match command {
            "up" | "parent" => {
                let path = self.focused_tab()?.path.clone();
                self.show_folder(parent_folder(&path), true)
            }
            "root" => self.show_folder("/", true),
            "home" => {
                let home = self.home_folder();
                self.show_folder(&home, true)
            }
            "reload" => {
                let path = self.focused_tab()?.path.clone();
                self.show_folder(&path, false)
            }
            "back" | "forward" => {
                let (id, tabs, active) = self.focused_files()?;
                let index = *active;
                let tab = tabs.get_mut(index).ok_or("tab not found")?;
                let position = if command == "back" {
                    tab.position.checked_sub(1).ok_or("no earlier folder")?
                } else {
                    let next = tab.position + 1;
                    if next >= tab.history.len() {
                        return Err("no later folder".into());
                    }
                    next
                };
                tab.position = position;
                tab.path = tab.history[position].clone();
                tab.entries.clear();
                tab.selected = None;
                tab.scope = FileScope::Folder;
                tab.query.clear();
                tab.stop_editing();
                let path = tab.path.clone();
                Ok(vec![AppEffect::ListDirectory {
                    window: id,
                    tab: index,
                    path,
                }])
            }
            "newtab" => {
                let home = self.home_folder();
                let (id, tabs, active) = self.focused_files()?;
                if tabs.len() >= TAB_LIMIT {
                    return Err("tab limit reached".into());
                }
                tabs.push(FileTab::new(&home));
                *active = tabs.len() - 1;
                let index = *active;
                Ok(vec![AppEffect::ListDirectory {
                    window: id,
                    tab: index,
                    path: home,
                }])
            }
            "open" => self.open_selection(),
            // List and grid show the same rows in the same order, so nothing moves
            // under a pointer when the layout changes.
            "view" => {
                let tab = self.focused_tab_mut()?;
                tab.view = match tab.view {
                    FileView::List => FileView::Grid,
                    FileView::Grid => FileView::List,
                };
                Ok(vec![])
            }
            "search" => {
                let tab = self.focused_tab_mut()?;
                tab.rename = None;
                tab.searching = true;
                Ok(vec![])
            }
            "search-clear" => {
                let tab = self.focused_tab_mut()?;
                if tab.query.is_empty() && !tab.searching {
                    return Err("no search to clear".into());
                }
                tab.query.clear();
                tab.searching = false;
                Ok(vec![])
            }
            "recents" => {
                let recents = self.recents.clone();
                let tab = self.focused_tab_mut()?;
                tab.scope = FileScope::Recents;
                tab.entries = recents;
                tab.selected = None;
                tab.query.clear();
                tab.stop_editing();
                Ok(vec![])
            }
            "browse" => {
                let path = self.focused_tab()?.path.clone();
                self.show_folder(&path, false)
            }
            "new-folder" | "new-file" => {
                let folder = command == "new-folder";
                let (id, tabs, active) = self.focused_files()?;
                let index = *active;
                let tab = tabs.get_mut(index).ok_or("tab not found")?;
                if tab.scope != FileScope::Folder {
                    return Err("the Recents list is not a folder".into());
                }
                let path = tab.path.clone();
                let name = if folder {
                    free_name(&tab.entries, "New folder", "", " ")
                } else {
                    free_name(&tab.entries, "Untitled", ".txt", " ")
                };
                let target = tab.child(&name);
                let create = if folder {
                    AppEffect::CreateDirectory {
                        window: id,
                        path: target,
                    }
                } else {
                    AppEffect::CreateFile {
                        window: id,
                        path: target,
                    }
                };
                // Relist rather than assume: what the folder holds afterwards is the
                // machine's answer, not this tab's guess.
                Ok(vec![
                    create,
                    AppEffect::ListDirectory {
                        window: id,
                        tab: index,
                        path,
                    },
                ])
            }
            "cut" | "copy" => {
                let tab = self.focused_tab()?;
                let path = tab.selected_path().ok_or("nothing is selected")?;
                self.clipboard = Some(Clipboard::new(vec![path], command == "cut"));
                Ok(vec![])
            }
            "paste" => self.paste(),
            "rename" => {
                let tab = self.focused_tab_mut()?;
                if tab.scope != FileScope::Folder {
                    return Err("the Recents list is not a folder".into());
                }
                let from = entry_name(tab.selection().ok_or("nothing is selected")?).to_owned();
                tab.searching = false;
                tab.rename = Some(Rename {
                    name: from.clone(),
                    from,
                });
                Ok(vec![])
            }
            "delete" => {
                let trash = self.trash_folder();
                let (id, tabs, active) = self.focused_files()?;
                let index = *active;
                let tab = tabs.get_mut(index).ok_or("tab not found")?;
                if tab.scope != FileScope::Folder {
                    return Err("the Recents list is not a folder".into());
                }
                let path = tab.selected_path().ok_or("nothing is selected")?;
                if path == trash || path.starts_with(&format!("{trash}/")) {
                    return Err("that is already in the trash".into());
                }
                let folder = tab.path.clone();
                Ok(vec![
                    AppEffect::TrashPath {
                        window: id,
                        path,
                        trash,
                    },
                    AppEffect::ListDirectory {
                        window: id,
                        tab: index,
                        path: folder,
                    },
                ])
            }
            rest => {
                if let Some(key) = rest.strip_prefix("sort:") {
                    let key = SortKey::parse(key).ok_or("unknown sort key")?;
                    let tab = self.focused_tab_mut()?;
                    // The same key again flips the direction; a different one starts
                    // ascending, which is what every file manager does.
                    tab.descending = tab.sort == key && !tab.descending;
                    tab.sort = key;
                    return Ok(vec![]);
                }
                if let Some(index) = rest.strip_prefix("tab:") {
                    let index: usize = index.parse().map_err(|_| "invalid tab")?;
                    let (id, tabs, active) = self.focused_files()?;
                    if index >= tabs.len() {
                        return Err("tab not found".into());
                    }
                    *active = index;
                    let path = tabs[index].path.clone();
                    // Re-list on switch: the folder may have changed while hidden.
                    return Ok(vec![AppEffect::ListDirectory {
                        window: id,
                        tab: index,
                        path,
                    }]);
                }
                if let Some(index) = rest.strip_prefix("closetab:") {
                    let index: usize = index.parse().map_err(|_| "invalid tab")?;
                    let (id, tabs, active) = self.focused_files()?;
                    if index >= tabs.len() {
                        return Err("tab not found".into());
                    }
                    if tabs.len() == 1 {
                        // Closing the last tab closes the window, as every shell does.
                        return self.close(id).map(|()| vec![]);
                    }
                    tabs.remove(index);
                    if *active > index {
                        *active -= 1;
                    }
                    *active = (*active).min(tabs.len() - 1);
                    return Ok(vec![]);
                }
                if let Some(path) = rest.strip_prefix("location:") {
                    let path = path.to_owned();
                    return self.show_folder(&path, true);
                }
                Err(format!("unknown file manager command {command}"))
            }
        }
    }
    /// Open whatever the active tab has selected: folders in place, documents in an
    /// editor. A document opened this way is what makes the Recents list real.
    fn open_selection(&mut self) -> Result<Vec<AppEffect>, String> {
        let tab = self.focused_tab()?;
        let entry = tab.selection().ok_or("nothing is selected")?.clone();
        let target = tab.selected_path().ok_or("nothing is selected")?;
        if entry.ends_with('/') {
            self.show_folder(&target, true)
        } else {
            self.remember(&target);
            self.launch("editor", &target).map(|(_, effects)| effects)
        }
    }
    /// A single click selects, focuses and switches; it never opens anything.
    pub fn click(&mut self, target: &str) -> Result<Vec<AppEffect>, String> {
        if let Some(id) = target.strip_prefix("focus:") {
            self.focus(id.parse().map_err(|_| "invalid window ID")?)?;
            return Ok(vec![]);
        }
        if target == "editor-save" {
            return self.key("Ctrl+s");
        }
        if let Some(window) = self.focused.and_then(|id| self.windows.get_mut(&id)) {
            if let AppState::Native(app) = &mut window.state {
                let (id, clock) = (window.id, self.clock_us);
                let effects = app.click(id, target, clock)?;
                return self.native_effects(effects);
            }
        }
        if let Some(command) = target.strip_prefix("files-") {
            return self.files_command(command);
        }
        if let Some(row) = target.strip_prefix("open:") {
            // `row` is a position on screen. Resolving it through `display()` is what
            // keeps a sorted or filtered view honest: the click selects the entry the
            // pointer was over, not the one at that index of the raw listing.
            let row: usize = row.parse().map_err(|_| "invalid entry")?;
            let (_, tabs, active) = self.focused_files()?;
            let active = *active;
            let tab = tabs.get_mut(active).ok_or("tab not found")?;
            let index = *tab.display().get(row).ok_or("entry not found")?;
            tab.selected = Some(index);
            tab.stop_editing();
            return Ok(vec![]);
        }
        if let Some(lines) = target.strip_prefix("terminal-scroll:") {
            let lines: usize = lines.parse().map_err(|_| "invalid scroll position")?;
            let id = self.focused.ok_or("no focused window")?;
            let state = &mut self.windows.get_mut(&id).ok_or("window not found")?.state;
            let AppState::Terminal { scroll, .. } = state else {
                return Err("window is not a terminal".into());
            };
            *scroll = lines.min(SCROLL_LIMIT);
            return Ok(vec![]);
        }
        let state = &self
            .focused
            .and_then(|id| self.windows.get(&id))
            .ok_or("no focused window")?
            .state;
        match (target, state) {
            (_, AppState::Editor { .. }) if target.starts_with("editor-text") => Ok(vec![]),
            ("terminal-input" | "terminal-line", AppState::Terminal { .. })
            | ("browser-address", AppState::Browser { .. }) => Ok(vec![]),
            _ => Err("interaction does not belong to focused application".into()),
        }
    }
    /// Click carrying the offset inside the control that was hit, so a text view can
    /// place its caret where the pointer actually landed.
    pub fn click_at(&mut self, target: &str, dx: i32, dy: i32) -> Result<Vec<AppEffect>, String> {
        let id = self.focused.ok_or("no focused window")?;
        let clock = self.clock_us;
        if let Some(AppState::Native(app)) = self
            .windows
            .get_mut(&id)
            .map(|w| &mut w.state)
            .filter(|_| !target.starts_with("focus:"))
        {
            let effects = app.click_at(id, target, dx, dy, clock)?;
            return self.native_effects(effects);
        }
        // `editor-text:<first row>[:<columns>]`: the scroll position, and the wrap width
        // when the view soft-wraps, both as the view painted them.
        let grid = target.strip_prefix("editor-text").map(|rest| {
            let mut parts = rest.trim_start_matches(':').split(':');
            let first = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            let columns = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            (first, columns)
        });
        let placed = match (self.windows.get_mut(&id).map(|w| &mut w.state), grid) {
            (Some(AppState::Editor { text, cursor, .. }), Some((first, columns))) => {
                *cursor = caret_for_point_wrapped(text, first, columns, dx, dy);
                true
            }
            // `terminal-line` is the prompt line alone, so `dx` is measured from the
            // first character of the input; the surrounding `terminal-input` body only
            // focuses the shell and leaves the caret where it was.
            (Some(AppState::Terminal { input, cursor, .. }), None) if target == "terminal-line" => {
                *cursor = caret_for_column(input, dx);
                true
            }
            _ => false,
        };
        if placed {
            return Ok(vec![]);
        }
        self.click(target)
    }
    /// A double click opens the thing that was clicked. Anything without a distinct
    /// double-click meaning falls back to the single-click behaviour.
    pub fn activate(&mut self, target: &str) -> Result<Vec<AppEffect>, String> {
        let clock = self.clock_us;
        if let Some(id) = self.focused {
            if let Some(AppState::Native(app)) = self
                .windows
                .get_mut(&id)
                .map(|w| &mut w.state)
                .filter(|_| !target.starts_with("focus:"))
            {
                let effects = app.activate(id, target, clock)?;
                return self.native_effects(effects);
            }
        }
        if target.starts_with("open:") {
            self.click(target)?;
            return self.open_selection();
        }
        self.click(target)
    }
}
/// Collapse a folder path to the canonical form the rest of the system stores.
fn normalize_folder(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.into()
    }
}
/// Byte offset in `text` nearest the point `(dx, dy)` of a monospace text view whose
/// topmost visible line is `first`. The 8x18 cell is the editor's painted grid.
pub fn caret_for_point(text: &str, first: usize, dx: i32, dy: i32) -> usize {
    caret_for_point_wrapped(text, first, 0, dx, dy)
}
/// The rows an editor paints: byte ranges into `text`, one per visual row. With
/// `columns` > 0 a line longer than that many characters is soft-wrapped, after the
/// last space that fits or, in a word longer than the row, at the edge. A row that
/// ends where the next begins is a soft wrap; a newline sits between the others.
pub fn editor_rows(text: &str, columns: usize) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    let mut start = 0;
    for line in text.split('\n') {
        let end = start + line.len();
        let mut from = start;
        loop {
            let rest = &text[from..end];
            match rest.char_indices().nth(columns).filter(|_| columns > 0) {
                None => {
                    rows.push((from, end));
                    break;
                }
                Some((cut, _)) => {
                    let brk = rest[..cut].rfind(' ').map_or(cut, |space| space + 1);
                    rows.push((from, from + brk));
                    from += brk;
                }
            }
        }
        start = end + 1;
    }
    rows
}
/// Visual (row, column) of the caret at byte `cursor` among `editor_rows`. At a soft
/// wrap the caret belongs to the start of the next row, as it does in a real editor.
pub fn editor_caret_cell(text: &str, cursor: usize, columns: usize) -> (usize, usize) {
    let mut cursor = cursor.min(text.len());
    while !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let rows = editor_rows(text, columns);
    for (i, (start, end)) in rows.iter().enumerate() {
        let soft = rows.get(i + 1).is_some_and(|(next, _)| next == end);
        if cursor >= *start && (cursor < *end || (cursor == *end && !soft)) {
            return (i, text[*start..cursor].chars().count());
        }
    }
    let (start, end) = rows[rows.len() - 1];
    (rows.len() - 1, text[start..end].chars().count())
}
/// `caret_for_point` on soft-wrapped rows `columns` characters wide (0: no wrap).
pub fn caret_for_point_wrapped(
    text: &str,
    first: usize,
    columns: usize,
    dx: i32,
    dy: i32,
) -> usize {
    const CELL_W: i32 = 8;
    const LINE_H: i32 = 18;
    let row = first + (dy.max(0) / LINE_H) as usize;
    let column = ((dx.max(0) + CELL_W / 2) / CELL_W) as usize;
    let rows = editor_rows(text, columns);
    let Some(&(start, end)) = rows.get(row) else {
        return text.len();
    };
    let content = &text[start..end];
    match content.char_indices().nth(column) {
        Some((i, _)) => start + i,
        // Past the end of a soft-wrapped row: the caret stays on this row, before its
        // last character, rather than jumping to the start of the next one.
        None if rows.get(row + 1).is_some_and(|(next, _)| *next == end) => content
            .char_indices()
            .next_back()
            .map_or(start, |(i, _)| start + i),
        None => end,
    }
}
/// Byte offset in a one-line monospace field for a click `dx` pixels from its first
/// character, on the same 8px cell the terminal paints.
pub fn caret_for_column(text: &str, dx: i32) -> usize {
    const CELL_W: i32 = 8;
    let column = ((dx.max(0) + CELL_W / 2) / CELL_W) as usize;
    text.char_indices()
        .nth(column)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}
fn parent_folder(path: &str) -> &str {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/",
        Some((parent, _)) => parent,
    }
}

/// Serialized instance data and declared module version. Registry code is supplied
/// by the embedding runtime and is never deserialized from a snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppInstance {
    pub kind: String,
    pub version: u32,
    pub state: serde_json::Value,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RegisteredApplications {
    pub instances: BTreeMap<String, AppInstance>,
}
impl RegisteredApplications {
    pub fn launch(
        &mut self,
        registry: &cw_sdk::Registry,
        kind: &str,
        id: &str,
        initial: serde_json::Value,
        context: &cw_sdk::AppContext,
    ) -> cw_protocol::Result<()> {
        if id.is_empty() || self.instances.contains_key(id) {
            return Err(cw_protocol::SimError::invalid(
                "empty or duplicate app instance",
            ));
        }
        let app = registry.application(kind)?;
        let state = app.initialize(initial, context)?;
        app.page(&state, context)?.validate()?;
        self.instances.insert(
            id.into(),
            AppInstance {
                kind: kind.into(),
                version: app.version(),
                state,
            },
        );
        Ok(())
    }
    pub fn event(
        &mut self,
        registry: &cw_sdk::Registry,
        id: &str,
        context: &cw_sdk::AppContext,
        event: &cw_sdk::AppEvent,
    ) -> cw_protocol::Result<Vec<cw_sdk::AppEffect>> {
        let instance = self
            .instances
            .get_mut(id)
            .ok_or_else(|| cw_protocol::SimError::not_found("app instance"))?;
        let app = registry.application(&instance.kind)?;
        if app.version() != instance.version {
            return Err(cw_protocol::SimError::invalid(
                "app module version mismatch",
            ));
        }
        // A plugin's failed event cannot leave a partially mutated app state.
        let mut next = instance.state.clone();
        let effects = app.event(&mut next, context, event)?;
        app.page(&next, context)?.validate()?;
        instance.state = next;
        Ok(effects)
    }
    pub fn page(
        &self,
        registry: &cw_sdk::Registry,
        id: &str,
        context: &cw_sdk::AppContext,
    ) -> cw_protocol::Result<cw_protocol::Page> {
        let instance = self
            .instances
            .get(id)
            .ok_or_else(|| cw_protocol::SimError::not_found("app instance"))?;
        let app = registry.application(&instance.kind)?;
        if app.version() != instance.version {
            return Err(cw_protocol::SimError::invalid(
                "app module version mismatch",
            ));
        }
        let page = app.page(&instance.state, context)?;
        page.validate()?;
        Ok(page)
    }
}
#[cfg(test)]
mod extension_tests {
    use super::*;
    struct Counter;
    impl cw_sdk::Application for Counter {
        fn kind(&self) -> &str {
            "counter"
        }
        fn event(
            &self,
            state: &mut serde_json::Value,
            _: &cw_sdk::AppContext,
            event: &cw_sdk::AppEvent,
        ) -> cw_protocol::Result<Vec<cw_sdk::AppEffect>> {
            *state = serde_json::json!(state.as_u64().unwrap_or(0) + 1);
            if event.kind == "fail" {
                return Err(cw_protocol::SimError::invalid("rejected"));
            }
            Ok(vec![])
        }
        fn page(
            &self,
            state: &serde_json::Value,
            _: &cw_sdk::AppContext,
        ) -> cw_protocol::Result<cw_protocol::Page> {
            Ok(cw_protocol::Page::new(format!("Count {state}")))
        }
    }
    #[test]
    fn custom_application_state_and_failed_event_atomicity() {
        let mut registry = cw_sdk::Registry::new();
        registry.register_application(Counter).unwrap();
        let mut apps = RegisteredApplications::default();
        let ctx = cw_sdk::AppContext {
            actor: "a".into(),
            machine: "m".into(),
            tick: 0,
            seed: 1,
            instance: "counter1".into(),
        };
        apps.launch(&registry, "counter", "counter1", serde_json::json!(0), &ctx)
            .unwrap();
        apps.event(
            &registry,
            "counter1",
            &ctx,
            &cw_sdk::AppEvent {
                kind: "increment".into(),
                target: None,
                data: serde_json::Value::Null,
            },
        )
        .unwrap();
        assert_eq!(
            apps.page(&registry, "counter1", &ctx).unwrap().title,
            "Count 1"
        );
        assert!(apps
            .event(
                &registry,
                "counter1",
                &ctx,
                &cw_sdk::AppEvent {
                    kind: "fail".into(),
                    target: None,
                    data: serde_json::Value::Null
                }
            )
            .is_err());
        assert_eq!(apps.instances["counter1"].state, serde_json::json!(1));
    }
}

#[cfg(test)]
mod file_manager_tests {
    use super::*;
    fn files(entries: &[&str]) -> DesktopState {
        let mut d = DesktopState {
            home: "/home/alice".into(),
            ..DesktopState::default()
        };
        let (id, _) = d.launch("files", "/work").unwrap();
        d.directory_loaded(id, 0, entries.iter().map(|e| (*e).to_owned()).collect())
            .unwrap();
        d
    }
    fn tab(d: &DesktopState) -> &FileTab {
        d.focused_tab().unwrap()
    }
    /// Sorting reorders the screen, so it has to reorder the click targets with it.
    /// Selection stays pinned to the file, not to the row it happened to be on.
    #[test]
    fn display_order_drives_click_targets_and_selection_follows_the_file() {
        let mut d = files(&["b.txt", "a/", "c.txt"]);
        // The raw listing is asciibetical; the default view sorts by name.
        assert_eq!(tab(&d).entries, vec!["a/", "b.txt", "c.txt"]);
        d.click("open:1").unwrap();
        assert_eq!(tab(&d).selection().unwrap(), "b.txt");
        // Folders first, then the same file is on a different row.
        d.click("files-sort:kind").unwrap();
        assert_eq!(tab(&d).selection().unwrap(), "b.txt");
        assert_eq!(tab(&d).row_of(1), Some(1));
        d.click("files-sort:kind").unwrap();
        assert!(tab(&d).descending);
        assert_eq!(tab(&d).row_of(1), Some(1));
        // Reversed by name: row 0 is the last entry of the listing.
        d.click("files-sort:name").unwrap();
        d.click("files-sort:name").unwrap();
        d.click("open:0").unwrap();
        assert_eq!(tab(&d).selection().unwrap(), "c.txt");
        // A row past the end of the displayed list is refused, not clamped.
        assert!(d.click("open:3").is_err());
    }
    #[test]
    fn search_filters_the_rows_and_renumbers_them() {
        let mut d = files(&["alpha.txt", "beta.txt", "gamma.txt"]);
        d.click("files-search").unwrap();
        d.text("MA").unwrap();
        // Case-insensitive substring, and the surviving row is row 0.
        assert_eq!(tab(&d).display(), vec![2]);
        d.click("open:0").unwrap();
        assert_eq!(tab(&d).selection().unwrap(), "gamma.txt");
        // Escape cancels the filter outright; a hidden filter would be a lie.
        d.key("Escape").unwrap();
        assert!(tab(&d).query.is_empty());
        assert_eq!(tab(&d).display().len(), 3);
        // The field is bounded, and so is the rename buffer.
        d.click("files-search").unwrap();
        d.text(&"x".repeat(FIELD_LIMIT)).unwrap();
        assert!(d.text("y").is_err());
        assert_eq!(tab(&d).query.chars().count(), FIELD_LIMIT);
    }
    #[test]
    fn navigating_leaves_every_field_and_the_recents_scope() {
        let mut d = files(&["a.txt"]);
        d.recents = vec!["/work/a.txt".into()];
        d.click("files-recents").unwrap();
        assert_eq!(tab(&d).scope, FileScope::Recents);
        // A listing for the folder the tab left does not overwrite Recents.
        let id = d.focused.unwrap();
        d.directory_loaded(id, 0, vec!["stale.txt".into()]).unwrap();
        assert_eq!(tab(&d).entries, vec!["/work/a.txt"]);
        // Opening a recent reaches the absolute path, not a child of the folder.
        d.click("open:0").unwrap();
        let effects = d.activate("open:0").unwrap();
        assert!(matches!(
            effects.first(),
            Some(AppEffect::ReadFile { path, .. }) if path == "/work/a.txt"
        ));
        // Mutations are refused in Recents: it is a list, not a folder.
        let mut d = files(&["a.txt"]);
        d.recents = vec!["/work/a.txt".into()];
        d.click("files-recents").unwrap();
        d.click("open:0").unwrap();
        for command in ["files-rename", "files-delete", "files-new-file"] {
            assert!(d.click(command).is_err(), "{command} acted on Recents");
        }
        // And a filter typed in one folder does not follow the tab to the next.
        let mut d = files(&["a.txt"]);
        d.click("files-search").unwrap();
        d.text("zz").unwrap();
        d.click("files-up").unwrap();
        assert!(tab(&d).query.is_empty() && !tab(&d).searching);
    }
    #[test]
    fn recents_are_bounded_deduplicated_and_only_ever_opened_documents() {
        let mut d = files(&["a.txt", "sub/"]);
        // Opening a folder is navigation, not a document: it never lands in Recents.
        d.click("open:1").unwrap();
        d.activate("open:1").unwrap();
        assert!(d.recents.is_empty());
        for i in 0..RECENT_LIMIT + 5 {
            d.remember(&format!("/work/f{i}.txt"));
        }
        assert_eq!(d.recents.len(), RECENT_LIMIT);
        assert_eq!(d.recents[0], format!("/work/f{}.txt", RECENT_LIMIT + 4));
        // Reopening moves a path to the front rather than repeating it.
        d.remember("/work/f0.txt");
        assert_eq!(d.recents[0], "/work/f0.txt");
        assert_eq!(d.recents.iter().filter(|p| *p == "/work/f0.txt").count(), 1);
        assert_eq!(d.recents.len(), RECENT_LIMIT);
    }
    #[test]
    fn clipboard_names_a_free_destination_and_a_cut_is_consumed_by_its_paste() {
        let mut d = files(&["notes.txt", "notes (copy).txt"]);
        d.click("open:1").unwrap();
        d.click("files-copy").unwrap();
        // The first free `(copy)` name, derived from the listing already on screen.
        let effects = d.click("files-paste").unwrap();
        assert!(matches!(
            &effects[0],
            AppEffect::CopyPath { to, .. } if to == "/work/notes (copy) 2.txt"
        ));
        // A copy survives its paste, so the same thing can be pasted twice.
        assert!(d.clipboard.is_some());
        d.click("files-cut").unwrap();
        // A cut onto a name already here is refused: a move must not rename itself.
        assert!(d.click("files-paste").is_err());
        d.click("files-up").unwrap();
        let effects = d.click("files-paste").unwrap();
        assert!(matches!(
            &effects[0],
            AppEffect::MovePath { from, to, .. }
                if from == "/work/notes.txt" && to == "/notes.txt"
        ));
        assert!(d.clipboard.is_none(), "a cut outlived its paste");
        assert_eq!(
            Clipboard::new(vec!["/p".into(); CLIPBOARD_LIMIT + 9], true)
                .paths
                .len(),
            CLIPBOARD_LIMIT
        );
    }
    #[test]
    fn delete_is_a_move_into_the_trash_and_refuses_to_nest_it() {
        let mut d = files(&["notes.txt"]);
        assert_eq!(d.trash_folder(), "/home/alice/.local/share/Trash/files");
        d.click("open:0").unwrap();
        let effects = d.click("files-delete").unwrap();
        assert!(matches!(
            &effects[0],
            AppEffect::TrashPath { path, trash, .. }
                if path == "/work/notes.txt" && trash == &d.trash_folder()
        ));
        // Emptying the trash from inside it is refused rather than nesting it.
        let mut d = files(&["old.txt"]);
        let (id, _) = d.launch("files", &d.trash_folder()).unwrap();
        d.directory_loaded(id, 0, vec!["old.txt".into()]).unwrap();
        d.click("open:0").unwrap();
        assert!(d.click("files-delete").is_err());
    }
    #[test]
    fn a_rename_only_commits_a_name_that_is_really_a_name() {
        let mut d = files(&["notes.txt", "launch.txt"]);
        d.click("open:1").unwrap();
        d.click("files-rename").unwrap();
        assert_eq!(tab(&d).rename.as_ref().unwrap().name, "notes.txt");
        for bad in ["", "..", "../escape", "a/b", r"a\b"] {
            let t = d.focused_tab_mut().unwrap();
            t.rename.as_mut().unwrap().name = bad.into();
            assert!(d.key("Enter").is_err(), "{bad} was accepted as a name");
            // A refused commit clears the edit rather than leaving it half-applied.
            assert!(tab(&d).rename.is_none());
            d.click("files-rename").unwrap();
        }
        d.focused_tab_mut().unwrap().rename.as_mut().unwrap().name = "launch.txt".into();
        assert!(d.key("Enter").is_err(), "renamed onto an existing file");
        d.click("files-rename").unwrap();
        d.key("Backspace").unwrap();
        let effects = d.key("Enter").unwrap();
        assert!(matches!(
            &effects[0],
            AppEffect::MovePath { from, to, .. }
                if from == "/work/notes.txt" && to == "/work/notes.tx"
        ));
    }
    #[test]
    fn the_terminal_caret_and_scroll_are_state_the_snapshot_keeps() {
        let mut d = DesktopState::default();
        let (id, _) = d.launch("terminal", "").unwrap();
        d.text("echo hi").unwrap();
        d.click_at("terminal-line", 4 * 8, 0).unwrap();
        d.text("X").unwrap();
        d.key("Backspace").unwrap();
        d.key("Home").unwrap();
        d.text("!").unwrap();
        match &d.windows[&id].state {
            AppState::Terminal { input, cursor, .. } => {
                assert_eq!(input, "!echo hi");
                assert_eq!(*cursor, 1);
            }
            _ => panic!("not a terminal"),
        }
        // Scroll is bounded, and any output pins the view back to the tail.
        d.click(&format!("terminal-scroll:{}", SCROLL_LIMIT + 1000))
            .unwrap();
        match &d.windows[&id].state {
            AppState::Terminal { scroll, .. } => assert_eq!(*scroll, SCROLL_LIMIT),
            _ => panic!("not a terminal"),
        }
        d.terminal_output(id, TerminalEntry::new("$", "ls", "a", "", 0))
            .unwrap();
        match &d.windows[&id].state {
            AppState::Terminal { scroll, .. } => assert_eq!(*scroll, 0),
            _ => panic!("not a terminal"),
        }
        let restored: DesktopState =
            serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(d, restored);
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;
    #[test]
    fn shift_cycles_off_once_lock_and_a_one_shot_releases_after_one_character() {
        let mut k = KeyboardState::default();
        assert_eq!(k.apply('a'), "a");
        k.cycle_shift();
        assert_eq!(k.shift, Shift::Once);
        assert_eq!(k.apply('a'), "A");
        assert_eq!(k.shift, Shift::Off, "a one-shot must release");
        assert_eq!(k.apply('b'), "b");
        k.cycle_shift();
        k.cycle_shift();
        assert_eq!(k.shift, Shift::Lock);
        assert_eq!((k.apply('c'), k.apply('d')), ("C".into(), "D".into()));
        assert_eq!(k.shift, Shift::Lock, "a lock must not release");
        k.cycle_shift();
        assert_eq!(k.shift, Shift::Off);
    }
    #[test]
    fn leaving_the_letter_plane_drops_a_pending_shift() {
        let mut k = KeyboardState::default();
        k.cycle_shift();
        k.set_plane(Plane::Numbers);
        assert_eq!(k.shift, Shift::Off, "a shift must not survive into digits");
        assert_eq!(k.plane, Plane::Numbers);
        k.set_plane(Plane::Letters);
        k.cycle_shift();
        k.set_plane(Plane::Letters);
        assert_eq!(k.shift, Shift::Once, "staying put must not drop it");
    }
    #[test]
    fn a_character_with_no_upper_case_is_unchanged_by_shift() {
        let mut k = KeyboardState {
            shift: Shift::Lock,
            plane: Plane::Letters,
        };
        assert_eq!(k.apply('1'), "1");
        assert_eq!(k.apply('.'), ".");
        // Non-ASCII still upper-cases, and may widen: ß becomes SS.
        assert_eq!(k.apply('é'), "É");
    }
}
#[cfg(test)]
mod focus_tests {
    use super::*;
    #[test]
    fn wrong_application_input_and_invalid_editor_cursor_rejected() {
        let mut desktop = DesktopState::default();
        let (id, _) = desktop.launch("editor", "/a").unwrap();
        assert!(desktop.click("terminal-input").is_err());
        desktop.click("editor-text").unwrap();
        if let AppState::Editor { text, cursor, .. } =
            &mut desktop.windows.get_mut(&id).unwrap().state
        {
            *text = "😀".into();
            *cursor = 1;
        }
        assert!(desktop.key("Backspace").is_err());
    }
}

#[cfg(test)]
mod window_geometry_tests {
    use super::*;
    use cw_scene::Rect;
    #[test]
    fn dragging_capture_survives_snapshot_and_focus_is_mru() {
        let area = Rect::new(0, 28, 1200, 700);
        let mut d = DesktopState::default();
        let (a, _) = d.launch("editor", "").unwrap();
        let (b, _) = d.launch("terminal", "").unwrap();
        let before = d.effective_frame(a, area);
        d.pointer_down(a, "drag", before.x + 100, before.y + 12, area)
            .unwrap();
        assert_eq!(d.ordered_windows(), vec![b, a]);
        d.pointer_move(before.x + 150, before.y + 42, area).unwrap();
        let mut restored: DesktopState =
            serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
        restored
            .pointer_up(before.x + 150, before.y + 42, area)
            .unwrap();
        assert_eq!(
            restored.effective_frame(a, area),
            Rect::new(before.x + 50, before.y + 30, before.width, before.height)
        );
        restored.minimize(a).unwrap();
        assert_eq!(restored.focused, Some(b));
        restored.focus(a).unwrap();
        assert!(!restored.windows[&a].minimized);
    }
    #[test]
    fn maximize_restore_is_per_window_and_snap_tearoff_restores_size() {
        let area = Rect::new(0, 28, 1200, 700);
        let mut d = DesktopState::default();
        let (a, _) = d.launch("files", "").unwrap();
        let (b, _) = d.launch("terminal", "").unwrap();
        let original = d.effective_frame(a, area);
        d.maximize(a, area).unwrap();
        assert_eq!(d.effective_frame(a, area), area);
        assert!(!d.windows[&b].maximized);
        d.maximize(a, area).unwrap();
        assert_eq!(d.effective_frame(a, area), original);
        d.snap(a, WindowSnap::Left, area).unwrap();
        assert_eq!(d.effective_frame(a, area).width, 600);
        d.pointer_down(a, "drag", 200, 45, area).unwrap();
        d.pointer_up(450, 200, area).unwrap();
        assert_eq!(d.effective_frame(a, area).width, original.width);
        assert_eq!(d.windows[&a].snapped, None);
    }
    #[test]
    fn all_resize_edges_preserve_opposite_edge_and_reject_bad_handles() {
        let area = Rect::new(0, 0, 1400, 1000);
        for edge in ["n", "ne", "e", "se", "s", "sw", "w", "nw"] {
            let mut d = DesktopState::default();
            let (id, _) = d.launch("terminal", "").unwrap();
            d.windows.get_mut(&id).unwrap().frame = Some(Rect::new(200, 200, 600, 400));
            d.pointer_down(id, &format!("resize:{edge}"), 300, 300, area)
                .unwrap();
            d.pointer_up(320, 330, area).unwrap();
            let r = d.effective_frame(id, area);
            assert_eq!(r.x, if edge.contains('w') { 220 } else { 200 });
            assert_eq!(r.y, if edge.contains('n') { 230 } else { 200 });
            assert_eq!(
                r.width,
                if edge.contains('w') {
                    580
                } else if edge.contains('e') {
                    620
                } else {
                    600
                }
            );
            assert_eq!(
                r.height,
                if edge.contains('n') {
                    370
                } else if edge.contains('s') {
                    430
                } else {
                    400
                }
            );
            assert!(d.pointer_down(id, "resize:bad", 0, 0, area).is_err());
        }
    }
}
