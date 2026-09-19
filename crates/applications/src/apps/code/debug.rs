//! Run and Debug: the breakpoints a person set, the session the machine is running for
//! them, and what the view shows of it.
//!
//! Nothing here executes a program. The machine's debug adapter does that
//! (`cw_computer::debug`), reached through `AppEffect::Debug`; this is the view's own
//! record of what it asked for and what came back, so what is painted is only ever what
//! the machine said.
use cw_protocol::debug::{ExceptionFilters, Frame, Scope, SourceBreakpoint, Stopped, Variable};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The most breakpoints a workbench keeps, so a stored one cannot grow without bound.
pub const BREAKPOINT_LIMIT: usize = 200;
/// Lines the Debug Console keeps.
pub const CONSOLE_LIMIT: usize = 500;
/// Watch expressions.
pub const WATCH_LIMIT: usize = 20;

/// A breakpoint as the workbench holds it: VS Code keeps breakpoints whether or not
/// anything is running, and tells the adapter about them when a session starts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breakpoint {
    /// Absolute path on the machine.
    pub path: String,
    /// 1-based line.
    pub line: u32,
    /// Stop only when this is true in the frame that reached it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    pub enabled: bool,
    /// What the adapter said about it: `None` until a session has seen it, `Some(false)`
    /// when the runtime cannot stop there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
}
impl Breakpoint {
    /// The breakpoint as the protocol sends it.
    pub fn source(&self) -> SourceBreakpoint {
        SourceBreakpoint {
            path: self.path.clone(),
            line: self.line,
            condition: self.condition.clone(),
            enabled: self.enabled,
        }
    }
}

/// A watch expression and the last thing it evaluated to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watch {
    pub expression: String,
    /// The value, or the error the debugger gave for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default)]
    pub failed: bool,
    /// What the value holds, when it holds anything.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub reference: u64,
}
fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

/// A line of the Debug Console.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleKind {
    /// The program's own output.
    #[default]
    Output,
    /// An expression a person typed.
    Input,
    /// What it evaluated to.
    Result,
    /// Why it could not be evaluated, or why the session ended.
    Error,
    /// The debugger's own narration — what it started, what it exited with. Kept apart
    /// from the program's output so a traceback is read from the program alone.
    Notice,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleLine {
    pub kind: ConsoleKind,
    pub text: String,
}

/// The session the machine is running, as the view knows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// The machine's handle for it.
    pub id: u64,
    /// `python` or `node`.
    pub kind: String,
    pub program: String,
    /// Why it is not running, or `None` while a request is in flight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
    #[serde(default)]
    pub frames: Vec<Frame>,
    #[serde(default)]
    pub scopes: Vec<Scope>,
    /// The frame whose variables and evaluations the view is showing.
    #[serde(default)]
    pub frame: usize,
    /// The program has ended; the toolbar offers only Restart and Stop.
    #[serde(default)]
    pub ended: bool,
}
impl Session {
    pub fn frame_id(&self) -> u64 {
        self.frames.get(self.frame).map_or(0, |f| f.id)
    }
    /// Where the program is stopped: the file and line to show the caret at.
    pub fn at(&self) -> Option<(String, u32)> {
        self.frames
            .get(self.frame)
            .map(|f| (f.path.clone(), f.line))
    }
}

/// Everything the Run and Debug view holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Debug {
    #[serde(default)]
    pub breakpoints: Vec<Breakpoint>,
    #[serde(default)]
    pub exceptions: ExceptionFilters,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<Session>,
    #[serde(default)]
    pub watches: Vec<Watch>,
    #[serde(default)]
    pub console: Vec<ConsoleLine>,
    /// What is typed in the Debug Console's input.
    #[serde(default)]
    pub input: String,
    /// The box the view has open for a new watch expression or a breakpoint condition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<Prompt>,
    /// References the view has expanded, and what each holds once it is known.
    #[serde(default)]
    pub expanded: BTreeSet<u64>,
    #[serde(default)]
    pub values: BTreeMap<u64, Vec<Variable>>,
    /// A request is in flight: the toolbar shows the program as running.
    #[serde(default)]
    pub busy: bool,
    /// Why the last request failed, shown in the view rather than swallowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The launch configurations read from `.vscode/launch.json`, by name.
    #[serde(default)]
    pub configs: Vec<Config>,
    /// The configuration the view will start.
    #[serde(default)]
    pub config: usize,
}

/// What the view is asking for in a one-line box.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PromptKind {
    /// A new watch expression.
    Watch,
    /// The condition of the breakpoint at `path`:`line`.
    Condition { path: String, line: u32 },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub kind: PromptKind,
    pub value: String,
}
impl Prompt {
    /// What the box says it wants, as VS Code labels it.
    pub fn label(&self) -> &'static str {
        match self.kind {
            PromptKind::Watch => "Expression to watch",
            PromptKind::Condition { .. } => "Break when expression is true",
        }
    }
}

/// A launch configuration, as `.vscode/launch.json` holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub name: String,
    /// `python` or `node`.
    pub kind: String,
    /// The program, relative to the workspace or absolute; empty means the active file.
    pub program: String,
    #[serde(default)]
    pub stop_on_entry: bool,
}

impl Debug {
    /// The breakpoint at a line, if there is one.
    pub fn at(&self, path: &str, line: u32) -> Option<&Breakpoint> {
        self.breakpoints
            .iter()
            .find(|b| b.path == path && b.line == line)
    }
    pub fn index_at(&self, path: &str, line: u32) -> Option<usize> {
        self.breakpoints
            .iter()
            .position(|b| b.path == path && b.line == line)
    }
    /// Add a breakpoint, or take away the one that is there. Returns whether one is
    /// there now.
    pub fn toggle(&mut self, path: &str, line: u32) -> Result<bool, String> {
        if let Some(i) = self.index_at(path, line) {
            self.breakpoints.remove(i);
            return Ok(false);
        }
        if self.breakpoints.len() >= BREAKPOINT_LIMIT {
            return Err(format!("at most {BREAKPOINT_LIMIT} breakpoints"));
        }
        self.breakpoints.push(Breakpoint {
            path: path.to_owned(),
            line,
            condition: None,
            enabled: true,
            verified: None,
        });
        self.breakpoints
            .sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        Ok(true)
    }
    /// The breakpoints of one file, in line order, as the protocol sends them.
    pub fn sources(&self, path: &str) -> Vec<SourceBreakpoint> {
        self.breakpoints
            .iter()
            .filter(|b| b.path == path && b.enabled)
            .map(Breakpoint::source)
            .collect()
    }
    /// Every enabled breakpoint, whatever file it is in.
    pub fn all_sources(&self) -> Vec<SourceBreakpoint> {
        self.breakpoints
            .iter()
            .filter(|b| b.enabled)
            .map(Breakpoint::source)
            .collect()
    }
    pub fn say(&mut self, kind: ConsoleKind, text: impl Into<String>) {
        let text = text.into();
        for line in text.trim_end_matches('\n').split('\n') {
            self.console.push(ConsoleLine {
                kind,
                text: line.to_owned(),
            });
        }
        if self.console.len() > CONSOLE_LIMIT {
            let over = self.console.len() - CONSOLE_LIMIT;
            self.console.drain(..over);
        }
    }
    /// Forget everything that belonged to the session that just ended.
    pub fn clear_session(&mut self) {
        self.session = None;
        self.values.clear();
        self.expanded.clear();
        self.busy = false;
        for watch in &mut self.watches {
            watch.value = None;
            watch.failed = false;
            watch.reference = 0;
        }
    }
}

/// The file VS Code keeps launch configurations in, under the workspace folder.
pub const LAUNCH_JSON: &str = ".vscode/launch.json";

/// A `launch.json` for a workspace, with a configuration for the program named.
pub fn launch_json(configs: &[Config]) -> String {
    let mut out = String::from(
        "{\n  // Run and Debug configurations. https://go.microsoft.com/fwlink/?linkid=830387\n  \"version\": \"0.2.0\",\n  \"configurations\": [\n",
    );
    for (i, c) in configs.iter().enumerate() {
        let (kind, request) = (&c.kind, "launch");
        out.push_str("    {\n");
        out.push_str(&format!("      \"name\": \"{}\",\n", c.name));
        out.push_str(&format!("      \"type\": \"{kind}\",\n"));
        out.push_str(&format!("      \"request\": \"{request}\",\n"));
        out.push_str(&format!("      \"program\": \"{}\",\n", c.program));
        out.push_str(&format!(
            "      \"stopOnEntry\": {},\n",
            if c.stop_on_entry { "true" } else { "false" }
        ));
        out.push_str("      \"console\": \"integratedTerminal\"\n");
        out.push_str(if i + 1 == configs.len() {
            "    }\n"
        } else {
            "    },\n"
        });
    }
    out.push_str("  ]\n}\n");
    out
}

/// The configurations in a `launch.json`. Unparseable JSON gives none rather than a
/// guess: a view must not invent a configuration nobody wrote.
pub fn parse_launch_json(text: &str) -> Vec<Config> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&strip_comments(text)) else {
        return vec![];
    };
    let Some(list) = value.get("configurations").and_then(|c| c.as_array()) else {
        return vec![];
    };
    list.iter()
        .filter_map(|c| {
            let kind = match c.get("type").and_then(|v| v.as_str())? {
                "python" | "debugpy" => "python",
                "node" | "pwa-node" => "node",
                _ => return None,
            };
            Some(Config {
                name: c.get("name").and_then(|v| v.as_str())?.to_owned(),
                kind: kind.to_owned(),
                program: c
                    .get("program")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                stop_on_entry: c
                    .get("stopOnEntry")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

/// `launch.json` is JSON with comments, as VS Code writes it.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

// ----- what the workbench does with all that -------------------------------------------

use super::{basename, syntax::Language, Focus, TabKind, Workbench};
use crate::AppEffect;
use cw_protocol::debug::{Launch, Reply, Request, Step};

impl Workbench {
    /// VS Code's launch variables, expanded the way VS Code expands them. A variable
    /// nobody here defines is left standing, so a configuration that names one is
    /// visibly wrong rather than quietly pointing somewhere else.
    pub fn expand_variables(&self, text: &str) -> String {
        let workspace = self.workspace().unwrap_or(&self.home).to_owned();
        let file = self
            .active_tab()
            .filter(|t| t.kind == TabKind::File)
            .map(|t| t.path.clone())
            .unwrap_or_default();
        let dirname = |p: &str| match p.rsplit_once('/') {
            Some((head, _)) if !head.is_empty() => head.to_owned(),
            Some(_) => "/".to_owned(),
            None => String::new(),
        };
        let base = basename(&file).to_owned();
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}').map(|i| start + i) else {
                break;
            };
            let name = &rest[start + 2..end];
            let value = match name {
                "workspaceFolder" | "cwd" => Some(workspace.clone()),
                "workspaceFolderBasename" => Some(basename(&workspace).to_string()),
                "file" => Some(file.clone()),
                "fileBasename" => Some(base.clone()),
                "fileBasenameNoExtension" => Some(
                    base.rsplit_once('.')
                        .map_or(base.clone(), |(stem, _)| stem.to_owned()),
                ),
                "fileExtname" => Some(
                    base.rsplit_once('.')
                        .map_or(String::new(), |(_, ext)| format!(".{ext}")),
                ),
                "fileDirname" => Some(dirname(&file)),
                "relativeFile" => Some(self.rel(&file).unwrap_or_else(|| file.clone())),
                "relativeFileDirname" => {
                    Some(self.rel(&dirname(&file)).unwrap_or_else(|| dirname(&file)))
                }
                "userHome" => Some(self.home.clone()),
                "pathSeparator" | "/" => Some("/".to_owned()),
                "lineNumber" => self
                    .active_tab()
                    .map(|t| (t.doc.text[..t.doc.cursor].matches('\n').count() + 1).to_string()),
                // `${env:NAME}` would need the machine's environment, which this view
                // does not have; it is left standing rather than guessed at.
                _ => None,
            };
            match value {
                Some(v) => out.push_str(&v),
                None => out.push_str(&rest[start..=end]),
            }
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        out
    }

    /// The configuration that would run now: the one chosen in the view, or the active
    /// file, which is what VS Code offers before a `launch.json` exists.
    pub fn debug_config(&self) -> Result<Config, String> {
        if let Some(c) = self.debug.configs.get(self.debug.config) {
            if !c.program.is_empty() {
                return Ok(c.clone());
            }
        }
        let tab = self
            .active_tab()
            .filter(|t| t.kind == TabKind::File)
            .ok_or("Open a Python or JavaScript file to debug it")?;
        let kind = match tab.lang {
            Language::Python => "python",
            Language::JavaScript | Language::TypeScript => "node",
            other => {
                return Err(format!(
                    "{} files cannot be debugged; only Python and JavaScript can",
                    other.name()
                ))
            }
        };
        Ok(Config {
            name: format!("Debug {}", basename(&tab.path)),
            kind: kind.to_owned(),
            program: tab.path.clone(),
            stop_on_entry: self
                .debug
                .configs
                .get(self.debug.config)
                .is_some_and(|c| c.stop_on_entry),
        })
    }
    /// A request for the machine's debugger. Only a request that moves the program
    /// makes it look like it is running; asking what a value holds does not.
    fn ask(&mut self, window: u64, tag: &str, request: Request) -> Vec<AppEffect> {
        self.debug.busy = matches!(
            request,
            Request::Launch(_) | Request::Resume { .. } | Request::Pause { .. }
        );
        self.debug.error = None;
        vec![AppEffect::Debug {
            window,
            tag: tag.to_owned(),
            request,
        }]
    }
    /// Every unsaved file in the active editor group, written before a launch. This is
    /// VS Code's `debug.saveBeforeStart` default, and without it a debugger would run
    /// yesterday's bytes while today's are on screen.
    fn save_group_before_debug(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let group = self.active_tab().map(|t| t.group);
        let dirty: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == super::TabKind::File && t.dirty() && Some(t.group) == group)
            .map(|(i, _)| i)
            .collect();
        let mut effects = vec![];
        for index in dirty {
            effects.extend(self.save_tab(window, index)?);
        }
        Ok(effects)
    }
    /// F5: start the program, or carry on from where it stopped.
    pub(super) fn debug_start(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        if self.debug.session.as_ref().is_some_and(|s| !s.ended) {
            return self.debug_resume(window, Step::Continue);
        }
        // A file no debugger can take (a shell script) is still run, as F5 always has.
        let config = match self.debug_config() {
            Ok(config) => config,
            Err(why) => {
                self.notice = Some(why);
                return self.run_active(window);
            }
        };
        let expanded = self.expand_variables(&config.program);
        let program = if expanded.starts_with('/') || expanded.contains(":\\") {
            expanded
        } else {
            self.abs(&expanded)
        };
        let cwd = self.workspace().unwrap_or(&self.home).to_owned();
        // `debug.saveBeforeStart` defaults to saving the editors in the active group,
        // so the debugger runs the program on screen rather than an older file.
        let mut effects = self.save_group_before_debug(window)?;
        self.view = super::View::Run;
        self.sidebar = true;
        // `debug.internalConsoleOptions` opens the Debug Console on the first session,
        // which is also what puts the panel on screen.
        self.panel_open = true;
        self.panel = super::PanelTab::Debug;
        self.debug.console.clear();
        self.debug
            .say(ConsoleKind::Notice, format!("Starting {program}"));
        // This is the session being asked for; the machine's reply gives it its id.
        self.debug.session = Some(Session {
            id: 0,
            kind: config.kind.clone(),
            program: program.clone(),
            stopped: None,
            frames: vec![],
            scopes: vec![],
            frame: 0,
            ended: false,
        });
        effects.extend(self.ask(
            window,
            "launch",
            Request::Launch(Launch {
                kind: config.kind.clone(),
                program,
                cwd,
                args: vec![],
                stop_on_entry: config.stop_on_entry,
                breakpoints: self.debug.all_sources(),
                exceptions: self.debug.exceptions,
            }),
        ));
        Ok(effects)
    }
    /// Continue, Step Over, Step Into, Step Out.
    pub(super) fn debug_resume(
        &mut self,
        window: u64,
        step: Step,
    ) -> Result<Vec<AppEffect>, String> {
        let session = self.session_id()?;
        Ok(self.ask(window, "resume", Request::Resume { session, step }))
    }
    pub(super) fn debug_pause(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let session = self.session_id()?;
        Ok(self.ask(window, "resume", Request::Pause { session }))
    }
    /// Stop: the program is killed and the session forgotten.
    pub(super) fn debug_stop(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let session = self.session_id()?;
        Ok(self.ask(window, "terminate", Request::Terminate { session }))
    }
    /// Restart: stop what is running, and start it again when the machine says it is
    /// gone (the reply under `restart` starts the next one).
    pub(super) fn debug_restart(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let session = self.session_id()?;
        Ok(self.ask(window, "restart", Request::Terminate { session }))
    }
    fn session_id(&self) -> Result<u64, String> {
        self.debug
            .session
            .as_ref()
            .map(|s| s.id)
            .ok_or_else(|| "Nothing is being debugged".to_owned())
    }
    /// Toggle a breakpoint on one line of a file, and tell a running session about it.
    pub(super) fn breakpoint_at(
        &mut self,
        window: u64,
        path: &str,
        line: u32,
    ) -> Result<Vec<AppEffect>, String> {
        self.debug.toggle(path, line)?;
        Ok(self.send_breakpoints(window, path))
    }
    /// The breakpoints of one file, sent to a session that is running.
    pub(super) fn send_breakpoints(&mut self, window: u64, path: &str) -> Vec<AppEffect> {
        let Some(session) = self
            .debug
            .session
            .as_ref()
            .filter(|s| !s.ended)
            .map(|s| s.id)
        else {
            return vec![];
        };
        let points = self.debug.sources(path);
        self.ask(
            window,
            "breakpoints",
            Request::Breakpoints {
                session,
                path: path.to_owned(),
                points,
            },
        )
    }
    /// Ask for what a value holds, the first time it is expanded.
    pub(super) fn debug_expand(&mut self, window: u64, reference: u64) -> Vec<AppEffect> {
        if !self.debug.expanded.insert(reference) {
            self.debug.expanded.remove(&reference);
            return vec![];
        }
        let Some(session) = self.debug.session.as_ref().map(|s| s.id) else {
            return vec![];
        };
        if self.debug.values.contains_key(&reference) {
            return vec![];
        }
        self.ask(
            window,
            &format!("vars:{reference}"),
            Request::Variables { session, reference },
        )
    }
    /// Evaluate an expression in the frame the view is showing: the Debug Console's
    /// input (`repl`) and the watch list (`watch`).
    pub(super) fn debug_evaluate(
        &mut self,
        window: u64,
        expression: &str,
        context: &str,
        tag: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let session = self.session_id()?;
        let frame = self.debug.session.as_ref().map_or(0, Session::frame_id);
        Ok(self.ask(
            window,
            tag,
            Request::Evaluate {
                session,
                frame,
                expression: expression.to_owned(),
                context: context.to_owned(),
            },
        ))
    }
    /// Evaluate every watch expression again, wherever the program has stopped.
    fn refresh_watches(&mut self, window: u64) -> Vec<AppEffect> {
        let mut effects = vec![];
        let expressions: Vec<(usize, String)> = self
            .debug
            .watches
            .iter()
            .enumerate()
            .map(|(i, w)| (i, w.expression.clone()))
            .collect();
        for (i, expression) in expressions {
            if let Ok(e) = self.debug_evaluate(window, &expression, "watch", &format!("watch:{i}"))
            {
                effects.extend(e);
            }
        }
        effects
    }
    /// What the machine answered. Everything the view shows of a session comes through
    /// here, so it can never show a stop the machine did not report.
    pub fn debug_reply(
        &mut self,
        window: u64,
        tag: &str,
        reply: Result<Reply, String>,
    ) -> Vec<AppEffect> {
        // Only the reply to a request that moved the program says it is not running.
        if matches!(tag, "launch" | "resume" | "restart" | "terminate") {
            self.debug.busy = false;
        }
        let reply = match reply {
            Ok(reply) => reply,
            Err(reason) => {
                self.debug.error = Some(reason.clone());
                self.debug.say(ConsoleKind::Error, reason.clone());
                if tag == "launch" {
                    // The machine has no debugger for this program. Say so, and run it
                    // the way Run Without Debugging would, rather than doing nothing.
                    self.debug.clear_session();
                    self.notice = Some(format!("{reason}; running without debugging"));
                    self.debug.say(
                        ConsoleKind::Error,
                        "Running without debugging: the program's output is in the terminal",
                    );
                    return self.run_active(window).unwrap_or_default();
                }
                if tag == "terminate" || tag == "restart" {
                    self.debug.clear_session();
                }
                return vec![];
            }
        };
        match (tag, reply) {
            ("launch", Reply::Launched { session, state }) => {
                if let Some(live) = &mut self.debug.session {
                    live.id = session;
                }
                self.settle(window, state)
            }
            ("resume", Reply::Stopped { state }) | ("launch", Reply::Stopped { state }) => {
                self.settle(window, state)
            }
            ("breakpoints", Reply::Breakpoints { verified }) => {
                // The adapter answers in the order the breakpoints were sent.
                for (i, point) in self
                    .debug
                    .breakpoints
                    .iter_mut()
                    .filter(|b| b.enabled)
                    .enumerate()
                {
                    if let Some(ok) = verified.get(i) {
                        point.verified = Some(*ok);
                    }
                }
                vec![]
            }
            ("terminate" | "restart", Reply::Terminated) => {
                self.debug.say(ConsoleKind::Notice, "Debug session ended");
                self.debug.clear_session();
                if tag == "restart" {
                    return self.debug_start(window).unwrap_or_default();
                }
                vec![]
            }
            (tag, Reply::Variables { variables }) if tag.starts_with("vars:") => {
                if let Ok(reference) = tag.trim_start_matches("vars:").parse::<u64>() {
                    self.debug.values.insert(reference, variables);
                }
                vec![]
            }
            (tag, Reply::Evaluated { result }) if tag.starts_with("watch:") => {
                if let Ok(i) = tag.trim_start_matches("watch:").parse::<usize>() {
                    if let Some(watch) = self.debug.watches.get_mut(i) {
                        watch.value = Some(result.value);
                        watch.reference = result.reference;
                        watch.failed = false;
                    }
                }
                vec![]
            }
            ("repl", Reply::Evaluated { result }) => {
                self.debug.say(ConsoleKind::Result, result.value);
                vec![]
            }
            _ => vec![],
        }
    }
    /// Take in where the program stopped: the call stack, the output it wrote, the
    /// editor showing the line, and the watches evaluated again.
    fn settle(&mut self, window: u64, state: cw_protocol::debug::State) -> Vec<AppEffect> {
        if !state.output.is_empty() {
            self.debug.say(ConsoleKind::Output, state.output.clone());
        }
        let ended = state.stopped.is_exit();
        if let Some(session) = &mut self.debug.session {
            session.frames = state.frames.clone();
            session.scopes = state.scopes.clone();
            session.frame = 0;
            session.stopped = Some(state.stopped.clone());
            session.ended = ended;
        }
        self.debug.values.clear();
        self.debug.expanded.clear();
        if ended {
            self.debug.say(ConsoleKind::Notice, state.stopped.label());
            self.problems_from_console();
            return vec![];
        }
        if let cw_protocol::debug::Stopped::Exception { text } = &state.stopped {
            self.debug.say(ConsoleKind::Error, text.clone());
        }
        let mut effects = vec![];
        // The editor follows the program: the frame's file, at its line.
        if let Some((path, line)) = self.debug.session.as_ref().and_then(Session::at) {
            effects.extend(self.open_file(window, &path, false, Some((line as usize, 1, 0))));
        }
        // Locals open by default, as they are in VS Code.
        let locals = self.debug.session.as_ref().and_then(|s| {
            s.scopes
                .iter()
                .find(|sc| !sc.expensive)
                .map(|sc| sc.reference)
        });
        if let Some(reference) = locals {
            effects.extend(self.debug_expand(window, reference));
        }
        effects.extend(self.refresh_watches(window));
        effects
    }
    /// A program that ended under the debugger reports what it printed the same way a
    /// program run in the terminal does: a traceback becomes a problem at its line.
    fn problems_from_console(&mut self) {
        let Some(session) = self.debug.session.as_ref() else {
            return;
        };
        let runner = match session.kind.as_str() {
            "python" => "python3",
            "node" => "node",
            _ => return,
        };
        let text: String = self
            .debug
            .console
            .iter()
            .filter(|line| line.kind == ConsoleKind::Output)
            .map(|line| format!("{}\n", line.text))
            .collect();
        let cwd = self.workspace().unwrap_or(&self.home).to_owned();
        let found = super::problems::from_output(runner, "", &text, &cwd);
        self.problems.retain(|p| p.source == "json");
        self.problems.extend(found);
    }
    /// The file and 1-based line the caret is on: what a breakpoint command acts on.
    pub(super) fn caret_line(&self) -> Result<(String, u32), String> {
        let tab = self
            .active_tab()
            .filter(|t| t.kind == TabKind::File)
            .ok_or("Open a file to set a breakpoint in")?;
        Ok((tab.path.clone(), tab.doc.line_of(tab.doc.cursor) as u32 + 1))
    }
    /// Which exceptions stop the program, told to a session that is running.
    pub(super) fn send_exceptions(&mut self, window: u64) -> Vec<AppEffect> {
        let Some(session) = self
            .debug
            .session
            .as_ref()
            .filter(|s| !s.ended)
            .map(|s| s.id)
        else {
            return vec![];
        };
        let filters = self.debug.exceptions;
        self.ask(
            window,
            "exceptions",
            Request::Exceptions { session, filters },
        )
    }
    /// Show another frame of the call stack: its file, its line, and its variables.
    pub(super) fn select_frame(
        &mut self,
        window: u64,
        frame: usize,
    ) -> Result<Vec<AppEffect>, String> {
        let session = self
            .debug
            .session
            .as_mut()
            .ok_or("Nothing is being debugged")?;
        if frame >= session.frames.len() {
            return Err("no such stack frame".into());
        }
        session.frame = frame;
        let mut effects = vec![];
        if let Some((path, line)) = self.debug.session.as_ref().and_then(Session::at) {
            effects.extend(self.open_file(window, &path, false, Some((line as usize, 1, 0))));
        }
        // The variables and the watches belong to the frame that is shown.
        self.debug.values.clear();
        self.debug.expanded.clear();
        let locals = self.debug.session.as_ref().and_then(|s| {
            s.scopes
                .iter()
                .find(|sc| !sc.expensive)
                .map(|sc| sc.reference)
        });
        if let Some(reference) = locals {
            effects.extend(self.debug_expand(window, reference));
        }
        effects.extend(self.refresh_watches(window));
        Ok(effects)
    }
    /// Enter in the Debug Console's input, or in whichever box is open.
    pub(super) fn debug_commit(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        if let Some(prompt) = self.debug.prompt.take() {
            let value = prompt.value.trim().to_owned();
            self.focus = Focus::Editor;
            return match prompt.kind {
                PromptKind::Watch => {
                    if value.is_empty() {
                        return Ok(vec![]);
                    }
                    if self.debug.watches.len() >= WATCH_LIMIT {
                        return Err(format!("at most {WATCH_LIMIT} watch expressions"));
                    }
                    self.debug.watches.push(Watch {
                        expression: value.clone(),
                        ..Watch::default()
                    });
                    let i = self.debug.watches.len() - 1;
                    if self.debug.session.as_ref().is_some_and(|s| !s.ended) {
                        return self.debug_evaluate(window, &value, "watch", &format!("watch:{i}"));
                    }
                    Ok(vec![])
                }
                PromptKind::Condition { path, line } => {
                    if self.debug.index_at(&path, line).is_none() {
                        self.debug.toggle(&path, line)?;
                    }
                    if let Some(i) = self.debug.index_at(&path, line) {
                        self.debug.breakpoints[i].condition =
                            (!value.is_empty()).then(|| value.clone());
                    }
                    Ok(self.send_breakpoints(window, &path))
                }
            };
        }
        let expression = self.debug.input.trim().to_owned();
        if expression.is_empty() {
            return Ok(vec![]);
        }
        self.debug.input.clear();
        self.debug.say(ConsoleKind::Input, expression.clone());
        if self.debug.session.as_ref().is_none_or(|s| s.ended) {
            self.debug
                .say(ConsoleKind::Error, "Nothing is being debugged");
            return Ok(vec![]);
        }
        self.debug_evaluate(window, &expression, "repl", "repl")
    }
    /// Write a `launch.json` for the workspace, as VS Code's "create a launch.json
    /// file" does, and open it.
    pub(super) fn create_launch_json(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let folder = self.workspace().ok_or("Open a folder first")?.to_owned();
        let config = self.debug_config()?;
        let program = config
            .program
            .strip_prefix(&format!("{folder}/"))
            .map_or(config.program.clone(), |rel| {
                format!("${{workspaceFolder}}/{rel}")
            });
        let configs = vec![Config {
            name: config.name.clone(),
            kind: config.kind.clone(),
            program,
            stop_on_entry: false,
        }];
        let path = format!("{folder}/{LAUNCH_JSON}");
        let content = launch_json(&configs);
        self.debug.configs = configs;
        self.debug.config = 0;
        let mut effects = vec![AppEffect::CreateDirectory {
            window,
            path: format!("{folder}/.vscode"),
        }];
        self.writes.push((path.clone(), content.clone()));
        effects.push(AppEffect::WriteFile {
            window,
            path: path.clone(),
            content,
        });
        effects.extend(self.open_file(window, &path, true, None));
        Ok(effects)
    }
}
