# Action families and observation channels

Reference for every built-in action family, its operations, payload shape and
return value, plus the observation channels and the privileged/actor split.
Derived from `crates/environment/src/lib.rs` (`Environment::dispatch`,
`Environment::observe`, `Environment::scene`) and
`crates/environment/src/desktop_extensions.rs`. When this file and the code
disagree, the code is right.

## The grant model

`EnvironmentConfig` is the whole grant list:

```json
{"actor":"alice","machines":["alice-mac"],
 "actions":["terminal.v1","pointer.v1","keyboard.v1","application.v1","browser.v1"],
 "observations":["semantic.v1","terminal.v1"],
 "action_budget":1024}
```

- `actions` — families the session may dispatch. An unknown family is rejected at
  `environment()` time, not at `step()` time.
- `observations` — channels `observe()` returns, and the gate on `scene()`/`render()`.
- `action_budget` — maximum actions per `step()` batch (default 1024). A larger
  batch is refused whole, before any action runs.
- `machines` — every `ActionEnvelope.machine` must be in this list.

`step()` checks `machines.contains(action.machine) && actions.contains(action.family)`
before dispatch; failure is a per-action `denied` outcome, not an exception. Errors
are flattened to four codes (`denied`, `not_found`, `invalid`, `action_failed`) with
fixed messages, so a failing action cannot leak world state through its error text.

Two presets exist in `crates/protocol/src/lib.rs`:

| Preset | actions | observations |
|---|---|---|
| `EnvironmentConfig::terminal` | `terminal.v1` | `terminal.v1` |
| `EnvironmentConfig::desktop` | `terminal.v1`, `filesystem.v1`, `browser.v1`, `pointer.v1`, `keyboard.v1`, `application.v1`, `http.v1` | `terminal.v1`, `semantic.v1` |

## Built-in action families

The seven built-in families are `BUILTIN_FAMILIES` in `crates/environment/src/lib.rs`.
Every one is actor-available: possession of the family in `actions` is the entire
permission. There are no owner-only operations *inside* a family — the
privileged/actor split is at the handle level (see below).

### `terminal.v1`

| Op | Payload | Returns |
|---|---|---|
| `execute` | `{"command": string}` | `{"exit_code": i32, "pid": u64, "stdout": string, "stderr": string}` |

The result is also latched into the session's `terminal.v1` observation channel.

**Two levels of failure, and they are not the same.** `outcome.success` reports
whether the *action* dispatched; `exit_code` reports whether the *command* worked.
A command that does not exist, or a flag the shell does not implement, returns
`success: true` with a non-zero `exit_code` and an explanatory `stderr` — exactly as
a real shell would. A provider that checks only `success` will read every failed
command as a success.

```
awk x  ->  success: true, {"exit_code": 1, "stderr": "awk: command not found: awk\n"}
```

The shell refuses rather than approximates. `find . -newer foo` does not silently
drop the predicate; it fails with ``find: unsupported predicate `-newer` ``. `sed`
accepts `s/PATTERN/REPLACEMENT/[g]` and says so when given anything else. Unsupported
surface is visible at the point of use, so a task built on it fails loudly instead of
producing a plausible wrong answer.

### `filesystem.v1`

| Op | Payload | Returns |
|---|---|---|
| `read` | `{"path": string}` | `{"content": string, "bytes": [u8]}` — `content` is lossy UTF-8; `bytes` is exact |
| `write` | `{"path": string, "content": string}` | `null` |
| `list` | `{"path": string}` | array of entry names |
| `stat` | `{"path": string}` | the serialized inode metadata |

Paths are resolved against the machine's cwd. `list` checks execute+read access as
the machine's user and fails `denied`; a missing path is `not_found`. These are the
actor's own filesystem calls, subject to simulated permissions — not the owner's
unrestricted view.

### `http.v1`

| Op | Payload | Returns |
|---|---|---|
| `request` | serialized `HttpRequest`: `{"method": string, "url": string, "headers": {string: string}, "body": [u8]}` | serialized `HttpResponse`: `{"status": u16, "headers": {…}, "body": [u8]}` |

Routed through the simulated network from the acting machine. No host socket is
involved.

### `browser.v1`

All ops return the active page projection (`{"version","title","elements","theme"}`).
Dispatch first forces the browser visible and, on a themed desktop, focuses or
launches a browser window; per-window browser state is swapped in and out so each
window keeps its own history and tabs.

| Op | Payload | Notes |
|---|---|---|
| `navigate` | `{"url": string}` | Performs a real simulated HTTP fetch |
| `back` / `forward` / `reload` | `{}` | History moves re-fetch |
| `fill` | `{"id": string, "value": string}` | Sets a form field and focuses it |
| `submit` | `{"id": string}` | Submits the named form |
| `click` | `{"id": string}` | Element id from the page projection |
| `key` | `{"key": string}` | Key into the focused page element |
| `new_tab` | `{}` | |
| `switch_tab` / `close_tab` | `{"tab": u64}` | Index |
| `scroll` | `{"y": i64}` | Clamped to `0..=i32::MAX` |

Any other op is `invalid`. Note the cross-family gate: `application.v1 launch` of
kind `browser`, `keyboard.v1 key` of `Enter` in a focused address bar, and pointer
clicks that resolve into page content all additionally require `browser.v1` in
`actions` and are `denied` without it.

### `application.v1`

| Op | Payload | Returns |
|---|---|---|
| `launch` | `{"kind": string, "argument"?: string, "instance"?: string, "initial"?: any}` | `{"window": u64}` for built-ins, `{"instance": string}` for a registered SDK app |
| `focus` | `{"window": u64}` | `null` |
| `close` | `{"window": u64}` | `null` |
| `home` / `launcher` / `minimize` / `maximize` / `switcher` | `{}` | `null` |
| `shell` | `{"target": string}` | Whatever that control returns |
| `event` | `{"instance"?: string, "event": AppEvent}` (or the event inline) | The registered app's new page |

`shell` invokes a shell control **by name**, reaching the same handler a pointer
reaches by hit-testing, through the same grants. The target is any `shell:*` id, or a
`window:<id>:*` id, which focuses that window first. This is how an agent drives a
control without locating its pixels, and how a test asserts a control's behaviour
independently of where a shell happened to paint it. A target that is neither is
`invalid`.

#### Shell interaction targets

Every one of these is routed by `shell_action` / `desktop_panel_action`, and every one
is reachable either from a painted control or from `application.v1 shell`.

| Target | Effect |
|---|---|
| `shell:launch:<kind>` | Launch or focus an application |
| `shell:launch:<kind>/<argument>` | Launch it *on* something: a file manager on a folder, a calendar on a date. Every shell's calendar panel paints each day as `shell:launch:calendar/YYYY-MM-DD`; days before the world began, or with no Calendar installed, carry no target |
| `shell:open:<kind>` | Desktop icon: select on one click, open on two |
| `shell:home` / `shell:launcher` / `shell:desktop` / `shell:dismiss` | Show the desktop, toggle the launcher, dismiss a panel |
| `shell:panel:<name>` | Open a panel: `apple`, `file`, `edit`, `view`, `window`, `help`, `spotlight`, `control`, `quick`, `calendar`, `notifications`, `settings`, `overview`, `context`, `power`, `app-menu` (GNOME header-bar primary menu), `app-settings` (Notepad settings), `page` (iOS Safari "AA"). On iOS `calendar` is Today View (search and widgets) and `notifications` Notification Center (the notices), and a phone's panel other than `search` is modal: the soft keyboard goes down under it and keystrokes reach nothing behind it until it closes. Choosing an entry in a drop-down menu (`file`, `edit`, `view`, `format`, `app-menu`) closes it |
| `shell:search` / `shell:settings` / `shell:overview` / `shell:notifications` / `shell:quick-settings` | Panel shortcuts |
| `shell:gesture:home` / `shell:gesture:overview` / `shell:gesture:notifications` / `shell:gesture:control-center` | Phone **gesture affordances**: the iPhone home indicator and the phones' status bars. A pointer reaches them by *dragging* from them (see Touch gestures below); a tap on one does nothing, as a tap on the glass does nothing. Named here, one performs what that swipe would do right now: `home` puts away a pulled-down sheet (Notification Center, Control Center, Search), returns Today View or the App Library to the first page, and otherwise goes home — from the home screen itself, to its first page; `overview` opens the App Switcher; `notifications` pulls down Notification Center, or on Android the shade and, pulled again, Quick Settings; `control-center` pulls down Control Center. Painted with the semantic role `gesture` |
| `shell:home-page:<n>` | Show page `<n>` (0 first) of the iOS home screen, closing the App Library or any panel. iOS paints one page dot per page above the dock, each carrying this target; the scene marks the current one `selected`. A page past the last shows the last. Returns `{"page"}` |
| `shell:mobile-back` | The platform Back action (Android's navigation bar ◁): close a panel or the launcher, else go back in the browser tab's history, else up one folder in the file manager, else leave the application for the home screen |
| `shell:back` / `shell:forward` / `shell:reload` / `shell:address` | Browser navigation, refused when there is no history or no page |
| `shell:tab:new` / `shell:tab:select:<i>` / `shell:tab:close:<i>` | Browser tabs |
| `shell:toggle:<switch>` | Flip a device switch: `wifi`, `bluetooth`, `airplane_mode`, `do_not_disturb`, `night_light`, `dark_mode`, `rotation_lock`, `flashlight`, `battery_saver`, `hotspot` |
| `shell:set:<level>:<pct>` | Set `brightness` or `volume` to an exact percentage |
| `shell:zoom:in` / `shell:zoom:out` / `shell:zoom:reset` | Step the page on screen through the browser zoom levels (50–300%) or back to 100%, remembered per site; the page is laid out for the narrower or wider CSS viewport, not resampled. Returns `{"zoom"}`. `Ctrl`/`Meta` + `=`, `-`, `0` in a browser do the same. Safari's View menu and iOS Safari's "AA" menu carry these |
| `shell:terminal:clear` | Empty the focused terminal's scrollback, as `clear` does (GNOME Terminal's Reset and Clear) |
| `shell:toggle:word_wrap` | Soft-wrap text editors at the window edge. Notepad's View menu and settings, TextEdit's View ▸ Wrap to Window and GNOME Text Editor's primary menu toggle it; phones always wrap. A wrapped editor's text target is `editor-text:<first row>:<columns>` |
| `shell:power:lock` / `:off` / `:restart` / `:wake` | Move the display between `Active`, `Locked` and `Off` |
| `shell:month:prev` / `:next` / `:today` | Page a calendar panel's month grid |
| `shell:type:<char>` / `shell:key:<key>` | On-screen keyboard: type one character, or send a named key |
| `shell:insert:<text>` | Insert a whole short line, for a suggestion chip or a paste control; same `keyboard.v1` pipeline as `shell:type:`. The Android keyboard's suggestion strip and the iOS QuickType bar each paint up to three chips completing the word before the caret from a fixed word list, each `shell:insert:<rest> ` |
| `shell:key:Shift` / `shell:plane:letters\|numbers\|symbols` | Keyboard modifier and plane |
| `shell:bookmark` / `shell:bookmark:open:<i>` | Save or reopen a page |
| `shell:download` | Fetch the page on screen through the gateway and write it to `~/Downloads` |
| `shell:screenshot` | Rasterise the screen to a PNG in `~/Pictures`; needs the `pixels.v1` grant |
| `shell:share` / `shell:share:chat` / `shell:share:mail` | Hand the focused window's page, document, or selected file (the folder when nothing is selected) to a messaging application. Messages opens with it as the draft; Mail opens a message with it as the body and its name as the subject, addressed to nobody yet. Nothing is sent until the actor sends it |
| `shell:trash` | Open the file manager on the trash folder |
| `shell:workspace:new` / `:close` / `:<n>` / `:move:<n>` | Virtual desktops |
| `shell:notice:<i>` / `shell:notifications:seen` | Open or acknowledge a notification |
| `shell:group:<name>` | Expand a launcher category |
| `shell:new` / `shell:save` / `shell:noop` | Application commands, and a deliberate click absorber |
| `window:<id>:focus\|drag\|close\|minimize\|maximize\|resize:<edge>` | Window management |
| `window:<id>:content:<target>` | A control inside that window's client area |

File manager controls, reached as `window:<id>:content:<target>`: `files-back`,
`files-forward`, `files-up`, `files-root`, `files-home`, `files-reload`,
`files-newtab`, `files-tab:<i>`, `files-closetab:<i>`, `files-location:<path>`,
`files-view`, `files-sort:<key>`, `files-search`, `files-search-clear`,
`files-recents`, `files-browse`, `files-open`, `files-new-folder`, `files-new-file`,
`files-cut`, `files-copy`, `files-paste`, `files-rename`, `files-delete`, and
`open:<i>`, which indexes the **displayed** row order rather than the raw listing.

`kind` accepts `text_editor` as an alias for `editor` and `file_manager` for
`files`. Built-in window kinds are `browser`, `files`, `editor`, `terminal`, plus
the nine native applications (`calendar`, `mail`, `chat`, `docs`, `notes`,
`contacts`, `settings`, `calculator`, `clock`) listed by the `native_apps!` macro in
`crates/applications/src/apps/mod.rs`. A world may also declare `desktop_apps`
metadata aliases that launch a browser window at a fixed URL. Launching a kind the
machine does not have installed is `not_found`.

### `keyboard.v1`

| Op | Payload | Returns |
|---|---|---|
| `type` | `{"text": string}` | `null`, or the registered app's page when one is focused |
| `key` | `{"key": string}` | `null`, the browser page, or the app page depending on focus |

Routing is by focus, in order: an open shell panel (search field), a focused
registered application, a focused address bar, a visible browser, otherwise the
desktop/window. `Meta`, `Super`, `Meta+Space` and `Ctrl+Escape` open the launcher on
a themed desktop; `Alt+Tab` cycles windows.

### `pointer.v1`

| Op | Payload | Returns |
|---|---|---|
| `move` | `{"x": i64, "y": i64, "width"?: u64, "height"?: u64}` | `{"cursor": string}` |
| `down` | same, plus `{"button"?: u64}` | `null` |
| `up` | same | `null`, `{"cursor": …}` while a drag is captured, or the target's own result |
| `click` | same | the target's result |
| `double_click` | same | the target's result |
| `cancel` | same | `null` |

`x`/`y` are clamped to ±32768; `width`/`height` default to 1024×768 and are capped
at 8192. Coordinates are in the same viewport you pass to `scene(width, height)` —
hit testing runs against that scene, so the actor aims using only what it can see.
`cursor` is one of `default`, `pointer`, `text`, `grab`, `grabbing`, `ns-resize`,
`ew-resize`, `nesw-resize`, `nwse-resize`.

Behaviour worth knowing:

- **`down` then `up` is a click with press identity.** `down` records the hit target
  and its bounds; `up` only fires if released inside those bounds. Dragging out
  cancels, as on a real pointer.
- **Double-click is real.** On a desktop theme, a single `click` on a desktop icon or
  file-manager row *selects*; `double_click` *opens*. On `ios`/`android` themes there
  is no such distinction and a tap opens immediately. `double_click` on a window
  title bar (`window:<id>:drag`) maximizes.
- **`button: 2` on `down`** opens the context panel.
- **Touch gestures** are synthesized from `down`/`up` on mobile themes (a move of more
  than 70 px, mostly along one axis; a smaller one is a tap). A finger coming down on a
  control only presses it; the release decides whether it was a tap or a swipe.
  - **iOS**: down from the status bar opens Notification Center, or Control Center
    when it starts right of the Dynamic Island (`shell:gesture:notifications` /
    `:control-center`); up from the bottom edge — the home indicator — goes home
    (`shell:gesture:home`), and a swipe longer than a third of the screen opens the App
    Switcher (`shell:gesture:overview`); up anywhere puts away Notification Center or
    Control Center. On the home screen, right to left moves to the next page and, past
    the last, opens the App Library; left to right moves to the previous page and,
    before the first, opens Today View (panel `calendar`); down opens Search. Left to
    right in the App Library returns to the last page, right to left in Today View to
    the first. Left to right along the bottom edge in an application switches to the
    previous one. The home indicator is painted only where the device shows one: over
    applications, Settings and Notification Center, not on the home screen.
  - **Android** (three-button navigation): down from the status bar, or anywhere on
    the home screen, opens the notification shade, and a second pull expands it to
    Quick Settings; up closes the shade; up on the home screen opens the app drawer and
    down closes it. Swipes that start on the 48 px navigation bar are presses of its
    buttons: Back (`shell:mobile-back`), Home (`shell:home`), Recents
    (`shell:overview`).
  - On both, a card swiped up in the overview / App Switcher closes that application.
    Tapping the space around the cards goes home.
- Window `drag` and `resize:<edge>` operations capture the pointer between `down` and
  `up`; while captured, `move`/`up` bypass hit testing.

#### Interaction targets

Scene nodes carry an `interaction` string. Clicking one is how everything in the
shell is driven, so these strings are part of the actor-facing contract.

| Prefix | Meaning |
|---|---|
| `window:<id>:drag` \| `:focus` \| `:minimize` \| `:maximize` \| `:close` \| `:resize:{n,s,e,w,ne,nw,se,sw}` | Window frame. Requires `application.v1`. |
| `window:<id>:content:<target>` | Forwarded into the window's content as `<target>` |
| `shell:launch:<kind>` | Launch an application (same path as `application.v1 launch`) |
| `shell:open:<kind>` | Desktop icon: select on click, launch on double-click |
| `shell:home`, `shell:launcher`, `shell:switcher`, `shell:minimize`, `shell:maximize`, `shell:close`, `shell:desktop`, `shell:dismiss`, `shell:noop`, `shell:new`, `shell:save`, `shell:mobile-back` | Shell commands |
| `shell:panel:<name>` where name ∈ `apple file edit view window help spotlight search control quick calendar clock notifications settings overview context power` | Toggle a panel. `shell:menu:<Name>`, `shell:search`, `shell:spotlight`, `shell:settings`, `shell:overview`, `shell:recents`, `shell:notifications`, `shell:control-center`, `shell:quick-settings`, `shell:system` are aliases. |
| `shell:toggle:<setting>` | Flip a device switch (wifi, bluetooth, airplane, dark, …). Returns `{"setting","value"}`. |
| `shell:set:<setting>:<percent>` | Set a level 0–100. Returns `{"setting","value"}`. |
| `shell:power:{lock,off,shutdown,restart,wake,unlock}` | Changes what the screen actually shows; `off`/`restart` clear windows |
| `shell:tab:new`, `shell:tab:select:<i>`, `shell:tab:close:<i>` | **Browser** tabs; re-dispatched as `browser.v1 new_tab`/`switch_tab`/`close_tab` |
| `window:<id>:content:files-newtab` \| `files-tab:<i>` \| `files-closetab:<i>` | **File manager** tabs. They live in the window, not the browser session, so they use different targets. |
| `window:<id>:content:files-{home,up,root,back,forward,reload,path}`, `files-location:<path>` | File manager navigation |
| `files-view`, `files-sort:{name,kind}` | List/grid toggle, and the sort key. The same key again reverses it. Both only reorder the view. |
| `files-search`, `files-search-clear` | Focus the query field, and clear it. Typing goes to `FileTab::query`, which filters the rows. |
| `files-{new-folder,new-file,cut,copy,paste,rename,delete}` | File manager mutations. Each runs through the kernel under the same access checks as `read_file`/`write_file`, and re-lists the folder afterwards. `delete` moves to `~/.local/share/Trash/files`; nothing is hard-removed. `rename` opens a field committed with `Enter` and cancelled with `Escape`. |
| `files-recents`, `files-browse` | Switch the tab between the desktop's recent-documents list and the folder it was showing. |
| `window:<id>:content:terminal-scroll:<n>`, `terminal-line` | Terminal scrollback position, and the prompt line — a click on it places the input caret. |
| `shell:address`, `shell:back`, `shell:forward`, `shell:reload` | Browser chrome. History ops require `browser.v1`. |
| `shell:type:<text>`, `shell:key:<key>` | Re-dispatched as `keyboard.v1` |
| `window:<id>:content:open:<index>` | File-manager row. **`<index>` is the row on screen**, after the sort and the query filter, not an index into the raw listing — so a click always lands on the row the pointer was over. Select on click, open on double-click. Folders open in the same tab; a document needs `editor` installed or the action is `not_found`. |

Every `shell:*` target requires `application.v1`. An unrecognized `shell:*` target
is `invalid`.

### Extension families

`Environment::register_action_family` adds a family; `register_observation_channel`
adds a channel. Both reject names that collide with built-ins. Registration is an
**owner** operation on trusted code. An extension still only runs for a session that
lists its name in `actions`/`observations`, and it receives `&mut Runtime`, so it is
responsible for its own access checks. Registered module versions are part of the
snapshot compatibility boundary.

## Observation channels

`observe()` returns `{"tick": u64, "channels": {name: value}}`. Built-in channels are
keyed by machine inside the channel value.

| Channel | Value | Content |
|---|---|---|
| `terminal.v1` | `{machine: result}` | The last `terminal.v1 execute` result on that machine; `null` before the first |
| `semantic.v1` | `{machine: Page}` | The focused surface as a structured page: the registered app's page, the native app view, the browser page, or the shell view |
| `browser.v1` | `{machine: {"page": Page\|null, "url": string\|null, "fields": {…}}}` | Current tab only |
| `filesystem.v1` | `{machine: {"cwd": string}}` | Working directory only — file contents come from the action family, not this channel |
| `pixels.v1` | `{}` | **Carries no data.** It is a capability token: it (or `semantic.v1`) unlocks `scene()` and `render()` |

`scene(width, height)` returns the structured scene for the *focused* machine and
requires `semantic.v1` or `pixels.v1`; without either it is `denied`. Viewport must
be 1..=16 777 216 pixels. `render(width, height)` rasterizes that same scene to
`{"width","height","rgba"}` and is gated identically.

Granting `semantic.v1` without `pixels.v1` is a meaningful configuration only if you
also refuse to call `render()`; the runtime does not distinguish them as gates.

## Privileged (owner) versus actor

The split is by **object**, not by operation name. This is what lets an evaluator
grade an episode honestly: the grader reads ground truth through a handle the agent
provably never had.

| Owner — `World` | Actor — `Environment` / `ActorEnvironment` |
|---|---|
| `environment(config)`, `session(id)` — mint and recover handles | `id` |
| `step(session, actions)`, `observe(session)`, `scene(…)`, `render(…)` | `step(actions)`, `observe()`, `scene(…)`, `render(…)` |
| `snapshot`, `restore`, `fork`, `reset` | — |
| `export_snapshot`, `import_snapshot` | — |
| `state_hash` | — |
| `trajectory` — full causal/network event log | — |
| `definition` — the whole world definition | — |
| `inspect` — unrestricted world state | — |
| `add_computer`, `remove_computer` | — |
| `register_application`, `register_action_family`, `register_observation_channel` | — |
| `runtime()` / `runtime_mut()`, `interfaces()` / `interfaces_mut()` | — |

The last three rows — registration, `runtime()` and `interfaces()` — are Rust-only;
they hand out unrestricted engine access and are deliberately absent from the Python
and Wasm bindings.

The bindings enforce this structurally rather than by convention. In Python,
`World` and `Environment` are separate classes and the actor object has exactly five
attributes — `id`, `step`, `observe`, `scene`, `render` — with no `snapshot`,
`inspect`, `state_hash` or `trajectory`; `examples/python/smoke.py` asserts that. In
Rust, `world.actor(&session)?` hands out an `ActorEnvironment<'_>` borrow with the
same four methods.

Consequences for evaluation:

- An actor can never read the world definition, so task-relevant facts cannot be
  recovered by introspecting the environment instead of using the computer.
- `trajectory()` and `inspect()` give a grader ground truth the agent has no path to.
- `state_hash()` over the owner handle detects whether two runs diverged at all, at
  semantic rather than pixel granularity.
- The action journal is tamper-evident (`action_journal()`, verified on
  `import_snapshot`), so a recorded episode can be replayed and checked rather than
  trusted. `enable_replay_verification(true)` additionally hashes after every step
  and fails `replay_diverged` at the first mismatching step.
- `fork(snapshot)` branches a checkpoint cheaply, which is how you score several
  continuations from one state without re-running the prefix.

Never hand a `World` to the agent under evaluation. Hand it the session handle.
