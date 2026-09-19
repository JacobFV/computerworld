# Debugging a program in the world

Visual Studio Code's **Run and Debug** view talks to the machine the program runs on.
Nothing in the editor executes anything: it sends requests and paints what comes back,
so what a person sees stopped on a line is only ever what the machine said.

```
Run and Debug view ── AppEffect::Debug { tag, request } ──▶ environment
                                                             │
                                          Runtime::debug ────┤ (kernel: records the event)
                                                             ▼
                                        Computer::debug ── DebugAdapter for the runtime
                                                             │
     DesktopState::debug_reply ◀── Result<Reply, String> ─────┘
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
| `Request::Evaluate { frame, expression, context }` | The watch list (`watch`) and the Debug Console (`repl`) |
| `Request::Terminate` | End the session, killing the program |
| `Reply::Launched { session, state }` | The session's handle, and where it stopped |
| `Reply::Stopped { state }` | Where it stopped after a step, a continue or a pause |
| `State` | `stopped` (`Entry`, `Breakpoint`, `Step`, `Pause`, `Exception`, `Exited`), the `frames` innermost first, the innermost frame's `scopes`, and everything the program wrote since the last reply |
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
  request, so a world snapshot holds a paused program like any other state. An adapter
  that cannot serialise a live interpreter may keep a recipe instead — the program and
  how far it has run — and replay it, which is exact because the world is deterministic.
- `Computer::debug(tick, request)` serves a request with the adapters this build has;
  `Computer::debug_with(adapters, ...)` serves it with adapters given to it, which is how
  the tests drive the seam.
- `debug::adapters()` is the registry.

## What is not implemented

**There are no adapters in this build.** `cw-pyvm` and `cw-jsvm` run a program to
completion and have no stepping API, and a debugger belongs in the interpreter that
executes the lines rather than in a second one beside it. Until they have one:

- `Computer::debug` answers every request with *no debug adapter for Python (or Node.js)
  is installed on this machine*.
- The Run and Debug view says so, keeps no session, and starts nothing: no call stack, no
  variables, no stepping.
- `F5` still runs the program: the view reports the machine's reason and then runs the
  file in the integrated terminal, exactly as Run Without Debugging (`Ctrl+F5`) does.

To finish the wiring, with no change anywhere above this file:

1. Implement `DebugAdapter` over the runtime's stepping API — one type per runtime, with
   `kind()` returning `python` or `node`.
2. Return them from `cw_computer::debug::adapters()`.

`crates/computer/tests/debug_seam.rs` drives the whole seam with an adapter of its own,
and `crates/applications/src/apps/code/tests.rs` drives the view against replies, so both
halves are already tested against the contract an adapter has to meet.

## The view

Breakpoints belong to the workbench, not to a session: they are set with a click in the
gutter (`code:gutter:<group>:<line>`), `F9`, or *Add Conditional Breakpoint*, they are
listed in the view, and they outlive whatever runs. They are sent with the launch, and
again whenever they change while a program runs. Everything else in the view — the call
stack, the variables, the watch values, the Debug Console's output — is what the last
reply said, and is cleared when the session ends.
