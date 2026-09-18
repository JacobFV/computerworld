# Debugging programs in a world

The simulated `python3` and `node` can be run under a debugger. The interface is
shaped after the [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/)
(DAP), so a VS Code-style front end — or an agent that speaks DAP — maps onto it
one request at a time: set breakpoints, launch, inspect the stopped program,
step, continue.

Nothing about it leaves the simulation. The debuggee is the same interpreter the
shell runs, on the same VFS, the same clock and the same seeded entropy, and a
debugged run produces the same output as an undebugged one.

## The pieces

| Where | What |
| --- | --- |
| `cw_script_host::debug` | The contract: `Debugger` (the front end), `DebugTarget` (the stopped program), `DebugConfig`, `SourceBreakpoint`, `StopEvent`, `Step`. |
| `cw_pyvm::run_debug`, `cw_jsvm::run_debug` | Run a program with a `Debugger` attached. |
| `cw_script_host::dap` | `DapSession`: a stateful session that turns DAP requests into runs of the program, and its `DebugRunner` trait. |
| `cw_computer::debugger::MachineDebugger` | A `DebugRunner` that runs the program on a machine of the world. |

## How a stopped program is possible

An interpreter cannot be kept alive between two actions of the world: its heap is
not serializable, and a snapshot may be restored anywhere. A session therefore
never holds a paused interpreter. It **replays**: each request runs the program
again from the start, with a script of what was answered at every earlier stop, and
stops again at the same place. Host calls the earlier run made — files written,
requests sent, the clock, randomness — are answered from a journal recorded the
first time rather than made again, so replaying does not write a file twice or
send a request twice.

Two consequences are worth knowing:

* Sessions survive snapshots: everything a session needs (the journal, the script
  of stops, the breakpoints) is plain data.
* Inspecting is not free. A `variables` request re-runs the program to the stop it
  names. Programs in a world are small, and the interpreter is fast, but a session
  that stops a million times into a long run is not what this is for.

A run that a front end suspends (`Step::Suspend`) ends where it stands and keeps
what it did; the session resumes it by replay when the next request arrives.

## Stopping

`DebugConfig` decides where a program stops:

* **Breakpoints** per source path and line, with a `condition` (an expression in
  the program's own language), a `hit_condition` (`"3"`, `">= 3"`, `"% 2"`), or a
  `log_message` — a logpoint, which prints `{expr}` interpolations through
  `Debugger::log` instead of stopping.
* **Exception filters**: `raised` stops where an exception is raised, `uncaught`
  only when nothing handles it.
* **`stop_on_entry`**: stop before the program's first line. The interpreter's own
  setup (the frozen stdlib modules it imports, Node's internal modules) is not the
  program and never stops.
* **`pause_after`**: stop with reason `pause` after that many instructions. This is
  how a front end interrupts a program that would otherwise run away; runs here are
  synchronous, so DAP's asynchronous `pause` request arms this for the next resume.

At a stop the front end gets a `StopEvent` and a `DebugTarget`, and answers with a
`Step`: `Continue`, `Next` (over), `StepIn`, `StepOut`, `Suspend` or `Terminate`.

## Looking at the stopped program

`DebugTarget` is DAP's inspection surface:

* `threads()` — the Python scheduler's threads (`MainThread`, `Thread-1`, …), or
  the one thread Node has.
* `stack_trace(thread)` — frames innermost first, with the absolute source path and
  a 1-based line and column.
* `scopes(frame)` — `Locals` and `Globals`.
* `variables(reference)` — a scope's names, or the children of a value: a list's
  items, a dict's entries, an object's properties. Values read as the language's
  console shows them (`repr()` in Python, `util.inspect` in Node).
* `evaluate(expression, frame)` — runs an expression where the program stands. In
  Python it runs with that frame's globals and locals; in Node the frame's
  bindings (its own, what it captured, and the frames around it) are in scope.
  Side effects are real, as in a debug console.
* `set_variable(reference, name, value)` — evaluates `value` and stores it in that
  local, global or container. The program goes on with the new value.

## Driving it from a session

`DapSession::handle(request, runner)` takes the DAP requests a front end sends and
returns a response and the events that came with it (`Initialized`, `Stopped`,
`Output`, `Exited`, `Terminated`). The requests it answers:

`Initialize`, `SetBreakpoints`, `SetExceptionBreakpoints`, `Launch`,
`ConfigurationDone`, `Threads`, `StackTrace`, `Scopes`, `Variables`, `Evaluate`,
`SetVariable`, `Continue`, `Next`, `StepIn`, `StepOut`, `Pause`, `Terminate`,
`Disconnect`.

Its `capabilities()` report what that amounts to: configuration-done, conditional
breakpoints, hit-conditional breakpoints, logpoints, evaluate-for-hovers,
set-variable, terminate, and the two exception filters.

A session against a machine of a world:

```rust
use cw_computer::debugger::MachineDebugger;
use cw_script_host::dap::{DapSession, Request};
use cw_script_host::debug::SourceBreakpoint;

let mut session = DapSession::new();
let command = vec!["python3".to_string(), "main.py".to_string()];
let mut runner = MachineDebugger::for_command(&mut computer, &mut shell, tick, &command)
    .expect("a runtime this machine debugs");

session.handle(Request::Initialize, &mut runner);
session.handle(
    Request::SetBreakpoints {
        path: "/home/user/main.py".into(),
        breakpoints: vec![SourceBreakpoint { line: 5, ..Default::default() }],
    },
    &mut runner,
);
session.handle(Request::Launch { stop_on_entry: false }, &mut runner);
let (_, events) = session.handle(Request::ConfigurationDone, &mut runner);
// events contains Stopped { reason: "breakpoint", .. }
```

`crates/computer/tests/debug_session.rs` walks a whole session this way, including
changing a variable at a stop and watching the program print the new value.

## What is not there

* No data breakpoints, no function breakpoints, no `gotoTargets`, no
  `completions`, no `disassemble`, no source-map support (the simulated runtimes
  run the program's own source, so there is nothing to map).
* No `restartFrame`, and no "run to cursor" beyond what breakpoints give.
* Stepping is per source line. Python's threads all stop together — stopping one
  thread while the others run is not modelled.
* A front end that wants a `stopped` event *while* the program runs cannot have
  one: runs are synchronous, so `Pause` arms `pause_after` for the next resume
  instead.
* The debuggee's own `breakpoint()` (Python) and `debugger` (JavaScript)
  statements do not stop the program.
