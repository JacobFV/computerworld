use super::*;

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
    /// What the address bar holds, which is what a person typed rather than strictly a
    /// URL: the browser reads it through its omnibox (`cw_browser::omnibox`), so a bare
    /// host, a host and a path, or a search phrase all land somewhere sensible.
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
    /// Put one trashed thing back where it came from. `path` names it the way the view
    /// has it — its path under `files/` — and the `.trashinfo` record says where that
    /// is. A delete a user regrets is undone by the machine, not by the file manager
    /// guessing which folder the file used to be in.
    RestorePath {
        window: u64,
        path: String,
    },
    /// Throw the whole trash away for good. The one file-manager command that really
    /// destroys data, which is why it is its own effect and never a flag on a delete.
    EmptyTrash {
        window: u64,
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
    /// Encode pixels as PNG and write them to `path`. Encoding needs the rasterizer, so
    /// the environment does it; the application only hands over what it drew.
    WriteImage {
        window: u64,
        path: String,
        width: u32,
        height: u32,
        #[serde(skip)]
        rgba: Vec<u8>,
    },
    /// Rasterise a line of text in the platform's bundled font and hand back its
    /// coverage, so a text tool stamps exactly the glyphs the renderer draws.
    RasterText {
        window: u64,
        text: String,
        size: u16,
        bold: bool,
    },
    /// Read a file's exact bytes: a workbook or a database is binary, which `ReadFile`'s
    /// lossy text would destroy. The bytes, or why they could not be read, go to
    /// `DesktopState::bytes_loaded`.
    ReadBytes {
        window: u64,
        path: String,
    },
    /// Write bytes to `path`, replacing what is there; the outcome goes to
    /// `DesktopState::bytes_saved`, so a failed save is reported rather than assumed.
    WriteBytes {
        window: u64,
        path: String,
        #[serde(skip)]
        bytes: Vec<u8>,
    },
    /// Put pixels on the machine's clipboard.
    CopyImage {
        window: u64,
        width: u32,
        height: u32,
        #[serde(skip)]
        rgba: Vec<u8>,
    },
    /// Ask for the picture on the machine's clipboard; it arrives as an image delivery
    /// for the path `clipboard:`, or as a failure naming why there is none.
    PasteImage {
        window: u64,
    },
    /// A Run and Debug request for the machine's debugger (`cw_computer::Computer::debug`):
    /// launch a program, step it, read its variables, evaluate an expression in a frame.
    /// The reply, or the reason there is none, goes to `DesktopState::debug_reply` under
    /// `tag`, which says what the application asked for.
    Debug {
        window: u64,
        tag: String,
        request: cw_protocol::debug::Request,
    },
    /// Record named data in the world's event log, as a registered application's
    /// `cw_sdk::AppEffect::Emit` does: what a checker or a service may watch for.
    Emit {
        window: u64,
        name: String,
        data: serde_json::Value,
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
/// The path a pasted picture is delivered under.
pub const CLIPBOARD_IMAGE: &str = "clipboard:";
/// The path a failed `RasterText` is reported under.
pub const TEXT_IMAGE: &str = "text:";
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
