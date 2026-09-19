//! What a debugger and a Run and Debug view say to each other: the Debug Adapter
//! Protocol's requests and events, in this world's own types.
//!
//! A machine serves these (`cw_computer::Computer::debug`); an application asks for them
//! with `AppEffect::Debug` and is answered with a [`Reply`]. Nothing here runs a program:
//! these are the words, and the machine's language runtimes are what speak them.
use serde::{Deserialize, Serialize};

/// A breakpoint a person set, before any debugger has seen it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceBreakpoint {
    /// Absolute path of the file on the machine.
    pub path: String,
    /// 1-based line.
    pub line: u32,
    /// Stop here only when this expression is true in the frame that reached it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// A disabled breakpoint stays in the list and is not sent to the adapter.
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn yes() -> bool {
    true
}

/// Which exceptions stop the program, DAP's exception filters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionFilters {
    /// Stop when an exception reaches the top without being handled.
    pub uncaught: bool,
    /// Stop wherever an exception is raised, handled or not.
    pub raised: bool,
}

/// Start a program under the debugger.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    /// `python` or `node`: which of the machine's runtimes runs the program.
    pub kind: String,
    /// The program's absolute path on the machine.
    pub program: String,
    /// Working directory for the run.
    pub cwd: String,
    /// Arguments after the program name.
    #[serde(default)]
    pub args: Vec<String>,
    /// Stop before the first line rather than running to the first breakpoint.
    #[serde(default)]
    pub stop_on_entry: bool,
    #[serde(default)]
    pub breakpoints: Vec<SourceBreakpoint>,
    #[serde(default)]
    pub exceptions: ExceptionFilters,
}

/// How to carry on from a stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Run until the next breakpoint, exception or the end.
    #[default]
    Continue,
    /// The next line of this frame, over any call.
    Over,
    /// Into the call on this line, if there is one.
    Into,
    /// Out of this frame, to where it returns.
    Out,
}

/// One request of the Run and Debug view. A session is one program being debugged; a
/// machine may hold several, so every request names the one it is about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum Request {
    /// Begin a session. The reply's `session` is the handle for everything after it.
    Launch(Launch),
    /// Replace the breakpoints in one file while the program runs.
    Breakpoints {
        session: u64,
        path: String,
        points: Vec<SourceBreakpoint>,
    },
    /// Which exceptions stop the program, changed mid-session.
    Exceptions {
        session: u64,
        filters: ExceptionFilters,
    },
    /// Carry on from where it stopped.
    Resume { session: u64, step: Step },
    /// Stop the program where it is (DAP's `pause`).
    Pause { session: u64 },
    /// The variables of one scope of one frame, or of a value already reported
    /// (`Variable::reference`), for expanding a structure.
    Variables { session: u64, reference: u64 },
    /// Evaluate an expression in a frame: the Debug Console and the watch list.
    Evaluate {
        session: u64,
        frame: u64,
        expression: String,
        /// `watch`, `repl` or `hover`, as DAP's context.
        context: String,
    },
    /// End the session; the program is killed if it is still running.
    Terminate { session: u64 },
}

/// One frame of the call stack, innermost first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    /// The frame's handle, for `Evaluate` and for the scopes of `Variables`.
    pub id: u64,
    /// The function's name, or `<module>` for the file itself.
    pub name: String,
    /// Absolute path of the file the frame is in.
    pub path: String,
    /// 1-based line the frame is stopped at.
    pub line: u32,
}

/// A named value in a scope, or inside another value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    /// The value as the language would print it.
    pub value: String,
    /// The type's name, as the debugger reports it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// Non-zero when this value holds more: ask `Variables` for this reference.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub reference: u64,
}
fn is_zero(v: &u64) -> bool {
    *v == 0
}

/// A scope of a frame: locals, globals, and whatever else the runtime keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub name: String,
    pub reference: u64,
    /// Scopes a view should show collapsed (globals, usually).
    #[serde(default)]
    pub expensive: bool,
}

/// Why the program is not running.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Stopped {
    /// Before the first line, because the configuration asked for it.
    Entry,
    /// On a breakpoint of `path` at `line`.
    Breakpoint { path: String, line: u32 },
    /// A step finished.
    Step,
    /// The debugger was asked to pause.
    Pause,
    /// An exception: `text` is what the runtime would print.
    Exception { text: String },
    /// The program ended.
    Exited { code: i32 },
}
impl Stopped {
    /// What the call stack's header says, as VS Code names the reason.
    pub fn label(&self) -> String {
        match self {
            Self::Entry => "Paused on entry".into(),
            Self::Breakpoint { .. } => "Paused on breakpoint".into(),
            Self::Step => "Paused on step".into(),
            Self::Pause => "Paused".into(),
            Self::Exception { text } => format!("Paused on exception: {text}"),
            Self::Exited { code } => format!("Exited with code {code}"),
        }
    }
    pub fn is_exit(&self) -> bool {
        matches!(self, Self::Exited { .. })
    }
}

/// Where a program stands after a request that moved it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub stopped: Stopped,
    /// The call stack, innermost frame first; empty once the program has ended.
    #[serde(default)]
    pub frames: Vec<Frame>,
    /// The scopes of the innermost frame, in the order to show them.
    #[serde(default)]
    pub scopes: Vec<Scope>,
    /// Everything the program wrote since the last reply, stdout and stderr in order.
    #[serde(default)]
    pub output: String,
}

/// What a machine answers a [`Request`] with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum Reply {
    /// A session began: its handle, and where the program stands.
    Launched { session: u64, state: State },
    /// The program moved and stopped again (or ended).
    Stopped { state: State },
    /// Which of the breakpoints sent were verified, in the order they were sent: a
    /// line the runtime cannot stop on (a comment, a blank line) comes back `false`.
    Breakpoints { verified: Vec<bool> },
    /// The children of a reference.
    Variables { variables: Vec<Variable> },
    /// An expression's value, and what it holds if it is a structure.
    Evaluated { result: Variable },
    /// The session is over.
    Terminated,
}

/// The scopes of the innermost frame are asked for by frame id; `Request::Variables`
/// takes any reference a reply gave, and this is how a view asks for a frame's own.
pub const SCOPE_LOCALS: &str = "Locals";
pub const SCOPE_GLOBALS: &str = "Globals";
