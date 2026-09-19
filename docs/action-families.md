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
fixed messages, so a failing action cannot leak world state through its error text. A
refusal also carries a `reason` from a closed vocabulary — which grant is missing, or
what is not there — whose message is likewise a compile-time constant. See
[Errors and refusals](agent-api.md#errors-and-refusals) for the whole table.

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
| `scroll` | `{"y": i64}`, or `{"row": string, "x": i64}` for a shelf that scrolls sideways | Clamped to `0..=i32::MAX` |

Any other op is `invalid`. Note the cross-family gate: `application.v1 launch` of
kind `browser`, `keyboard.v1 key` of `Enter` in a focused address bar, and pointer
clicks that resolve into page content all additionally require `browser.v1` in
`actions` and are `denied` without it. That denial carries the reason
`browser_family_required`, and `application.v1 list` marks such an application
`launchable: false` with the same reason, so an installed-but-ungranted browser is
never a bare refusal.

### `application.v1`

| Op | Payload | Returns |
|---|---|---|
| `list` | `{"installed"?: bool}` (default `true`) | array of `{"id","label","kind","installed","launchable","blocked_by"}` |
| `launch` | `{"kind": string, "argument"?: string, "instance"?: string, "initial"?: any}` | `{"window": u64}` for built-ins, `{"instance": string}` for a registered SDK app |
| `focus` | `{"window": u64}` | `null` |
| `close` | `{"window": u64}` | `null` |
| `home` / `launcher` / `minimize` / `maximize` / `switcher` | `{}` | `null` |
| `shell` | `{"target": string}` | Whatever that control returns |
| `event` | `{"instance"?: string, "event": AppEvent}` (or the event inline) | The registered app's new page |

`list` is how an agent finds out what is on a machine without guessing ids. `id` is the
`kind` `launch` takes; `label` is the name this machine's shell paints under the
application's icon — the same string, from the same table, that titles its window;
`kind` is `builtin` (one of the four the compositor implements), `native` (an
application shipped with the simulator) or `web` (a `desktop_apps` alias that opens a
site in the browser). `installed` reflects the computer's `installed_apps`;
`launchable` says whether `launch` would succeed **right now for this session**, and
`blocked_by` names the [documented reason](agent-api.md#errors-and-refusals) it would
not — typically `application_not_installed` or `browser_family_required`. Passing
`{"installed": false}` returns the uninstalled entries too, so "what could this world
have?" is answerable as well as "what does it have?". From the shell, `apps` lists the
same ids.

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
| `shell:open:<kind>` | Desktop icon: select on one click, open on two. `shell:open:trash` is the Windows Recycle Bin, which opens the trash folder |
| `shell:home` / `shell:launcher` / `shell:desktop` / `shell:dismiss` | Show the desktop, toggle the launcher, dismiss a panel |
| `shell:panel:<name>` | Open a panel: `apple`, `file`, `edit`, `view`, `go` (Finder's Go menu), `window`, `help`, `spotlight`, `control`, `quick`, `calendar`, `notifications`, `settings`, `overview`, `context`, `power`, `app-menu` (GNOME header-bar primary menu), `app-settings` (Notepad settings), `page` (iOS Safari "AA"). On iOS `calendar` is Today View (search and widgets) and `notifications` Notification Center (the notices), and a phone's panel other than `search` is modal: the soft keyboard goes down under it and keystrokes reach nothing behind it until it closes. Choosing an entry in a drop-down menu (`file`, `edit`, `view`, `format`, `app-menu`) closes it |
| `shell:search` / `shell:settings` / `shell:overview` / `shell:notifications` / `shell:quick-settings` | Panel shortcuts |
| `shell:gesture:home` / `shell:gesture:overview` / `shell:gesture:notifications` / `shell:gesture:control-center` | Phone **gesture affordances**: the iPhone home indicator and the phones' status bars. A pointer reaches them by *dragging* from them (see Touch gestures below); a tap on one does nothing, as a tap on the glass does nothing. Named here, one performs what that swipe would do right now: `home` puts away a pulled-down sheet (Notification Center, Control Center, Search), returns Today View or the App Library to the first page, and otherwise goes home — from the home screen itself, to its first page; `overview` opens the App Switcher; `notifications` pulls down Notification Center, or on Android the shade and, pulled again, Quick Settings; `control-center` pulls down Control Center. Painted with the semantic role `gesture` |
| `shell:home-page:<n>` | Show page `<n>` (0 first) of the iOS home screen, closing the App Library or any panel. iOS paints one page dot per page above the dock, each carrying this target; the scene marks the current one `selected`. A page past the last shows the last. On Windows it pages Start's pinned apps (three rows a page, dots at the right edge) and Start stays open. Returns `{"page"}` |
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
`files-recents`, `files-browse`, `files-starred`, `files-star`, `files-star:<i>`,
`files-quick-access`, `files-gallery`, `files-trash`, `files-hidden`, `files-open`,
`files-new-folder`, `files-new-file`,
`files-cut`, `files-copy`, `files-paste`, `files-rename`, `files-delete`, and
`open:<i>`, which indexes the **displayed** row order rather than the raw listing.

Visual Studio Code controls (kind `code`), reached as `window:<id>:content:<target>`.
Every one dispatches into the same command table the Command Palette, the menus and
the keybindings use, and a control whose command cannot run now is painted disabled
with the reason:

| Target | Effect |
|---|---|
| `code:cmd:<command id>` | Run a command, e.g. `workbench.action.quickOpen`, `workbench.action.files.save`, `python.execInTerminal`, `git.commit`. The ids are VS Code's own (`crates/applications/src/apps/code/commands.rs`) |
| `code:menu:<file\|edit\|selection\|view\|go\|run\|terminal\|help\|manage>`, `code:menu-close` | Open a menu from the title bar (Windows, Ubuntu) or the Manage gear; on macOS the same menus hang from the Mac menu bar's File, Edit, View and Help |
| `code:activity:<explorer\|search\|scm\|run>` | Activity bar; the active view again hides the side bar |
| `code:tree:<relative path>` | Explorer row: a folder toggles; a file opens in a preview tab on click and pinned on double click. A `pointer.v1 down` with `"button": 2` opens the Explorer's context menu on that row (New File, New Folder, Rename, Delete, Copy Path, Reveal in the file manager, Open in Integrated Terminal), whose entries are `code:cmd:` targets |
| `code:explorer`, `code:inline` | Focus the Explorer (arrow keys, `Enter`, `F2` rename, `Delete` to the trash) or the inline name box of New File, New Folder and Rename |
| `code:tab:<i>`, `code:tab-close:<i>`, `code:crumb:<folder>` | Editor tabs (a double click pins a preview), close (asks first when unsaved), and a breadcrumb folder revealed in the Explorer |
| `code:editor:<group>:<first row>:<first column>:<wrap columns>:<visible rows>` | The text area of an editor group. A click places the caret at the character under the pointer and gives that group the focus; `pointer.v1 down` then `up` inside it selects from the press to the release; a double click selects a word; a press with `"modifiers": ["alt"]` adds a cursor there; `"button": 2` opens the editor's context menu (Cut, Copy, Paste, Go to Definition, Command Palette). Tabs in the text are tab stops, and `editor.renderWhitespace` draws spaces and tabs |
| `code:scroll:<group>:<row>` | Scrollbar track: page that group's editor to that row |
| `code:minimap:<group>:<first row>:<rows>` | The minimap. A press scrolls the editor to the part of the file under the pointer, and a drag keeps scrolling it |
| `code:group:<i>` | An editor group with nothing open in it (Split Editor Right/Down, Move Editor into Next/Previous Group and Focus *n*th Editor Group are `code:cmd:` targets; a file open in two groups is one document) |
| `code:find-input`, `code:replace-input`, `code:find:<case\|word\|regex\|prev\|next\|replace\|replace-all\|toggle-replace\|close>` | The find widget (`Ctrl+F`, `Ctrl+H`) |
| `code:search-input`, `code:search-replace-input`, `code:search:<case\|word\|regex\|toggle-replace\|clear\|collapse>`, `code:search-file:<path>`, `code:search-result:<file>:<hit>`, `code:search-replace-all` | Search view: literal, regex, case and whole-word search over the workspace's files; a result opens its file with the match selected |
| `code:scm-message`, `code:scm-stage:<path>`, `code:scm-open:<path>` | Source Control, backed by the machine's `git` (`status`, `add`, `commit`, `init`, `branch`, `checkout`) |
| `code:panel:<problems\|output\|terminal\|debug>`, `code:panel-close`, `code:terminal`, `code:terminal-line`, `code:term-tab:<i>`, `code:term-scroll:<n>`, `code:problem:<i>` | The panel. The terminal is a session of the machine's shell with its own working directory; a problem opens its file at its line; `debug` is the Debug Console |
| `code:gutter:<group>:<line>` | Set or remove a breakpoint on that line of the editor's file |
| `code:bp:<i>`, `code:bp-remove:<i>`, `code:bp-open:<i>`, `code:exception:<raised\|uncaught>` | The Run and Debug view's breakpoint list: enable, remove, open its line, and the exception filters |
| `code:frame:<i>`, `code:var:<reference>`, `code:watch-remove:<i>`, `code:debug-config:<i>` | A frame of the call stack, a variable expanded, a watch expression removed, a launch configuration chosen |
| `code:repl-input`, `code:debug-prompt`, `code:debug-prompt-ok`, `code:debug-prompt-close` | The Debug Console's input, and the box that asks for a watch expression or a breakpoint's condition |
| `code:status:<branch\|problems\|position\|indent\|eol\|language>` | Status bar items: branch picker, Problems, Go to Line, tab size, line endings, language mode |
| `code:quick-input`, `code:quick:<i>`, `code:quick-ok`, `code:quick-close` | Quick input: Quick Open (`Ctrl+P`, fuzzy over workspace files, `:` for a line), the Command Palette (`Ctrl+Shift+P`, `>`), and the pickers (theme, language, tab size, line endings, branch, Open Folder, Save As) |
| `code:dialog:<i>`, `code:notice-close`, `code:settings:<theme\|font\|tab\|wrap>:<value>`, `code:welcome` | Modal dialog buttons, the notification toast, the Settings editor, and the empty editor area |

Keys follow VS Code, with `Meta` treated as `Cmd`, i.e. as `Ctrl`: `Ctrl+S`, `Ctrl+Z`/`Ctrl+Y`,
`Ctrl+X`/`C`/`V` through the machine's text clipboard, `Tab`/`Shift+Tab`, `Ctrl+/`,
`Alt+Up`/`Down`, `Shift+Alt+Up`/`Down`, `Ctrl+Shift+K`, `Ctrl+Enter`, `Ctrl+G`, `Ctrl+F`/`H`,
`F3`, `Ctrl+B`, `Ctrl+J`, ``Ctrl+` ``, `F5`/`Ctrl+F5`, `Alt+Z`, `Ctrl+,` and the `Ctrl+K` chords
(`Ctrl+K Ctrl+O` Open Folder, `Ctrl+K Ctrl+T` theme, `Ctrl+K M` language). A single typed
character is a keystroke (brackets and quotes close and are typed over, `Enter` keeps the
indentation); a longer `keyboard.v1 type` is inserted as written, the way a paste is, so
its own indentation is not indented again.

FreeCAD controls (kind `freecad`), reached as `window:<id>:content:<target>`. Commands
carry FreeCAD's own names; one the document cannot take now (Pocket with no solid,
Fillet with no edge selected) is painted disabled with the reason, everywhere it appears.

| Target | Effect |
|---|---|
| `freecad:cmd:<command>` | Run a command: `Std_New`, `Std_Open`, `Std_Save`, `Std_SaveAs`, `Std_Import`, `Std_Export`, `Std_Undo`, `Std_Redo`, `Std_Delete`, `Std_Refresh`, `Std_SelectAll`, `Std_ViewFitAll`, `Std_ViewFitSelection`, `Std_View{Isometric,Front,Top,Right,Rear,Bottom,Left}`, `Std_OrthographicCamera`, `Std_PerspectiveCamera`, `Std_ToggleVisibility`, `Std_SelBoundingBox`, `Std_AxisCross`, `Std_ReportView`, `Std_Measure`, `Std_About`, `PartDesign_{Body,NewSketch,Pad,Revolution,Pocket,Hole,Groove,Fillet,Chamfer,Mirrored,LinearPattern,PolarPattern,MoveTip}`, `Sketcher_{EditSketch,LeaveSketch,ViewSketch}`, the geometry tools `Sketcher_Create{Point,Line,Arc,3PointArc,Circle,3PointCircle,Polyline,Rectangle,Slot,Fillet}`, `Sketcher_Trimming`, `Sketcher_Extend`, `Sketcher_ToggleConstruction`, the constraints `Sketcher_Constrain{Coincident,PointOnObject,Horizontal,Vertical,Parallel,Perpendicular,Tangent,Equal,Symmetric,Block,Lock,DistanceX,DistanceY,Distance,Radius,Diameter,Angle}`, `Sketcher_ToggleDrivingConstraint`, `Sketcher_SelectConflictingConstraints` (`crates/applications/src/apps/freecad/commands.rs`) |
| `freecad:menu:<File\|Edit\|View\|Tools\|Part Design\|Sketch\|Help>`, `freecad:menu-close`, `freecad:workbench`, `freecad:wb:<Part Design\|Sketcher>`, `freecad:overflow` | The menu bar (on macOS FreeCAD's File, Edit, View and Help hang from the Mac menu bar), the workbench selector, and the toolbar's overflow of tools that do not fit |
| `freecad:view:<w>:<h>` | The 3D view, a pointer drag surface painted at `w`×`h`. A click picks a face, edge or vertex (or, in a sketch, a point or edge, or places the active tool's next point); a left drag orbits, a right drag pans (Gesture; middle drag with OpenInventor), and in a sketch a drag on geometry moves it through the solver. `pointer.v1 wheel` over it zooms at the pointer |
| `freecad:navcube` (a click lands on the face, edge or corner under it), `freecad:navcube-arrow:<left\|right\|up\|down\|cw\|ccw>`, `freecad:navcube-menu`, `freecad:navcube-view:<command>` | The navigation cube: 26 facets turning the view, 15° steps, and its menu (orthographic, perspective, isometric, fit all) |
| `freecad:tree:<object>` (double click edits it), `freecad:tree-toggle:<object>`, `freecad:tree-eye:<object>`, `freecad:tree:origin:<XY_Plane…>` | The model tree: select, expand, show/hide, the body's Origin |
| `freecad:tab:<model\|tasks>`, `freecad:prop-tab:<view\|data>`, `freecad:prop:<property>` | Combo View tabs and the property editor. A number or text property opens its edit field, a boolean flips, an enumeration drops down its choices (`freecad:choice:prop/<property>:<value>`); a change recomputes everything downstream |
| `freecad:task:<ok\|cancel>`, `freecad:task:toggle:<option>`, `freecad:task:plane:<XY_Plane\|XZ_Plane\|YZ_Plane>`, `freecad:task:select:<add\|remove>`, `freecad:task:remove-ref:<i>`, `freecad:task:measure-clear`, `freecad:field:task:<parameter>`, `freecad:choice:open:task/<parameter>`, `freecad:choice:task/<parameter>:<value>` | Task panels: a feature's parameters (previewed live; Cancel restores the document), the plane chooser for a new sketch, a fillet's or chamfer's edge list with its radius or sizes (`ChamferType` Equal distance / Two distances / Distance and Angle, `Size`, `Size2`, `ChamferAngle`, `FlipDirection`, `UseAllEdges`), the Measure panel |
| `freecad:sk:constraint:<i>`, `freecad:sk:dim:<i>` (double click edits the value), `freecad:sk:element:<i>`, `freecad:sk:fold:<section>`, `freecad:sk:close`, `freecad:sk:select-free`, `freecad:sk:construction`, `freecad:sk:fillet-radius`, `freecad:field:constraint:<i>` | The Sketcher: constraint and element lists, dimension labels in the view, solver messages ("Under constrained: 2 DoFs" selects the free geometry), construction mode and the fillet tool's radius |
| `freecad:file:<entry:<name>\|open:<name>\|place:<folder>\|crumb:<folder>\|home\|back\|forward\|up\|new-folder\|folder-prompt:<default name>\|folder-create\|folder-cancel\|collapse\|type:<i>\|ok\|cancel\|replace\|keep\|list:<rows>\|scroll-by:<n>\|side-scroll-by:<n>\|scrollbar:<rows>:<height>\|side-scrollbar:<rows>:<total>:<height>>`, `freecad:field:file-name`, `freecad:field:folder-name`, `freecad:choice:open:filetype`, `freecad:choice:filetype:<i>`, `freecad:choice:open:filepath`, `freecad:choice:filepath:<folder>` | The platform's own file dialog, over the machine's real folders: the Mac's NSSavePanel/NSOpenPanel sheet (Save As and Tags, the folder pop-up `filepath`, Back/Forward, Favorites and Locations, File Format, New Folder sheet, the `collapse` disclosure), Windows 11's common item dialog (Back/Forward/Up, breadcrumb `crumb:` segments, New folder made at once as "New folder (n)", Home (`home`: the pinned Quick access folders), Desktop…Videos, This PC, Name/Type details, File name and Save as type) and GNOME's GTK chooser (Cancel/title/Save header bar, Name, path bar, places with Trash and Other Locations, Create Folder popover, filter drop-down). The sidebars list the same places as Finder, Explorer and Files. A click on a row (`entry:`) selects it; a double click (or `open:`) enters a folder or chooses a file; Open with a folder selected enters it. The list and the sidebar scroll: `pointer.v1 wheel` over them (a notch is three rows), a click on the scrollbar's track jumps there (the target carries the rows shown and the track height; the click's offset picks the row), Windows' arrows step a row (`scroll-by:`), and `ArrowUp`/`ArrowDown`/`PageUp`/`PageDown`/`Home`/`End` move the selection and keep it in view; a click on the list's empty space (`list:`) selects nothing. Saving over a listed file asks first (`replace` answers Replace/Yes, `keep` Cancel/No; `Enter` presses Cancel on macOS, No on Windows, Replace on GNOME). Documents (`*.FCStd.json`), import (STEP AP214/AP242 as an exact `Part::Feature`, STL, OBJ, DXF) and export (STEP AP214 `.step` and AP242 `.stp` of the exact solid, binary STL, ASCII STL `.ast`, OBJ, DXF of a sketch, hidden-line SVG of the view) |
| `freecad:dialog:<ok\|cancel\|save\|discard\|block>`, `freecad:field-cancel`, `freecad:nav-menu`, `freecad:nav:<Gesture\|OpenInventor>`, `freecad:report-close`, `freecad:report-clear` | Dialogs (a modal dialog answers clicks outside it with `block`), the navigation style menu and the report view |

Keys: `Ctrl+N/O/S/Shift+S/I/E/Z/Y/R/A`, `Delete`, in a file dialog `Enter`/`Escape` (answer the frontmost prompt, else OK/Cancel), `Alt+ArrowLeft`/`Ctrl+[` back, `Alt+ArrowRight`/`Ctrl+]` forward, `Alt+ArrowUp`/`Ctrl+ArrowUp` up, `Escape` (drops the tool in hand, then
the selection, then leaves the sketch or cancels the task), `Enter` (commits a field,
finishes a polyline, OKs a task), arrows pan and `PageUp`/`PageDown` zoom. With no
field focused, typing `0`–`6` turns to the standard views and a space toggles the
selection's visibility, as FreeCAD's single-key shortcuts do. Typed text goes to the
focused field; values take units (`25`, `25 mm`, `1 in`, `45 °`, `0.5 rad`).

KiCad controls (kind `kicad`), reached as `window:<id>:content:<target>`. KiCad opens one
window per frame — project manager, Schematic Editor, PCB Editor, Simulator, Symbol
Editor, Footprint Editor, 3D Viewer — all over the one open project. A control whose command cannot run now is painted disabled with
the reason; while a dialog is open only its own controls act:

| Target | Effect |
|---|---|
| `kicad:menu:<title>` | Open or close a menu of the frame's menu bar (`File`, `Edit`, `View`, `Place`, `Route`, `Inspect`, `Tools`, `Simulation`, `Help`) |
| `kicad:pm:new`, `kicad:pm:open`, `kicad:pm:close`, `kicad:pm:refresh`, `kicad:pm:folder` | Project manager: New Project and Open Project dialogs over `~/Documents/KiCad`, close the project, re-list its folder, show it in the file manager |
| `kicad:pm:launch:<sch\|pcb\|symed\|fped>`, `kicad:pm:file:<i>` | Open the Schematic, PCB, Symbol or Footprint Editor (or raise it); select a project tree row — a double click opens a `.kicad_sch`/`.kicad_pcb` in its editor |
| `kicad:canvas:sch:<x0>:<y0>:<zoom>:<w>:<h>` | The schematic sheet. The arguments are the view it was painted with (mils at the left/top edge, pixels per 1000 mils, canvas size). A drag surface: `pointer.v1 down`/`move`/`up` drive the current tool (select, box-select and drag-move with the select tool), a `click` is a press and release at one point, `move` with no button down draws the wire or symbol being placed under the pointer, and a `double_click` finishes a wire or opens a symbol's or label's properties (or enters a sheet). `pointer.v1 wheel` over any KiCad canvas zooms about the pointer; with `modifiers: ["shift"]` it pans up and down and with `["ctrl"]` left and right, and a sideways turn (`delta_x`) pans sideways, as KiCad's default mouse settings do |
| `kicad:sch:tool:<select\|symbol\|power\|wire\|label\|global\|hlabel\|noconnect\|junction\|bus\|entry\|sheet\|sheetpin>` | Schematic tools. Symbol and power open the symbol chooser (the project's own libraries first); a wire or bus ends on a pin, wire or bus, with a double click, or with `End`; a bus entry joins a wire to a bus at 45° (`R` turns it); a sheet is two clicks, corner to corner, then Sheet Properties (name and file); a sheet pin is a click on a sheet's edge and offers the sheet's hierarchical labels that have none; hierarchical labels belong inside a sub-sheet. Label names like `D[0..7]` name vector buses |
| `kicad:sch:<enter\|leave>`, `kicad:sch:enter:<id>`, `kicad:sch:goto:<i>` | Hierarchy: enter the selected sheet (or a double click on it), back to the parent (`Alt+Backspace`), jump to a sheet of the Hierarchy panel |
| `kicad:sch:<symed\|fped>` | Open the Symbol Editor or the Footprint Editor |
| `kicad:sch:<save\|undo\|redo\|rotate\|mirror-x\|mirror-y\|delete\|properties>` | Edit commands on the selection (keys `Ctrl+S`, `Ctrl+Z`/`Ctrl+Y`, `R`, `X`, `Y`, `Del`, `E`) |
| `kicad:sch:zoom:<in\|out\|fit\|objects>[:<view>]`, `kicad:sch:<grid\|units\|posture\|auto-annotate>` | View: zoom about the canvas centre (`F1`/`F2` zoom about the pointer, `Home`, `Ctrl+Home`); grid, display units, wire posture and automatic annotation toggles |
| `kicad:sch:<annotate\|erc\|netlist\|bom>` | Annotate Schematic, Electrical Rules Checker, Export Netlist (KiCad `.net` or SPICE `.cir`) and Generate BOM (`.csv`) dialogs |
| `kicad:sch:<simulator\|update-pcb\|pcb>` | Open the Simulator; Update PCB from Schematic (raises the PCB Editor with its update dialog); switch to the PCB Editor |
| `kicad:canvas:pcb:<x0>:<y0>:<zoom>:<w>:<h>` | The board, in micrometres and pixels per millimetre; the same pointer contract as the schematic canvas. The route tool starts on a pad or track, adds 45° corners on clicks and ends on a pad or track of the same net (or with a double click, `End`); `V` while routing drops a via and changes layer. The interactive router walks around pads, tracks, vias and rule areas of other nets with the board's clearance, in 45° segments; in highlight-collisions mode it draws straight 45° postures and rings what they would violate |
| `kicad:pcb:tool:<select\|route\|via\|zone\|keepout\|line\|rect>` | PCB tools; lines and rectangles are drawn on the active layer (a rectangle on `Edge.Cuts` is the board outline); a zone or rule area (keepout) closes on its first corner or a double click and asks for its net, layer and clearance (`kicad:dlg:keepout` turns a zone into a rule area) |
| `kicad:pcb:layer:<layer>`, `kicad:pcb:eye:<layer>` | Appearance panel: make a layer active, show or hide it |
| `kicad:pcb:<save\|undo\|redo\|rotate\|rotate-cw\|rotate45\|properties\|flip\|delete\|width\|grid\|posture\|ratsnest\|fill\|unfill>` | Edit and view commands (keys `R` 90° counter-clockwise, `Shift+R` clockwise, `Ctrl+R` 45°, `E` footprint properties with any angle, `F`, `Del`, `W`, `/`, `B`, `Ctrl+B`, `X` for the router, `PageUp`/`PageDown` for the copper layer) |
| `kicad:pcb:<router\|3d\|fped>` | Interactive Router Settings (`kicad:dlg:mode:<walkaround\|highlight>`), the 3D Viewer (`Alt+3`), the Footprint Editor |
| `kicad:pcb:zoom:<in\|out\|fit>[:<view>]`, `kicad:pcb:<update\|drc\|plot\|drill\|setup\|schematic>` | Zoom; Update PCB from Schematic, Design Rules Checker, Plot (Gerber RS-274X or SVG), Generate Drill Files (Excellon), Board Setup (design rules) dialogs; switch to the schematic |
| `kicad:sim:<run\|settings\|probe\|signals>`, `kicad:sim:cursor:<0\|1>`, `kicad:sim:toggle:<signal>` | Simulator: run, the analysis dialog (operating point, DC sweep, AC, transient), probe (arms the schematic's probe tool: a click on a wire or pin plots its voltage), Add Signals, show/hide a cursor, take a signal off the plot |
| `kicad:canvas:plot:<x0>:<x1>:<w>:<lin\|log>` | The plot area: a drag moves the nearest cursor along the x axis |
| `kicad:symed:<new-lib\|new-symbol\|save\|properties\|edit\|rotate\|delete\|delete-symbol\|undo\|redo\|cancel\|finish\|schematic>` | Symbol Editor: New Library (writes `<name>.kicad_sym` and `sym-lib-table`), New Symbol (name, reference, units), save (`Ctrl+S`; placed copies in the schematic follow), Library Symbol Properties (fields, units, simulation model, `Sim.Params`, power, pin name/number visibility), edit the selected pin (`E`), rotate it (`R`), delete (`Del`), undo/redo |
| `kicad:symed:lib:<name>`, `kicad:symed:open:<Lib:Name>`, `kicad:symed:unit:<n>`, `kicad:symed:tool:<select\|pin\|rect\|circle\|line\|text>`, `kicad:symed:zoom:<in\|out\|fit>[:<view>]` | Library tree, the symbol on the canvas, the unit drawn into, drawing tools (a pin click opens Pin Properties: name, number, length, unit, electrical type `kicad:dlg:kind:<keyword>`, orientation `kicad:dlg:orient:<0\|90\|180\|270>`; rectangles and circles are two clicks; a polyline ends with a double click or `Escape`) |
| `kicad:canvas:symed:<x0>:<y0>:<zoom>:<w>:<h>` | The symbol, in mils with the library's Y up; the select tool picks and drags pins and graphics, a double click opens a pin's or text's properties (or the symbol's) |
| `kicad:fped:<new-lib\|new-footprint\|save\|properties\|edit\|rotate\|delete\|delete-footprint\|fit-courtyard\|grid\|undo\|redo\|cancel\|finish\|board>` | Footprint Editor: New Library (`<name>.pretty/`, `fp-lib-table`), New Footprint, save (one `.kicad_mod` per footprint), Footprint Properties (description, body height for the 3D Viewer), edit or quarter-turn the selected pad, fit the courtyard to pads and body |
| `kicad:fped:lib:<name>`, `kicad:fped:open:<Lib:Name>`, `kicad:fped:tool:<select\|pad\|silk\|fab\|courtyard>`, `kicad:fped:zoom:<in\|out\|fit>[:<view>]` | Library tree, the footprint on the canvas, tools: a pad click opens Pad Properties (number, position, size, drill; `kicad:dlg:smd:<yes\|no>`, `kicad:dlg:shape:<circle\|rect\|oval\|roundrect>`); silkscreen lines chain until a double click; fabrication outline and courtyard are two clicks |
| `kicad:canvas:fped:<x0>:<y0>:<zoom>:<w>:<h>` | The footprint, in micrometres like the board canvas; the select tool picks and drags pads and silkscreen |
| `kicad:canvas:3d:<w>:<h>` | The 3D Viewer's image of the board: substrate with its drilled holes, mask, copper, silkscreen and extruded component bodies. A drag orbits (or pans in pan mode); the wheel zooms about the pointer |
| `kicad:v3d:view:<top\|bottom\|front\|back\|left\|right\|iso>`, `kicad:v3d:zoom:<in\|out\|fit>`, `kicad:v3d:mode:<orbit\|pan>`, `kicad:v3d:orbit:<left\|right\|up\|down>`, `kicad:v3d:toggle:<board\|copper\|mask\|silk\|bodies>` | 3D Viewer: standard views (`Z`, `Shift+Z`, `Y`, `Shift+Y`, `X`, `Shift+X`), zoom (`F1`, `F2`, `Home`), drag mode, 15° turns (arrow keys), Appearance check boxes |
| `kicad:dlg:<ok\|cancel>` and the dialog's own `kicad:dlg:<command>[:<arg>]`, `kicad:field:<name>` | Dialog buttons, list rows and check boxes; a field click focuses it for `keyboard.v1 type` (`Backspace`, `Enter` for OK, `Escape` to cancel) |
| `kicad:about` | Help ▸ About KiCad |

Typed letters with no dialog field focused are the frame's hotkeys (`W` wire, `A` symbol,
`P` power, `L` label, `Q` no-connect, `J` junction in the schematic; `R` run and `P`
probe in the simulator).

`kind` accepts `text_editor` as an alias for `editor` and `file_manager` for
`files`. Built-in window kinds are `browser`, `files`, `editor`, `terminal`, plus
the native applications (`calendar`, `mail`, `chat`, `docs`, `notes`, `contacts`,
`settings`, `calculator`, `clock`, `photos`, `music`, `maps`, `weather`, `code`, `freecad`, `kicad`, the image
editors `paint`, `preview`, `pixelmator`, `gimp`, `pinta`, `sketchbook`, the spreadsheets
`spreadsheet` and `excel`, the SQLite client `database`, and the video editors
`clipchamp`, `imovie`, `kdenlive` and `videoeditor`) listed by the
`native_apps!` macro in `crates/applications/src/apps/mod.rs`. A world may also
declare `desktop_apps` metadata aliases that launch a browser window at a fixed URL.
Launching a kind the machine does not have installed is `not_found`.

#### Video editor controls

The video editors are interfaces over one engine (`crates/video`, see
[video-editing.md](video-editing.md)): Clipchamp (`clipchamp`) on Windows 11, iMovie
(`imovie`) on macOS and iOS, Kdenlive (`kdenlive`) on Ubuntu and the Android video editor
(`videoeditor`). Launched with a folder as `argument` the import sheet starts there (the
default is `Videos` on Windows and Ubuntu, `Movies` elsewhere); a `.cwvideo` project or a
media file opens or imports it. Every control is `window:<id>:content:video:<command>`; a
command the product does not have (Clipchamp's reverse, iMovie's track lock) is refused,
and a control that cannot act now is painted disabled with its reason.

| Target | Effect |
|---|---|
| `video:play`, `video:start`, `video:end`, `video:step:<±n>`, `video:skip:<±n>`, `video:seek:<frame>` | Transport. Play toggles playback, which advances with the world clock (`sleep` in a shell moves it); step moves by frames, skip by five seconds (Clipchamp) |
| `video:shuttle:<j\|k\|l>` | J/K/L shuttle (iMovie on the Mac, Kdenlive): L plays forward at 1×, 2×, 4×; J backwards; K stops |
| `video:ruler:<scroll>` | The timeline ruler, a drag surface: press and drag to scrub. `<scroll>` is the first frame the ruler showed |
| `video:media:<id>:<ox>:<oy>:<lane>:<scroll>` | A media bin item, a drag surface. A click selects it; dragging it onto a timeline lane and releasing places a clip at that frame (snapping to clip edges and the playhead). `(ox, oy)` is the timeline lanes' origin relative to the item, `<lane>` the lane height. Released just above the top video lane (or below the last audio lane) it makes a new track |
| `video:clip:<id>:<lane>:<scroll>` | A timeline clip, a drag surface: click to select, drag sideways to move it (either edge snaps), up or down to another track of its kind. A drop onto another clip lands at that clip's nearer edge and pushes what follows (ripple insert). `<scroll>` is the first frame the lanes showed |
| `video:lanes:<scroll>` | The timeline's lanes behind the clips: a click deselects; the wheel over them (and over the ruler and clips) scrolls or zooms the timeline |
| `video:trim-in:<id>`, `video:trim-out:<id>` | The selected clip's trim handles, drag surfaces: move its in or out point, within the neighbouring clips and the media's length |
| `video:transition:<id>` | Select a transition (its duration then shows in the inspector) |
| `video:append:<media>`, `video:overlay:<media>` | Add media to the end of the main track (at the playhead in iMovie and on phones), or over the movie as picture in picture (Android) |
| `video:add-title:<plain\|headline\|lower\|top\|credits>`, `video:add-color:<rrggbb>`, `video:add-transition:<cross_dissolve\|dip_to_black\|dip_to_white\|wipe\|slide>` | Add a title (glyphs rasterised by the renderer in the platform font), a background colour clip, or a transition on the cut after the selected clip (else the cut nearest the playhead) |
| `video:split`, `video:delete`, `video:ripple-delete`, `video:undo`, `video:redo` | Edit: split at the playhead (the selected clip, else every clip it crosses), delete (closing the gap in iMovie and the Android editor), delete and close the gap (Kdenlive) |
| `video:set:<prop>:<value>`, `video:nudge:<prop>:<±n>`, `video:slider:<prop>:<width>` | Set a property of the selected clip — `opacity`, `x`, `y`, `scale`, `rotation`, `volume`, `brightness`, `contrast`, `saturation`, `temperature`, `speed` (25–400), `fade-in`, `fade-out`, `crop-left`/`-top`/`-right`/`-bottom`, or the selected `transition`'s length. The slider is a drag surface |
| `video:key:<prop>`, `video:ease:<prop>` | Add or remove a keyframe at the playhead; switch its interpolation between linear and ease (Kdenlive, Android) |
| `video:reverse`, `video:rotate`, `video:reset`, `video:mute-clip`, `video:pip:<corner>`, `video:fit`, `video:fill`, `video:ken-burns` | Clip commands; Fit, Crop to Fill and Ken Burns are iMovie's cropping modes |
| `video:title-text`, `video:title-bold`, `video:title-size:<±n>`, `video:title-color:<rrggbb>`, `video:title-bg:<rrggbbaa\|none>`, `video:title-pos:<top\|center\|lower\|bottom>` | The selected title; `title-text` focuses its text for `keyboard.v1 type` |
| `video:track-mute:<id>`, `video:track-hide:<id>`, `video:track-lock:<id>`, `video:add-track:<video\|audio>` | Track headers (per product: Kdenlive has all three, Clipchamp hide and mute) and Kdenlive's insert-track buttons |
| `video:snap`, `video:zoom-in`, `video:zoom-out`, `video:zoom-fit:<px>`, `video:zoom:<width>`, `video:scroll:<frame>` | Snapping; timeline zoom (the zoom slider is a drag surface) |
| `video:tab:<name>`, `video:inspector:<page>`, `video:deselect` | Browser tabs and inspector pages |
| `video:import`, `video:open`, `video:browse:<folder/\|..>`, `video:pick:<file>`, `video:pick-add:<file>`, `video:close-sheet` | The file sheet: import media (APNG, PNG, JPEG, WAV) or open a project; a phone's picker adds the clip to the movie |
| `video:new`, `video:save`, `video:save-as`, `video:save-confirm`, `video:name`, `video:project-name` | Projects, saved as `<name>.cwvideo` JSON in the media folder |
| `video:export`, `video:export-size:<w>x<h>`, `video:export-fps:<12\|24\|30>`, `video:export-start`, `video:export-cancel` | Export (Kdenlive's Render, iMovie's Share): an APNG and a WAV mixdown, encoded a few frames per simulation step |

Keys: `Space` play/pause, arrows step a frame, `Home`/`End`, `Delete`, `Ctrl+Z`/`Ctrl+Y`
(`Meta+Z`/`Meta+Shift+Z` on the Mac), `Ctrl+=`/`Ctrl+-` zoom, `Ctrl+S` save, `Ctrl+I`
import, `Escape` closes a sheet or clears the selection; split is `S` in Clipchamp,
`Meta+B` in iMovie and `Shift+R` in Kdenlive; `J`/`K`/`L` shuttle in iMovie and
Kdenlive; `Shift+Delete` ripple-deletes and `S` toggles snapping in Kdenlive. With no
field focused, typed letters are these shortcuts.

#### Music player controls

One player (`crates/applications/src/apps/music`) with five faces: Apple Music on macOS
and iOS, YouTube Music on Android, Media Player on Windows 11 and Rhythmbox on Ubuntu.
Everything on screen comes from the `media` service the world backs it with
(spotify.com, or music.youtube.com on Android): `GET /api/catalog` is the catalogue with
this listener's library, likes, playlists, history, player and speakers, and every
control is a real request, after which the catalogue is read again. Controls are
`window:<id>:content:music:<command>`.

| Target | Effect |
|---|---|
| `music:home`, `music:new`, `music:radio`, `music:library[:<recent\|playlists\|artists\|albums\|songs>]`, `music:liked`, `music:queue`, `music:back`, `music:expand` | Navigation; `expand` opens a phone's full-screen Now Playing |
| `music:album:<id>`, `music:artist:<id>`, `music:playlist:<id>`, `music:mood:<tag>`, `music:find:<tag>` | Open a collection; a mood chip filters Home |
| `music:play:<context>[@<track>]`, `music:shuffle-play:<context>` | Play a context (`album:<id>`, `playlist:<id>`, `artist:<id>`, `station:<id>`, `mood:<tag>`, `library`, `liked`, `charts`, `track`), optionally starting at a track |
| `music:toggle`, `music:previous`, `music:next`, `music:shuffle`, `music:repeat`, `music:seek:<n>`, `music:jump:<i>` | Transport; `seek` names a thousandth of the track, `jump` a queue position |
| `music:volume:<pct>`, `music:mute`, `music:volume-popover` | The player's own volume, which the machine's output volume then scales (Rhythmbox's volume button opens the popover). On a phone the Now Playing slider is the phone's own volume instead, `shell:set:volume:<pct>`, until the music is playing on a speaker |
| `music:output`, `music:output:<device id>`, `music:output:` | AirPlay (macOS, iOS), Cast (Android) or Cast to device (Windows). Opening the picker asks each of the account's speakers whether it is on the network; picking one hands it the session (`POST <speaker>/api/cast`) and then tells the service (`output`); the empty id brings the music back to this device |
| `music:lyrics`, `music:lyric:<i>`, `music:lyrics-sheet` | Time-synced lyrics: the panel in Apple Music, the LYRICS tab in YouTube Music. The sung line follows the world clock, and a line seeks to where it is sung |
| `music:like:<id>`, `music:save:<id>`, `music:save-album:<id>`, `music:subscribe:<id>`, `music:play-next:<id>`, `music:play-last:<id>` | Likes, library, following, and the play queue |
| `music:menu:<id>`, `music:menu-close`, `music:menu-playlists`, `music:add:<playlist>`, `music:remove:<playlist>@<track>` | The song menu ("…", "⋮") and Add to Playlist |
| `music:search-field`, `music:search`, `music:clear`, `music:compose`, `music:create`, `music:cancel` | Search and the New Playlist sheet |
| `music:filter-artist:<id\|all>`, `music:filter-album:<id\|all>` | Rhythmbox's Artist and Album browser |
| `music:reload`, `music:popup-close` | Re-read the catalogue; close a popup |

Keys: `Space` (or `MediaPlayPause`) toggles playback with no field focused,
`MediaTrackNext`/`MediaTrackPrevious` skip, `Enter` searches or creates, `Escape` closes
a menu, a popup or the composer.

#### Image editor controls

The image editors are interfaces over one engine (`crates/raster`): Windows 11 Paint
(`paint:`), macOS Preview (`preview:`) and Pixelmator Pro (`pixelmator:`), GIMP
(`gimp:`) and Pinta (`pinta:`) on Ubuntu, Sketchbook (`sketchbook:`) on Android, and
the phones' photo editors inside Photos (`photos:edit:`). Launched with an image path
as `argument`, an editor opens that file (PNG or JPEG; BMP on the desktops; GIMP's
layered XCF in GIMP). Every control is
`window:<id>:content:<prefix>:<command>`; a command the product does not have (Paint's
Gaussian blur, GIMP's shape tools) is refused.

| Command | Effect |
|---|---|
| `tool:<tool>` | `select-rect`, `select-ellipse`, `lasso`, `wand`, `move`, `crop`, `pencil`, `brush`, `airbrush`, `pen`, `marker`, `highlighter`, `eraser`, `fill`, `text`, `picker`, `zoom`, `pan`, `shape`, `gradient` (GIMP, Pinta, Pixelmator), `clone` (GIMP, Pinta's Clone Stamp, Pixelmator), `heal` (GIMP), `repair` (Pixelmator: paint over something and it is rebuilt from its surroundings on release), `paths` (GIMP) — each product offers its own subset |
| `shape:<kind>` | Shape tool: `line`, `arrow`, `rectangle`, `rounded-rectangle`, `ellipse`, `polygon`, `triangle`, `right-triangle`, `diamond`, `pentagon`, `hexagon`, `right-arrow`, `left-arrow`, `up-arrow`, `down-arrow`, `star`, `heart`, `curve` (Paint: drag a line, then two drags bend it), `freeform` (Pinta), `polyline`. Pinta's `line` stays editable after the drag: drag its points, press on it to add one, `Enter` (or `curve-edit:commit`) draws it, `Escape` (`curve-edit:cancel`) drops it |
| `gradient-shape:<linear\|bilinear\|radial\|square\|diamond\|conical-sym\|conical-asym>`, `gradient-repeat:<none\|sawtooth\|triangular\|truncate>`, `gradient-colors:fg-bg\|fg-transparent`, `gradient-reverse` | Gradient options. A drag lays the ramp from press to release through the selection, previewed on the canvas while dragging |
| `aligned[:on\|off]`, `clone-source:<x>:<y>` | Clone and heal: aligned (the offset of the first stroke is kept) or not (every stroke starts from the source). The source is set by a modifier-click on the canvas — `pointer.v1` `modifiers: ["ctrl"]` in GIMP and Pinta, `["alt"]` (Option) in Pixelmator — or by `clone-source` |
| `path:select\|fill\|stroke\|close\|delete` | GIMP's path: Select ▸ From Path, Edit ▸ Fill Path / Stroke Path… (a `stroke-path` dialog with `line-width`), close it, delete it. With the Paths tool a click adds an anchor, a drag from it pulls handles, a drag on an anchor or handle moves it, Ctrl-click on the first anchor closes the path |
| `outline:none\|solid`, `fill:none\|solid`, `fill-style:outline\|fill\|both`, `antialias`, `bold`, `merged`, `mode:<replace\|add\|subtract\|intersect>` | Tool options |
| `color:<rrggbb>`, `fg:<rrggbb>`, `bg:<rrggbb>`, `slot:1\|2`, `swap-colors`, `reset-colors` | Colours: the active slot, foreground, background (Paint's Color 1 and Color 2) |
| `set:<param>:<value>` | Set a tool option (`size`, `hardness`, `opacity`, `tolerance`, `font-size`, `zoom`, `layer-opacity`), a field of the open dialog, or a phone editor's adjustment |
| `undo`, `redo`, `new`, `open`, `open:<name>`, `folder:<name>`, `folder-up`, `save`, `save-as`, `save-as:<ext>`, `export`, `overwrite`, `format:<png\|jpg\|bmp\|xcf>`, `xcf-compression`, `save-confirm`, `cancel`, `close-panel` | History and files. The save sheet's name field takes typing; its extension is the format and `format:` chips rewrite it. PNG goes through the environment's encoder; JPEG (the engine's baseline encoder), BMP and XCF are encoded by the engine and written as bytes. Paint saves PNG, JPEG and BMP; Preview and Pixelmator export PNG and JPEG with a quality slider (`set:jpeg-quality:<1-100>`); Pinta saves PNG, JPEG (then a `jpeg-quality` dialog) and BMP; GIMP saves XCF (layers, modes, opacity, visibility; RLE tiles, or zlib with `xcf-compression`), exports PNG, JPEG (then its `jpeg` dialog with `quality` and `subsampling` 4:4:4/4:2:0) and BMP, and `overwrite` re-exports the file it came from. A file is only saved in place in a format the product saves |
| `select-all`, `select-none`, `select-invert`, `delete`, `crop-selection`, `crop-apply`, `copy`, `cut`, `paste` | Selection and clipboard. Copy puts pixels on the machine clipboard (shared by every editor); paste adds them as a new layer |
| `rotate:cw\|ccw\|180`, `flip:h\|v`, `flatten` | Whole-image operations |
| `dialog:<id>`, `apply`, `reset`, `action:<id>` | Parameter dialogs (`brightness-contrast`, `exposure`, `levels`, `curves`, `hue-saturation`, `saturation`, `color-balance`, `temperature`, `shadows-highlights`, `threshold`, `posterize`, `gaussian-blur`, `box-blur`, `sharpen`, `unsharp-mask`, `median`, `noise-reduction`, `pixelate`, `vignette`, `resize`, `rotate`, `new-image`, `color`, `adjust-color`) preview their adjustment or filter on the canvas while their values change (GIMP's `preview-toggle` is its Preview checkbox) and change nothing until `apply`, which commits exactly what was previewed; `cancel` leaves the image as it was. `curves` has a `channel` (0 value, 1 red, 2 green, 3 blue); one-shot actions are `invert`, `grayscale`, `auto-levels`, `sepia`, `edge-detect`, `emboss`, `sharpen` |
| `layer:new\|delete\|duplicate\|up\|down\|merge`, `layer:select:<i>`, `layer:toggle:<i>`, `layer:blend:<mode>`, `layers` | Layers (bottom layer is 0); blend modes `normal`, `multiply`, `screen`, `overlay`, `add`, `darken`, `lighten` |
| `zoom:in\|out[:<w>:<h>]`, `zoom:fit`, `zoom:<percent>` | View zoom |
| `menu:<id>`, `tab:<id>`, `text:commit\|cancel` | Open an editor's own menu or panel tab; finish or drop text being typed |
| `focus:<adjustment>`, `preset:<id>`, `look:reset`, `look:aspect:<id>`, `look:rotate`, `look:flip`, `look:done` | Phone editors: pick the adjustment the dial or slider drives, a filter or suggestion, crop to an aspect, and save (iOS Done overwrites a PNG; Google Photos' Save copy writes `<name>-edited.png`) |
| `photos:begin-edit:ios\|android`, `photos:edit-with:<kind>`, `photos:edit:discard` | Photos' Edit button: edit in place on a phone, or open the photo in the platform's installed editor on a desktop (announced disabled when none is installed) |

**Drag surfaces.** `canvas:<w>:<h>` (the image view), `slider:<param>:<width>`,
`dial:<param>`, `curve:<w>:<h>`, `hscroll:…` and `vscroll:…` follow the pointer:
`pointer.v1 down` on one captures the pointer — on a phone too, before any gesture —
and every `move` and the `up` are delivered to it relative to where it was painted,
even outside it. A brush paints along the drag, a selection or shape spans it, a
slider takes the value under the pointer. `click` on a surface is a press and release
at one point (a dot, a fill, a colour pick). The text tool types where it was
clicked; `keyboard.v1 type` fills it and `Enter` stamps the glyphs, rasterised in the
platform's font by the renderer. A drag on the canvas reports the `crosshair` cursor.
`pointer.v1 move` over the canvas with no button down is delivered to it too: Paint,
GIMP and Pinta show the pixel under the pointer in their status bars, and the desktop
editors outline the brush tip (and a clone's source) under the pointer. The Move tool
shows the layer moving during the drag.

#### Spreadsheet controls

`spreadsheet` is each platform's own spreadsheet over one engine (`crates/sheet`):
Excel on Windows, Numbers on macOS and iOS, LibreOffice Calc on Ubuntu and Google
Sheets on Android; `excel` is Microsoft Excel as a second application on the Mac.
Launched on nothing they open on `~/Documents` (Excel's Open page, Numbers' document
browser, Calc's Start Center); launched with a `.xlsx`, `.ods` or `.csv` path they open
that file. Files are real bytes on the machine: new workbooks save as XLSX, or ODS from
Calc; a file keeps its own format when saved again. Every control is
`window:<id>:content:sheet:<command>`.

| Command | Effect |
|---|---|
| `new:<excel\|numbers\|calc\|sheets>`, `open`, `openfile:<name>`, `folder:<name\|..>`, `closelist`, `save:<product>`, `savecsv`, `savexlsx` | Files: a blank workbook, the document list, a file from it, and saving (`Ctrl+S`). `savecsv` writes the current sheet as CSV |
| `select:<A1\|A1:B2>`, `col:<letter>`, `row:<n>`, `all`, `namebox`, `tab:<i>`, `rename[:<i>]`, `scroll:<up\|down\|pageup\|pagedown\|left\|right>` | Selection and navigation. The Name Box takes a reference or a name (a new name is defined for the selection); typing renames a tab |
| `edit[:bar]`, `enter`, `cancel`, `insertfn:<NAME>`, `autosum` | The cell editor and formula bar. Typing over a selected cell starts editing; `Enter`/`Tab` commit (an unclosed parenthesis is closed, as Excel does); a formula that does not parse is kept open with the reason |
| `cut`, `copy`, `paste`, `undo`, `redo`, `clear`, `clearall`, `clearformats`, `filldown`, `fillright` | Editing. Copy puts tab-separated text on the machine clipboard; pasting it back keeps formulas and formats, with references adjusted |
| `bold`, `italic`, `underline`, `align:<left\|center\|right>`, `fmt:<general\|number\|currency\|accounting\|percent\|comma\|date\|longdate\|time\|scientific\|text>`, `dec:<more\|less>`, `fill:<rrggbb\|none>`, `color:<rrggbb\|none>` | Formatting the selection |
| `insert:<rows\|cols\|sheet>`, `delete:<rows\|cols\|sheet\|chart>`, `colwidth:<col>:<px>`, `autofit`, `freeze:<panes\|row\|col\|none>` | Structure. Inserting or deleting rows and columns rewrites every reference to them |
| `sort:<asc\|desc>`, `filter`, `filterpick:<col>`, `filtertoggle:<col>:<value>` | Sort the current region by the active column (a text header row stays put); AutoFilter with a value list per column |
| `chart:<column\|bar\|line\|pie>`, `chartsel:<i>`, `charttype:<kind>` | Charts of the selection (or the data around the active cell) |
| `merge:<center\|across\|cells\|down>`, `merge!:<mode>`, `unmerge` | Merge & Center, Merge Across (one merge per row), Merge Cells and Sheets' Merge vertically. Merging cells that hold more than the top-left value asks first (`merge!` is the confirmed form) and keeps only that value; a merge is selected, navigated and sorted around as one cell, as Excel does. Center Across Selection is `Align::CenterAcross` |
| `border:<bottom\|top\|left\|right\|none\|all\|outside\|thickoutside\|inside\|insideh\|insidev\|doublebottom\|thickbottom\|topbottom\|topthickbottom\|topdoublebottom>`, `borderline:<thin\|medium\|thick\|double\|dotted\|dashed\|hair\|mediumDashed\|dashDot\|mediumDashDot\|dashDotDot\|mediumDashDotDot\|slantDashDot>`, `bordercolor:<rrggbb\|auto>`, `drawborder:<border\|grid\|erase\|off>` | Cell borders: Excel's presets in the current line style and colour; Draw Border mode draws on the grid by dragging |
| `cf:<greater\|less\|between\|equal\|text\|date\|duplicate\|top\|toppct\|bottom\|bottompct\|above\|below\|formula>`, `cf:bar:<rrggbb>`, `cf:scale:<id>`, `cf:icons:<set>`, `cfclear:<selection\|sheet>`, `cfmanage`, `cfrule:<i>`, `cfdelete`, `cfup`, `cfdown`, `cfstop` | Conditional formatting: rule dialogs (Highlight Cells, Top/Bottom, New Rule with a formula), data bars, colour scales, icon sets, and the Rules Manager (select, delete, reorder, Stop If True) |
| `dialog:<ok\|cancel>`, `dialog:field:<i>`, `dialog:preset:<id>`, `dialog:choice:<id>`, `dialog:<newsheet\|existing>`, `dialog:delim:<tab\|comma\|semicolon\|space>`, `dialog:mergeruns` | The open dialog: typing fills the focused field, `Tab` moves, `Enter` is OK and `Escape` Cancel. A refused OK reports why and keeps the dialog |
| `pivot:<new\|refresh\|refreshall\|delete\|pane>`, `pivotfield:<k>`, `pivotarea:<k>:<rows\|cols\|values\|filters\|remove>`, `pivotagg:<i>:<sum\|count\|average\|max\|min>`, `pivotfilter:<k>`, `pivothide:<k>:<item>` | Pivot tables from the selection, onto a new or an existing sheet; the field list ticks a field into Rows (text) or Values (numbers), moves it between areas, changes the summary and hides items. Refresh rereads the source; Sheets and Numbers refresh on every change |
| `calcnow`, `ttc` | Calculate Now (`F9`) and Text to Columns on one column |
| `menu:<id>`, `ribbon:<tab>`, `inspector[:<pane>]`, `zoom:<in\|out\|reset\|percent>`, `gridlines`, `dismiss`, `noop` | Menus, Excel's ribbon tabs and backstage, Numbers' Format and Organize sidebar, view settings, the message dialog |

**Drag surfaces.** `sheet:grid:<row height>:<scale>` is the cell area and
`sheet:fill:<row height>:<scale>` the fill handle at the corner of the selection. A drag
on the grid selects from the press to the release (the press stays the active cell);
while a formula is waiting for an argument (`=SUM(`) a drag inserts the range instead. A
drag from the fill handle fills the selection in the direction dragged furthest,
continuing number and date series and adjusting relative references.
`sheet:chartmove:<i>:<row height>:<scale>` is a chart: pressing selects it and a drag
moves it, landing on the cells (and offsets into them) under its new corners.
`sheet:chartsize:<i>:<handle>:<row height>:<scale>` are the eight sizing handles of the
selected chart, clockwise from the top-left (0) to the left edge (7). In Draw Border
mode a drag on the grid draws the outline (or grid, or erases) of the cells it spans.
While a formula is typed its references are coloured in the editor and their cells
outlined in the same colours. A double click on
the grid edits the active cell; on a phone a tap selects and a second tap (a double
click) edits. Keys follow Excel with `Meta` as `Ctrl`: arrows (with `Shift` to extend,
`Ctrl` to jump to the edge of the data), `Tab`, `Enter`, `F2`, `Delete`, `Backspace`,
`PageUp`/`PageDown`, `Ctrl+Home`/`End`, `Ctrl+A`, `Ctrl+C`/`X`/`V`, `Ctrl+Z`/`Y`,
`Ctrl+B`/`I`/`U`, `Ctrl+D`/`R`, `Alt+=` and `Ctrl+S`.

#### Database controls

`database` is DB Browser for SQLite on Windows and Ubuntu and TablePlus on macOS,
over the `crates/sql` engine; phones have none. Launched with a `.db`, `.sqlite`,
`.sqlite3` or `.db3` path it opens that SQLite file. Changes stay in the open
connection until Write Changes (TablePlus's Commit, `Ctrl+S`) writes the whole file;
Revert Changes returns to what the file holds. Every control is
`window:<id>:content:db:<command>`.

| Command | Effect |
|---|---|
| `new`, `open`, `openfile:<name>`, `folder:<name\|..>`, `cancel`, `write`, `revert`, `close`, `savechanges`, `discard`, `import`, `exportcsv` | Files. `new` creates `Untitled.db` in the folder at once; `import` makes a table from a CSV file (typed INTEGER, REAL or TEXT by its values); `exportcsv` writes the browsed table as `<table>.csv`; closing with unwritten changes asks first |
| `tab:<structure\|browse\|pragmas\|execute>`, `menu:<file\|edit\|view\|tools\|tables>`, `dismiss`, `noop` | DB Browser's four tabs and menus; TablePlus's Data and Structure views |
| `expand:<node>`, `tree:<table:NAME\|index:NAME\|view:NAME\|trigger:NAME>`, `droptable[:<name>]`, `yes`, `no` | The Database Structure tree (Tables, Indices, Views, Triggers); Delete Table asks before it drops |
| `table:<name>`, `cell:<row>:<col>`, `editcell[:<row>:<col>]`, `setnull`, `newrow`, `deleterow`, `sort:<col>`, `filter:<col>`, `clearfilters`, `refresh`, `page:<first\|prev\|next\|last>` | Browse Data. An edited cell is an `UPDATE … WHERE rowid = ?` (by primary key in a `WITHOUT ROWID` table, whose rows are shown in key order), so the column's type affinity and the table's constraints decide what is stored (a refused edit is reported). A filter is `LIKE %text%`, or a comparison when it starts with `=`, `<`, `>`, `<=`, `>=`, `<>` or `!=`. Views are read-only |
| `sql[:start]`, `run`, `runline`, `clearsql`, `results:<up\|down>` | Execute SQL: the editor (typing, `Enter`, arrows), Execute all (`F5`, `Ctrl+Enter`, `Ctrl+R`) and Execute current line (`Shift+F5`); the grid shows the last statement that returned rows and the message pane reports rows, changes or the error with its line |
| `pragma:foreign_keys`, `pragma:user_version:<up\|down>`, `integrity` | Edit Pragmas and Tools › Integrity Check |
| `createtable`, `modifytable[:<name>]`, `design:<name\|add\|remove\|top\|up\|down\|bottom\|withoutrowid\|ok\|cancel>`, `design:cell:<field>:<col>`, `design:type:<field>:<INTEGER\|TEXT\|BLOB\|REAL\|NUMERIC>` | DB Browser's Edit Table Definition dialog. Columns are Name, Type, NN, PK, AI, U, Default, Check, Collation, Foreign Key (0–9): a click on a text cell selects it (typing replaces it, a double click edits at the end), on a check box toggles it; `Tab` moves along the text cells and `Enter` leaves the cell, then is OK. OK writes the `CREATE TABLE` DB Browser writes, or for a change `ALTER TABLE` (rename table or column, add or drop column) where SQLite can and otherwise SQLite's twelve-step rebuild (`sqlb_temp_table_N`, copy, drop, rename, indexes, triggers and views back, `foreign_key_check`). A refused change is reported and changes nothing |
| `createindex[:<table>]`, `index:<name\|unique\|ok\|cancel>`, `index:table:<name>`, `index:col:<column>`, `index:order:<i>` | Edit Index Definition: name, table, Unique, the index columns (a click adds or removes one) and each one's ASC/DESC. The partial index clause is painted disabled: the engine has no partial indexes |
| `struct:cell:<row>:<col>`, `struct:addcol`, `struct:delcol`, `struct:dropindex:<name>` | TablePlus's Structure view: column_name, data_type, is_nullable, column_default, primary_key, foreign_key (0–5) are edited in place and staged; Commit (`Cmd+S`) applies them as above and writes the file, Discard drops them |

A double click on a grid cell edits it; `Enter` or `Tab` commits, `Escape` cancels,
`Delete` sets NULL. The shell's `sqlite3` works on the same files.

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
| `down` | same, plus `{"button"?: u64, "modifiers"?: ["ctrl" \| "alt" \| "shift" \| "meta"]}` (`modifiers` is accepted on every op; an unknown name is `invalid`) | `null` |
| `up` | same | `null`, `{"cursor": …}` while a drag is captured, or the target's own result |
| `click` | same | the target's result |
| `double_click` | same | the target's result |
| `cancel` | same | `null` |
| `wheel` | same, plus `{"delta_y": i64, "delta_x"?: i64, "modifiers"?: ["Shift"\|"Ctrl"\|"Meta"]}` in pixels (positive `delta_y` rolls towards the user and moves content up; 120 is one notch) | `{"handled": bool}`: whether anything under the pointer moved |

`x`/`y` are clamped to ±32768; `width`/`height` default to the machine's screen as the
actor last addressed it (the last `width`/`height` a pointer action stated), or before
any to the shell's native screen (390×844 on iOS, 412×915 on Android, 1280×800 on a
desktop), and are capped at 8192. That same last-addressed size, with its orientation,
is what a screenshot (`shell:screenshot`, Recents' Screenshot) captures. Coordinates are in the same viewport you pass to `scene(width, height)` —
hit testing runs against that scene, so the actor aims using only what it can see.
`cursor` is one of `default`, `pointer`, `text`, `grab`, `grabbing`, `crosshair` (an
image editor's canvas), `ns-resize`, `ew-resize`, `nesw-resize`, `nwse-resize`.

Behaviour worth knowing:

- **Scrolling is a platform service.** A pane whose content is taller than its view is
  published in `Scene::scrolls` (`ScrollArea`: `target`, `window`, `bounds`, `offset`,
  `extent`, `horizontal`, and on iOS the large `title` that collapses into the navigation
  bar once `offset >= title_height`). A `wheel` first goes to the application's own use of the
  wheel, then to the innermost pane under the pointer that can still move that way,
  then outwards. Applications with their own wheel: Visual Studio Code's editor and
  terminal (whole rows), spreadsheets (three rows a notch, Shift for columns, Ctrl
  zooms), SQLite grids (rows), KiCad canvases (zoom about the pointer; Shift pans up and
  down, Ctrl left and right, as KiCad's defaults), image editors' canvas (scroll, Shift
  across, Ctrl zooms about the pointer), FreeCAD's 3D view (zoom), terminals (lines),
  and the video editors' timelines (`video:lanes`, `video:ruler`, `video:clip`: the
  wheel scrolls through time, Shift or a sideways wheel too, and Ctrl/Cmd zooms about
  the frame under the pointer; a view put somewhere by hand stays there until the
  playhead moves or plays). A plain-text editor's rows are the `pane:text` area: the
  view scrolls by whole rows independently of the caret, and an edit or caret move
  brings the caret back into view; `editor-text:<first row>` always names the row
  painted first. A video editor's media bin is the `pane:bin` area.
  The browser's page is the `pane:page` area; its offset is `browser.v1 scroll`'s `y`.
  Offsets are window state (`Window::scroll`) and survive snapshots.
- **Some panes scroll sideways.** A shelf of album covers (Apple Music's and YouTube
  Music's shelves, and a page `Row` with `Style::scroll_x`) is published `horizontal`:
  its `offset` and `extent` run along x. `delta_x` moves it, and so does `delta_y` with
  Shift held; a plain `delta_y` passes it by to the upright pane around it. On a page
  such a shelf is `pane:row:<row id>`, which `browser.v1 scroll` also takes by name.
- **Scroll bars are real.** On desktops each overflowing pane paints a thumb sized to
  the real extent, on a drag surface `window:<id>:content:pane:<name>:<track>:<thumb>:<max>`
  (`hpane:…`, along the bottom edge, for one that scrolls sideways):
  dragging the thumb scrolls, a press on the track jumps the thumb there, and a named
  click without a point pages forward. Phones paint no bar at rest, as they do not.
- **Phones scroll under the finger.** A touch that moves more than 12 px mostly
  vertically, starts inside the application in front and is no shell gesture (not from
  the status bar, the home indicator or the navigation bar) takes the pane it started
  on and taps nothing. Every `move` then scrolls it so the content stays under the
  finger (an application's own wheel use, a grid or a scrollback, moves a row at a
  time); pulled past an end, the content follows with iOS's rubber-band resistance
  (`Scroll::stretch`) and springs back on release. On `up` the pane flings on: the
  velocity is the last two finger samples over the world-clock time between them (a
  60 Hz frame when the clock did not move), and the distance is the platform's
  deceleration (iOS 0.998/ms; Android's OverScroller spline), stopping at an end. A
  finger that rested 40 ms of world clock before lifting does not fling. A `down`/`up`
  with no `move` between scrolls by exactly the distance moved. A mostly sideways touch
  moves a shelf that scrolls sideways under it the same way.
- **A phone's Back is the application's first.** Android's Back button (`shell:mobile-back`)
  and iOS's navigation-bar chevron go to the application's parent screen before leaving
  it: Mail's message to its mailbox, a mailbox (iOS) to Mailboxes, Gmail's drawer
  closed, a conversation to the Messages list, a note or document to its list, a
  contact to Contacts.
- **Text focus is one answer.** A native application reports the text field that has
  the focus (`focus.keyboard.target`, role `textbox`); a phone paints its keyboard
  exactly when there is one, and on a phone `keyboard.v1 type` with none reaches
  nothing. Messages' composer and a note's or document's body take the focus when
  tapped on a phone, and whenever their conversation or document is open on a desktop.

- **`down` then `up` is a click with press identity.** `down` records the hit target
  and its bounds; `up` only fires if released inside those bounds. Dragging out
  cancels, as on a real pointer.
- **Double-click is real.** On a desktop theme, a single `click` on a desktop icon or
  file-manager row *selects*; `double_click` *opens*. On `ios`/`android` themes there
  is no such distinction and a tap opens immediately. `double_click` on a window
  title bar (`window:<id>:drag`) maximizes.
- **`button: 2` on `down`** opens the context panel — except over an application drag
  surface that uses the secondary button (FreeCAD's 3D view pans with a right drag).
  `button` is passed to drag surfaces, so a middle drag can differ from a left one.
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
    down closes it. Sideways on the home screen walks Pixel Launcher's pages (the dots
    above the hotseat are `shell:home-page:<n>`); in Recents it walks the carousel,
    and past the oldest card is Clear all. Swipes that start on the 48 px navigation
    bar are presses of its buttons: Back (`shell:mobile-back`), Home (`shell:home`),
    Recents (`shell:overview`).
  - On both, a card swiped up in the overview / App Switcher closes that application.
    Tapping the space around the cards goes home.
- Window `drag` and `resize:<edge>` operations capture the pointer between `down` and
  `up`; while captured, `move`/`up` bypass hit testing. So do an application's drag
  surfaces (an image editor's canvas and sliders; see *Image editor controls*), and a
  drag that starts on one is never taken for a touch gesture.

#### Interaction targets

Scene nodes carry an `interaction` string. Clicking one is how everything in the
shell is driven, so these strings are part of the actor-facing contract.

| Prefix | Meaning |
|---|---|
| `window:<id>:drag` \| `:focus` \| `:minimize` \| `:maximize` \| `:close` \| `:resize:{n,s,e,w,ne,nw,se,sw}` | Window frame. Requires `application.v1`. |
| `window:<id>:content:<target>` | Forwarded into the window's content as `<target>` |
| `shell:launch:<kind>` | Launch an application (same path as `application.v1 launch`) |
| `shell:open:<kind>` | Desktop icon: select on click, launch on double-click (`trash` opens the trash folder) |
| `shell:home`, `shell:launcher`, `shell:switcher`, `shell:minimize`, `shell:maximize`, `shell:close`, `shell:desktop`, `shell:dismiss`, `shell:noop`, `shell:new`, `shell:save`, `shell:mobile-back` | Shell commands |
| `shell:panel:<name>` where name ∈ `apple file edit view go window help spotlight search control quick calendar clock notifications settings overview context power` | Toggle a panel. `shell:menu:<Name>`, `shell:search`, `shell:spotlight`, `shell:settings`, `shell:overview`, `shell:recents`, `shell:notifications`, `shell:control-center`, `shell:quick-settings`, `shell:system` are aliases. |
| `shell:toggle:<setting>` | Flip a device switch (wifi, bluetooth, airplane, dark, …). Returns `{"setting","value"}`. |
| `shell:set:<setting>:<percent>` | Set a level 0–100. Returns `{"setting","value"}`. |
| `shell:power:{lock,off,shutdown,restart,wake,unlock}` | Changes what the screen actually shows; `off`/`restart` clear windows |
| `shell:recents:slot:<n>`, `shell:recents:clear`, `shell:recents:select`, `shell:recents:copy`, `shell:recents:screenshot` | Android Recents: centre card `<n>` (oldest 0; -1 is the Clear all slot); close every application; Select mode on the centred card (its text is outlined); copy that card's text (read from the application's semantic page) to the clipboard, returning `{"copied"}`; capture the centred application alone to `~/Pictures`. Refused unless Recents is open. |
| `shell:tab:new`, `shell:tab:select:<i>`, `shell:tab:close:<i>` | **Browser** tabs; re-dispatched as `browser.v1 new_tab`/`switch_tab`/`close_tab` |
| `window:<id>:content:files-newtab` \| `files-tab:<i>` \| `files-closetab:<i>` | **File manager** tabs. They live in the window, not the browser session, so they use different targets. |
| `window:<id>:content:files-{home,up,root,back,forward,reload,path}`, `files-location:<path>` | File manager navigation |
| `files-view`, `files-sort:{name,kind,size,modified}` | List/grid toggle, and the sort key (`date` is an alias of `modified`). The same key again reverses it. Both only reorder the view. Size and Date modified sort on the metadata the listing really carried; a row the machine said nothing about sorts last. GNOME Files opens in the grid, where the selected or a starred item carries a `files-star:<i>` button and the context menu offers Star/Unstar (`files-star`) and Open (`files-open`) on the selection. |
| `files-search`, `files-search-clear` | Focus the query field, and clear it. Typing goes to `FileTab::query`, which filters the rows. |
| `files-{new-folder,new-file,cut,copy,paste,rename,delete}` | File manager mutations. Each runs through the kernel under the same access checks as `read_file`/`write_file`, and re-lists the folder afterwards. `delete` moves to `~/.local/share/Trash/files` and writes the `.trashinfo` record beside it; nothing is hard-removed. `rename` opens a field committed with `Enter` and cancelled with `Escape`. |
| `files-move-to-trash`, `files-restore`, `files-restore:<i>`, `files-empty-trash` | The trash, as the FreeDesktop spec has it. `files-move-to-trash` is `files-delete` under the name the action really has, and the `Delete` key dispatches it. `files-restore` puts the selection back where its `.trashinfo` record says it came from, and `files-restore:<i>` names on-screen row `<i>` instead; both are refused outside the Trash, and an ambiguous record names its candidates rather than guessing. `files-empty-trash` destroys what is in the trash for good and is refused anywhere else. Every one of them goes through `Kernel::trash_path`, `Kernel::restore_path` and `Kernel::empty_trash`, the same trash the shell's `trash`, `trash-list`, `trash-restore`, `trash-empty` and `gio trash` commands use. |
| `files-recents`, `files-browse` | Switch the tab between the desktop's recent-documents list and the folder it was showing. |
| `files-starred`, `files-star`, `files-star:<i>` | Files' Starred list (`DesktopState::starred`), and star or unstar the selection or on-screen row `<i>`. A star toggles; unstarring in the Starred list takes the row off it. |
| `files-quick-access`, `files-gallery` | Explorer's Home (the pinned folders the home folder really holds, then favourites, then recent documents) and Gallery (the image files in `~/Pictures`). Both are views over a real listing, not folders, so mutations are refused in them. |
| `files-trash`, `files-hidden` | Show the trash folder in the tab, and show or hide dot files (also `Ctrl+H`). Dot files are hidden by default, as in every desktop file manager. A tab showing the trash draws each row's original path beside its name, read from the `.trashinfo` records. |
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
