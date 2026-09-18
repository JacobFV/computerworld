//! The debugger hook both interpreters implement, shaped after the Debug Adapter
//! Protocol so a VS Code-style front end maps onto it one to one.
//!
//! A runtime started with a [`Debugger`] consults it at every new source line:
//! breakpoints (with conditions, hit counts and log messages), stepping and pause
//! requests make the runtime call [`Debugger::stopped`] with a [`DebugTarget`] — a
//! view of the stopped program (threads, frames, scopes, variables, evaluation in a
//! frame). The debugger inspects what it likes and answers with a [`Step`] saying
//! how to go on. [`Step::Suspend`] ends the run where it stands; the stateful
//! [`crate::dap::DapSession`] resumes it later by deterministic replay.
use std::collections::BTreeMap;

/// A line breakpoint, as DAP's `SourceBreakpoint`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceBreakpoint {
    pub line: u32,
    /// Stop only when this expression (in the program's language) is truthy.
    pub condition: Option<String>,
    /// Stop only on the Nth hit: `"3"`, `">= 3"`, `"% 2"` (every second hit).
    pub hit_condition: Option<String>,
    /// A logpoint: print this (with `{expr}` interpolated) instead of stopping.
    pub log_message: Option<String>,
}

/// Which exceptions stop the program (DAP exception breakpoint filters
/// `raised` and `uncaught`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExceptionFilters {
    pub raised: bool,
    pub uncaught: bool,
}

/// Everything that decides where a debuggee stops.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DebugConfig {
    /// Absolute source path -> its breakpoints.
    pub breakpoints: BTreeMap<String, Vec<SourceBreakpoint>>,
    pub exceptions: ExceptionFilters,
    pub stop_on_entry: bool,
    /// Stop with reason `pause` after this many instructions without another stop
    /// (DAP `pause` for a program that would otherwise run away). `None`: never.
    pub pause_after: Option<u64>,
}
impl DebugConfig {
    /// Breakpoint ids are positional: path order, then line order within a path,
    /// counting from 1. They are stable while the configuration is.
    pub fn breakpoint_id(&self, path: &str, line: u32) -> Option<u64> {
        let mut id = 0;
        for (p, bps) in &self.breakpoints {
            for bp in bps {
                id += 1;
                if p == path && bp.line == line {
                    return Some(id);
                }
            }
        }
        None
    }
    pub fn breakpoints_at(&self, path: &str, line: u32) -> Vec<&SourceBreakpoint> {
        self.breakpoints
            .get(path)
            .map(|v| v.iter().filter(|b| b.line == line).collect())
            .unwrap_or_default()
    }
    pub fn has_breakpoints_in(&self, path: &str) -> bool {
        self.breakpoints.get(path).is_some_and(|v| !v.is_empty())
    }
}

/// Evaluates a DAP hit condition against the hit count (1-based).
pub fn hit_condition_met(cond: &str, hits: u64) -> bool {
    let c = cond.trim();
    let (op, rest) = if let Some(r) = c.strip_prefix(">=") {
        (">=", r)
    } else if let Some(r) = c.strip_prefix("<=") {
        ("<=", r)
    } else if let Some(r) = c.strip_prefix("==") {
        ("==", r)
    } else if let Some(r) = c.strip_prefix('>') {
        (">", r)
    } else if let Some(r) = c.strip_prefix('<') {
        ("<", r)
    } else if let Some(r) = c.strip_prefix('%') {
        ("%", r)
    } else if let Some(r) = c.strip_prefix('=') {
        ("==", r)
    } else {
        ("==", c)
    };
    let Ok(n) = rest.trim().parse::<u64>() else {
        return true;
    };
    match op {
        ">=" => hits >= n,
        "<=" => hits <= n,
        ">" => hits > n,
        "<" => hits < n,
        "%" => n != 0 && hits.is_multiple_of(n),
        _ => hits == n,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Entry,
    Breakpoint,
    Step,
    Pause,
    Exception,
}
impl StopReason {
    /// DAP's `StoppedEvent.reason` string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Breakpoint => "breakpoint",
            Self::Step => "step",
            Self::Pause => "pause",
            Self::Exception => "exception",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopEvent {
    pub reason: StopReason,
    pub thread_id: u64,
    /// For exceptions: `"ZeroDivisionError"` / `"TypeError"`.
    pub description: String,
    /// For exceptions: the message.
    pub text: String,
    pub hit_breakpoint_ids: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    pub id: u64,
    pub name: String,
    /// Absolute path of the source (`<string>`/`[eval]` for inline code).
    pub path: String,
    /// 1-based line and column.
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// `Locals`, `Globals`, `Closure`, `Module`...
    pub name: String,
    pub variables_reference: u64,
    pub expensive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    /// The value as the language's REPL shows it (`repr` / `util.inspect`).
    pub value: String,
    pub type_name: String,
    /// Non-zero when the value has children to expand with [`DebugTarget::variables`].
    pub variables_reference: u64,
    /// Number of named / indexed children (DAP paging hints).
    pub named_variables: u64,
    pub indexed_variables: u64,
}

/// How a stopped program goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Continue,
    /// Step over: the next line in this frame (or its caller when it returns).
    Next,
    /// Step into the next call, else like `Next`.
    StepIn,
    /// Run until the current frame returns.
    StepOut,
    /// End the run here, keeping everything it did; resumed later by replay.
    Suspend,
    /// Kill the debuggee.
    Terminate,
}
impl Step {
    pub fn tag(self) -> u8 {
        match self {
            Self::Continue => 0,
            Self::Next => 1,
            Self::StepIn => 2,
            Self::StepOut => 3,
            Self::Suspend => 4,
            Self::Terminate => 5,
        }
    }
    pub fn from_tag(t: u8) -> Self {
        match t {
            0 => Self::Continue,
            1 => Self::Next,
            2 => Self::StepIn,
            3 => Self::StepOut,
            5 => Self::Terminate,
            _ => Self::Suspend,
        }
    }
}

/// A stopped program, as the debugger sees it. Frame ids and variable references
/// are valid until the program resumes.
pub trait DebugTarget {
    fn threads(&mut self) -> Vec<Thread>;
    /// Innermost frame first.
    fn stack_trace(&mut self, thread_id: u64) -> Vec<StackFrame>;
    fn scopes(&mut self, frame_id: u64) -> Vec<Scope>;
    fn variables(&mut self, variables_reference: u64) -> Vec<Variable>;
    /// Evaluates an expression in a frame (the top frame of the stopped thread when
    /// `None`). Side effects are real, as in a debug console.
    fn evaluate(&mut self, expression: &str, frame_id: Option<u64>) -> Result<Variable, String>;
    /// Assigns `value` (an expression) to the child `name` of a container or scope.
    fn set_variable(
        &mut self,
        variables_reference: u64,
        name: &str,
        value: &str,
    ) -> Result<Variable, String>;
}

/// The front end a runtime reports to.
pub trait Debugger {
    fn config(&self) -> &DebugConfig;
    /// The program stopped; inspect it and say how to continue.
    fn stopped(&mut self, event: &StopEvent, target: &mut dyn DebugTarget) -> Step;
    /// Text a logpoint produced.
    fn log(&mut self, text: &str) {
        let _ = text;
    }
}

/// What a runtime's debug run returns besides its [`crate::Outcome`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DebugRunInfo {
    /// The run ended because the debugger answered [`Step::Suspend`].
    pub suspended: bool,
    /// The debugger answered [`Step::Terminate`].
    pub terminated: bool,
}

/// Formats a logpoint message: `{expr}` segments are evaluated, `{{`/`}}` escape.
pub fn format_log_message(msg: &str, mut eval: impl FnMut(&str) -> String) -> String {
    let mut out = String::new();
    let chars: Vec<char> = msg.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => {
                out.push('{');
                i += 2;
            }
            '}' if chars.get(i + 1) == Some(&'}') => {
                out.push('}');
                i += 2;
            }
            '{' => {
                let mut depth = 1;
                let mut j = i + 1;
                while j < chars.len() {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let expr: String = chars[i + 1..j.min(chars.len())].iter().collect();
                out.push_str(&eval(&expr));
                i = j + 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hit_conditions() {
        assert!(hit_condition_met("3", 3));
        assert!(!hit_condition_met("3", 2));
        assert!(hit_condition_met(">= 2", 5));
        assert!(hit_condition_met("% 2", 4));
        assert!(!hit_condition_met("% 2", 3));
    }
    #[test]
    fn log_messages() {
        assert_eq!(
            format_log_message("x={x} {{lit}}", |e| format!("<{e}>")),
            "x=<x> {lit}"
        );
    }
}
