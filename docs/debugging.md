# Debugging a program in the world

The simulated `python3` and `node` can be run under a debugger. The interface is shaped
after the [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/)
(DAP), so a VS Code-style front end — or an agent that speaks DAP — maps onto it one
request at a time: set breakpoints, launch, inspect the stopped program, step, continue.

Nothing about it leaves the simulation. The debuggee is the same interpreter the shell
runs, on the same VFS, the same clock and the same seeded entropy, and a debugged run
produces the same output as an undebugged one.

Visual Studio Code's **Run and Debug** view talks to the machine the program runs on.
Nothing in the editor executes anything: it sends requests and paints what comes back,
so what a person sees stopped on a line is only ever what the machine said.

```
Run and Debug view ── AppEffect::Debug { tag, request } ──▶ environment
                                                             │
                                          Runtime::debug ────┤ (kernel: records the event)
                                                             ▼
                                        Computer::debug ── DebugAdapter for the runtime
                                                             │           │
     DesktopState::debug_reply ◀── Result<Reply, String> ─────┘           ▼
                                                              DapSession ─ MachineDebugger
                                                                          (replays the run)
```

## The words

`cw_protocol::debug` holds the Debug Adapter Protocol's vocabulary as this world's own
types:

| Type | What it is |
|---|---|
| `Request::Launch(Launch)` | Start `program` with `kind` (`python`, `node`), `cwd`, `stop_on_entry`, the breakpoints that are set and the exception filters |
| `Request::Breakpoints` | Replace the breakpoints of one file while the program runs |
| `Request::Exceptions` | Which exceptions stop it (`raised`, `uncaught`) |
| `Request::Resume { step }` | `Continue`, `Over`, `Into`, `Out` |
| `Request::Pause` | Stop it where it is |
| `Request::Variables { reference }` | A scope's variables, or what one of them holds |
| `Request::Evaluate { frame, expression, context }` | The watch list (`watch`), the Debug Console (`repl`) and a name the pointer rests on in the editor (`hover`) |
| `Request::Terminate` | End the session, killing the program |
| `Reply::Launched { session, state }` | The session's handle, and where it stopped |
| `Reply::Stopped { state }` | Where it stopped after a step, a continue or a pause |
| `State` | `stopped` (`Entry`, `Breakpoint`, `Step`, `Pause`, `Exception`, `Exited`), the `frames` innermost first, the innermost frame's `scopes` again, and everything the program wrote since the last reply |
| `Frame` | One frame of the call stack: its `id`, `name`, `path`, `line` and its own `scopes`, captured with the stop so that any frame's variables are a `Variables` request away |
| `Reply::Breakpoints { verified }` | For each breakpoint sent, in order, whether the runtime can stop there |
| `Reply::Variables`, `Reply::Evaluated` | Variables, and one expression's value |

A request that cannot be served is an error with the reason in it; the view shows the
reason rather than inventing a stop.

## The machine's half

`cw_computer::debug` defines the seam:

- `trait DebugAdapter` — one implementation per language runtime. Every method is handed
  the `Computer` (so the debugger sees the same files, clock and entropy the program
  does) and the `Session` it is about.
- `struct Session` — a program stopped under a debugger: its id, its runtime, its
  program, and `state`, a JSON value that is the adapter's own and opaque to everything
  else. The machine stores it (`Computer::debug`) and hands it back with the next
  request, so a world snapshot holds a paused program like any other state.
- `Computer::debug(tick, request)` serves a request with the adapters this build has;
  `Computer::debug_with(adapters, ...)` serves it with adapters given to it, which is how
  the tests drive the seam.
- `debug::adapters()` is the registry: `python` and `node`, both implemented in
  `cw_computer::debugger`.

## The pieces below it

| Where | What |
| --- | --- |
| `cw_script_host::debug` | The contract: `Debugger` (the front end), `DebugTarget` (the stopped program), `DebugConfig`, `SourceBreakpoint`, `StopEvent`, `Step`. |
| `cw_pyvm::run_debug`, `cw_jsvm::run_debug` | Run a program with a `Debugger` attached. |
| `cw_script_host::dap` | `DapSession`: a stateful session that turns DAP requests into runs of the program, and its `DebugRunner` trait. |
| `cw_computer::debugger::MachineDebugger` | A `DebugRunner` that runs the program on a machine of the world. |
| `cw_computer::debugger::{PythonAdapter, NodeAdapter}` | The two `DebugAdapter`s, over a `DapSession` kept as data. |

## How a stopped program is possible

An interpreter cannot be kept alive between two actions of the world: its heap is not
serializable, and a snapshot may be restored anywhere. A session therefore never holds a
paused interpreter. It **replays**: each request runs the program again from the start,
with a script of what was answered at every earlier stop, and stops again at the same
place. Host calls the earlier run made — files written, requests sent, the clock,
randomness — are answered from a journal recorded the first time rather than made again,
so replaying does not write a file twice or send a request twice.

The adapters keep that script in `Session::state`: the command, every request that moved
the program, the journal as hex, and what was captured at the stop. Serving a request
rebuilds the `DapSession` from it, adds the new request, and stores the script again.

Three consequences are worth knowing:

* Sessions survive snapshots: everything a session needs is plain data. A machine
  serialised to JSON and read back carries on stepping where it stopped.
* Inspecting is not free. Moving the program is `O(stops)` runs of it, so a session that
  steps a hundred times through a long program is not what this is for.
* The variables of a stop are captured when it happens, for every frame of the stack
  — four levels deep, two hundred children per value, two thousand values in all —
  because `DebugAdapter::variables` is answered from what the machine holds, without
  running anything. The locals of every frame are read before any globals, and an
  outer frame's globals only one level deep, so a wide global object (Node's) does
  not spend the budget before the caller's own variables. A structure deeper than
  that, or a scope beyond the budget, comes back with nothing to open.

A run that a front end suspends (`Step::Suspend`) ends where it stands and keeps what it
did; the session resumes it by replay when the next request arrives.

## Stopping

`DebugConfig` decides where a program stops:

* **Breakpoints** per source path and line, with a `condition` (an expression in the
  program's own language), a `hit_condition` (`"3"`, `">= 3"`, `"% 2"`), or a
  `log_message` — a logpoint, which prints `{expr}` interpolations through
  `Debugger::log` instead of stopping. (`cw_protocol::debug::SourceBreakpoint`, what the
  view sends, carries the path, the line and a condition.)
* **Exception filters**: `raised` stops where an exception is raised, `uncaught` only
  when nothing handles it.
* **`stop_on_entry`**: stop before the program's first line. The interpreter's own setup
  (the frozen stdlib modules it imports, Node's internal modules) is not the program and
  never stops.
* **`pause_after`**: stop with reason `pause` after that many instructions. This is how a
  front end interrupts a program that would otherwise run away; runs here are
  synchronous, so DAP's asynchronous `pause` request arms this for the next resume.

At a stop the front end gets a `StopEvent` and a `DebugTarget`, and answers with a
`Step`: `Continue`, `Next` (over), `StepIn`, `StepOut`, `Suspend` or `Terminate`.

## Looking at the stopped program

`DebugTarget` is DAP's inspection surface:

* `threads()` — the Python scheduler's threads (`MainThread`, `Thread-1`, …), or the one
  thread Node has.
* `stack_trace(thread)` — frames innermost first, with the absolute source path and a
  1-based line and column.
* `scopes(frame)` — `Locals` and `Globals`.
* `variables(reference)` — a scope's names, or the children of a value: a list's items, a
  dict's entries, an object's properties. Values read as the language's console shows
  them (`repr()` in Python, `util.inspect` in Node).
* `evaluate(expression, frame)` — runs an expression where the program stands. In Python
  it runs with that frame's globals and locals; in Node the frame's bindings (its own,
  what it captured, and the frames around it) are in scope. Side effects are real, as in
  a debug console: a `repl` evaluation becomes part of what a replay does, while a
  `watch` or a `hover` is treated as the read it is meant to be.
* `set_variable(reference, name, value)` — evaluates `value` and stores it in that local,
  global or container. The program goes on with the new value. (The Run and Debug view's
  protocol has no request for this; a DAP front end driving `DapSession` does.)

## Driving it from a session

`DapSession::handle(request, runner)` takes the DAP requests a front end sends and
returns a response and the events that came with it (`Initialized`, `Stopped`, `Output`,
`Exited`, `Terminated`). The requests it answers:

`Initialize`, `SetBreakpoints`, `SetExceptionBreakpoints`, `Launch`,
`ConfigurationDone`, `Threads`, `StackTrace`, `Scopes`, `Variables`, `Evaluate`,
`SetVariable`, `Continue`, `Next`, `StepIn`, `StepOut`, `Pause`, `Terminate`,
`Disconnect`.

Its `capabilities()` report what that amounts to: configuration-done, conditional
breakpoints, hit-conditional breakpoints, logpoints, evaluate-for-hovers, set-variable,
terminate, and the two exception filters.

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
changing a variable at a stop and watching the program print the new value;
`crates/computer/tests/debug_adapters.rs` drives the same two runtimes through the
machine's own requests, snapshot and all.

## The view

Breakpoints belong to the workbench, not to a session: they are set with a click in the
gutter (`code:gutter:<group>:<line>`), `F9`, or *Add Conditional Breakpoint*, they are
listed in the view, and they outlive whatever runs. They are sent with the launch, and
again whenever they change while a program runs. Everything else in the view — the call
stack, the variables, the watch values, the Debug Console's output — is what the last
reply said, and is cleared when the session ends.

Starting a session is VS Code's own sequence. The editors in the active group are
written first (`debug.saveBeforeStart`), so the debugger runs the program on screen;
the configuration's `program`, `cwd` and arguments go through VS Code's variables —
`${workspaceFolder}`, `${file}`, `${fileBasenameNoExtension}`, `${relativeFile}`,
`${lineNumber}` and the rest, with anything this world cannot resolve left standing
rather than guessed at; and the Debug Console opens on the first session
(`debug.internalConsoleOptions`). The console keeps the debugger's own narration apart
from the program's output, which is what lets a traceback printed by a debugged program
become a problem at its line, exactly as one printed in the terminal does.

Clicking a frame in the Call Stack (`code:frame:<i>`) selects it: the editor moves to
that frame's file and line (the innermost frame's line stays in the debugger's yellow;
the selected outer frame's is the paler focused-frame shade, as in VS Code), the
Variables view shows that frame's own scopes from what the stop already captured, and
the watches and the Debug Console evaluate there. While the program is stopped, the
pointer resting on a name in the editor shows its value on a plate beside the pointer:
a name among the frame's captured variables (including a dotted path such as `a.b` into
one already opened) is shown at once; any other is asked of the machine with
`Request::Evaluate` in context `hover`, which changes nothing, and one the machine
cannot evaluate shows nothing. The plate is a region (`code:hover`) whose label is
`name: value`, so `semantic.v1` reads it, and it goes away when the pointer moves off
the name, when the program moves and when the session ends.

`crates/computer/tests/debug_seam.rs` drives the whole seam with an adapter of its own,
and `crates/applications/src/apps/code/tests.rs` drives the view against replies, so both
halves are tested against the contract an adapter has to meet — including what the view
does when a machine has no adapter for a runtime: it says so and runs the file in the
terminal, exactly as Run Without Debugging (`Ctrl+F5`) does.

## What is not there

* A debugged program runs against an offline host: the seam hands an adapter the machine
  and nothing else, so a program stopped under the debugger cannot reach the network or
  the world's clock and entropy adapters, and its standard input is empty. Run it from
  the shell for those.
* No data breakpoints, no function breakpoints, no `gotoTargets`, no `completions`, no
  `disassemble`, no source-map support (the simulated runtimes run the program's own
  source, so there is nothing to map).
* No `restartFrame`, and no "run to cursor" beyond what breakpoints give.
* Stepping is per source line. Python's threads all stop together — stopping one thread
  while the others run is not modelled.
* A front end that wants a `stopped` event *while* the program runs cannot have one: runs
  are synchronous, so `Pause` arms `pause_after` for the next resume instead.
* An `uncaught` stop happens where nothing has handled the exception — the outermost
  frame — not where it was raised; the `raised` filter stops at the raise point, with the
  frames that raised it.
* A breakpoint is verified against the launched program's own source; one in a file the
  program imports is taken on trust until it is reached.
* The debuggee's own `breakpoint()` (Python) and `debugger` (JavaScript) statements do
  not stop the program.
