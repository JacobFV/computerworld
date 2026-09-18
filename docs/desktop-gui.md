# Native OS desktop presentation

OS shells and application views are Rust scene projections. The browser console
transfers RGBA frames, scales coordinates and forwards input. Native, Wasm and
Python consumers use the same state, scene contracts and renderer; programmatic
interaction does not require the console. See the [Python/JavaScript guide and runnable demos](programmatic-computer-use.md).

## Profiles

The browser example opts into these profiles:

| Profile ID | Presentation |
|---|---|
| `virtual-macos-golden-gate` | Coastal wallpaper, menu bar, traffic lights, centered dock, Finder-style file views |
| `virtual-windows-11` | Blue bloom wallpaper, caption controls, centered taskbar, Start/search, Explorer-style file views |
| `virtual-ubuntu-24` | Noble Numbat wallpaper, GNOME-style top bar, left dock, Activities and Nautilus-style file views |
| `virtual-ios-18` | Island/status area, widgets, rounded-square icons, dock, full-screen apps and app library |
| `virtual-android-12` | Material-style clock/search, circular icons, app drawer, full-screen apps and navigation controls |

Define ordinary `OsProfile` values with these IDs and suitable filesystem family,
home and shell fields. Existing generic profiles retain their original rendering.
Alternatively, map computer IDs to themes without changing the OS substrate:

```json
{"metadata":{"desktop_themes":{"workstation":"virtual-windows-11"}}}
```

A browser-only actor without `application.v1` receives a content-only browser
scene, including on graphical profiles. Desktop dimensions are caller-selected;
the console uses 960×640 desktops and 390×780 phones.

[Visual archaeology](../research/desktop-visuals.md) and the [overhaul reference review](../research/desktop-fidelity-references.md)
record predecessor paths and official visual references. Original generated
wallpapers and platform-inspired icon assets are bundled alongside the attributed
Ubuntu wallpaper and Yaru icons. See [asset provenance and licensing](../crates/render/assets/README.md).
No desktop asset requires a runtime network request.

## Window and application interaction

Desktop applications occupy stacked, clipped windows with preserved state. Pointer
down/move/up on a title bar moves a window; edge/corner regions resize it. Window
controls minimize, maximize/restore and close. Title-bar double-click toggles
maximize, and dragging to supported screen edges snaps a window. Focus and stacking
order determine which window receives input. Browser windows keep independent
navigation state. Window geometry, focus, panels and pointer capture are included
in session snapshots.

Nineteen window kinds are launchable. Four are built into `DesktopState::launch`:
`terminal` (output and input), `files`/`file_manager` (tabbed filesystem navigation
with click-to-select and double-click-to-open), `editor`/`text_editor` (saved from its menus),
and `browser`. The other fifteen are `NativeApp` kinds listed by the `native_apps!`
macro in `crates/applications/src/apps/mod.rs`: `calendar`, `mail`, `chat`, `docs`,
`notes`, `contacts`, `settings`, `calculator`, `clock`, `photos`, `music`, `maps`,
`weather`, `code`, `freecad` and `kicad` (plus the image editors, spreadsheets and
database client). Each is backed by a world service or the machine's own files
rather than a static mock. Browser applications obtain supported pages through simulated DNS,
networking and HTTP.

`metadata.desktop_apps` entries declare a world's application catalog and take a
`kind`. `kind: "native"` names one of the native applications and supplies the
service URL it should open against; it is launched in its own right, not as an
alias. An optional `urls` object overrides that URL per platform (`macos`, `windows`,
`ubuntu`, `ios`, `android`), because one kind of application is a different product on
each: the reference world's `music` entry opens spotify.com's catalogue everywhere but
Android, whose player is YouTube Music and reads `http://music.youtube.com/`. `kind: "browser"` is a genuine alias: it opens a browser window at a fixed
URL, and therefore requires both `application.v1` and `browser.v1` grants plus an
installed `browser`. Any other `kind` is rejected. Either way these remain ordinary
world services, not special kernel concepts.

### Home folders and file manager places

Creating a desktop session on a machine with a desktop theme is the user's first
login there. Like `xdg-user-dirs-update` on Ubuntu, a new Windows profile and a new
macOS account, it makes the platform's standard folders in the home folder, plus
`~/.local/share/Trash/files`, the folder deletions are moved to: Desktop, Documents,
Downloads, Music, Pictures, Public, Templates and Videos on Ubuntu; Desktop,
Documents, Downloads, Movies, Music, Pictures and Public on macOS; Desktop,
Documents, Downloads, Music, Pictures and Videos on Windows. Only missing folders are
made, so a second login changes nothing, snapshots carry them, and `reset` logs in
again. A terminal-only or browser-only session, and a phone, log nobody in.

Each file manager's sidebar holds the platform's standard places, each one a real
command, and only those whose folder exists right now:

| Shell | Sidebar |
|---|---|
| Files (Ubuntu) | Recent, Starred, Home, Desktop, Documents, Downloads, Music, Pictures, Videos, Trash, then Other Locations (the computer's root) |
| Finder (macOS) | Favorites: Recents, Desktop, Documents, Downloads; Locations: Macintosh HD. The Go menu adds Back, Forward, Enclosing Folder, Home and Computer |
| File Explorer (Windows) | Home (pinned folders, favourites and recent files), Gallery (images in Pictures), the six pinned folders, This PC |

Recent is the documents really opened from a file manager; Starred is the set a row's
star adds to (`DesktopState::starred`). AirDrop, iCloud, Applications, Tags,
OneDrive and Network have nothing behind them in the simulator and are omitted
rather than painted. Dot files are hidden until `Ctrl+H`.

Terminal windows print the prompt the machine's default shell prints, and title
themselves from it: bash's `user@host:~/dir$` (GNOME Terminal's title is
`user@host: ~/dir`), zsh's `user@host dir %` on a Mac (Terminal's title is
`user — -zsh`), and PowerShell's `PS C:\path>` (Windows Terminal's tab reads
"Windows PowerShell").

### KiCad

`kicad` is KiCad 8, installed on the reference world's macOS, Windows and Ubuntu
desktops (not the phones). Its engine is `crates/eda`, a pure crate: schematic
capture with connectivity, ERC, KiCad and SPICE netlists and a BOM; a SPICE-class
simulator (modified nodal analysis, dense LU with partial pivoting, Newton–Raphson with
junction limiting, gmin and source stepping; diode, BJT, level-1 MOSFET, controlled
sources; DC operating point and sweep, AC, trapezoidal/backward-Euler transient) whose
arithmetic avoids platform `libm` so waveforms are bit-identical everywhere; and board
layout with update from schematic, ratsnest, 45° routing, raster zone fill with thermal
reliefs, DRC, Gerber RS-274X/Excellon output and SVG plots.

- Each frame is its own window — project manager, Schematic Editor, PCB Editor,
  Simulator — over one open project; after every action the desktop copies the frame
  that changed to the others (`NativeApp::share_from`), so a schematic edit is what the
  board editor's Update PCB and the simulator see.
- Projects live in `~/Documents/KiCad/<name>/` as `<name>.kicad_pro` (JSON, with the
  design rules), `.kicad_sch` and `.kicad_pcb` (KiCad 8 S-expressions, file versions
  20231120 and 20240108) and are read with `ReadFiles`, so a missing schematic or board
  is a new one rather than an error. Exports go next to them (`.net`, `.cir`, `.csv`,
  `gerbers/`).
- The canvases are drag surfaces whose targets carry the view they were painted with,
  and they follow the pointer with no button down (`NativeApp::hovers`), so the wire,
  track or symbol being placed is drawn under it.

### Music

`music` is one player with five faces over the `media` service the world backs it with
(spotify.com, or music.youtube.com on Android): Apple Music on macOS and iOS, YouTube
Music on Android, Media Player on Windows 11 and Rhythmbox on Ubuntu. Its controls are
listed in [action-families.md](action-families.md#music-player-controls).

- **Artwork is real artwork.** Every album, artist and playlist has a cover generated
  from its id by `crates/artwork`: a palette and a composition (a sunset, a Bauhaus
  grid, waves, orbits, shards, stripes, a halftone) laid out in a 1000-unit square. The
  players draw it with the renderer's own shapes at any size, round for an artist's
  avatar; the sites serve the same composition rasterised from `GET /art/<key>` (see
  [service-sdk.md](service-sdk.md)), so an album looks the same everywhere.
- **Volume is state.** The service keeps a volume and a mute with the listener's
  session, which every player's slider sets and the machine's own output volume then
  scales — Music's toolbar slider, Media Player's and Rhythmbox's volume buttons. On the
  phones, where the real players use the device's volume, the slider is the machine's
  (`shell:set:volume:<pct>`) until the music is playing on a speaker, when it is the
  session's again.
- **Lyrics are time-synced.** Catalogue songs carry `[start_ms, line]` lyrics; the line
  being sung is the last one that has started at the world clock's position, lit in
  Apple Music's lyrics panel, YouTube Music's LYRICS tab, and spotify.com's lyrics view.
  Tapping a line seeks to it. A song with none says so, and YouTube Music greys the tab
  out, as it does.
- **Casting is real.** The account's speakers are `speaker` services on the network
  (`crates/../services/speaker`: Living Room, Kitchen, Office TV in the reference
  world). Opening AirPlay, Cast or Cast to device asks each one whether it is there; one
  that does not answer is shown disabled with why. Picking one hands it the session —
  the queue, the position, the repeat mode and the volume — which it carries forward on
  the same clock with the same calculation, and every later change is sent on. Its own
  page says what it is playing.
- **Shelves scroll sideways.** Home's and Explore's shelves are horizontal panes: the
  wheel's `delta_x` (or Shift with the wheel), the bar along their bottom edge on a
  desktop, and a sideways swipe on a phone.

### Visual Studio Code

`code` is installed on the reference world's macOS, Windows and Ubuntu desktops. It
draws its own 35 px title bar on all three — traffic lights and the command center on
macOS; the menu bar, command center and window buttons on Windows and Ubuntu — in the
Dark Modern theme by default (Light Modern from Settings or `Ctrl+K Ctrl+T`). Launched on
nothing it opens `~/project` when the machine has one and the Welcome page otherwise;
`shell:launch:code/<folder>` opens a folder. What it shows and does is the machine's:

- The Explorer is a listing of the folder (`AppEffect::ListTree`, which walks the VFS
  under the user's own read permissions); New File, New Folder, Rename and Delete are
  `CreateFile`, `CreateDirectory`, `MovePath` and `TrashPath` through the kernel.
- An editor holds a file's text (`AppEffect::ReadFiles`) and Save writes it back
  (`WriteFile`); the tab turns clean only when the write is reported done. CRLF files
  are saved with CRLF. Undo is a history of reversible edits, bounded at 200.
- Highlighting is a tokenizer per language (Python, JavaScript/TypeScript, Rust, JSON,
  Markdown with fenced languages, HTML with embedded CSS and JavaScript, CSS, shell)
  with Dark+/Light+ token colours and bracket pair colourisation.
- Search reads the workspace's files (up to 512 files and 8 MiB per search) and matches
  literally or by regular expression, with case and whole-word options.
- The integrated terminal is a session of the machine's shell (`AppEffect::ShellRun`,
  `Runtime::execute_in`): the same commands, exit codes and prompt as the Terminal, but
  with its own working directory, so `cd` in it does not move the machine's shell. Run
  (`F5`, the editor's Run button, `python.execInTerminal`) saves the file and types
  `python3 <file>` (`python` on Windows), `node <file>` or `bash <file>` into it. A
  missing interpreter shows the shell's own `command not found` and exit 127.
- Problems are what tools reported: CPython and Node tracebacks and bash line errors
  from runs, and the JSON parser's error for an open JSON file. Each opens its line.
- Source Control runs `git status`, `git add`, `git commit`, `git init`, `git branch`,
  `git checkout`, `git reset` (Unstage) and `git restore`/`git checkout --` (Discard
  Changes; a new file is trashed instead) in the workspace and logs them in the Output
  panel.
- The editor area splits right and down into up to four groups (`Ctrl+\`, View: Split
  Editor Right/Down, Move Editor into Next/Previous Group, Focus *n*th Editor Group).
  A file open in two groups is one document: an edit in one appears in the other, and
  one undo takes it back in both.
- The minimap is a scaled map of the file's lines drawn from the same tokens as the
  text; pressing or dragging it scrolls the editor to that part of the file.
- Multiple cursors: Alt+click adds one, `Ctrl+Alt+Up`/`Down` add one above or below,
  `Ctrl+D` adds the next occurrence of the selection, `Ctrl+Shift+L` all of them, and
  `Escape` drops the extras. Typing, pasting, Backspace, Delete and the arrows act at
  every cursor, and each keystroke is one undo step.
- Tabs in the text are tab stops (`editor.tabSize` columns), including where a click
  lands; `editor.renderWhitespace` (View: Toggle Render Whitespace) draws spaces as
  middle dots and tabs as arrows.
- A right press opens the context menu for what is under it: the Explorer's (New File,
  New Folder, Rename, Delete, Copy Path, Reveal in the file manager, Open in Integrated
  Terminal) or the editor's (Cut, Copy, Paste, Go to Definition, Command Palette). Go to
  Definition reads the workspace and opens where the name under the caret is defined
  (`def`/`class`, `function`, `const`/`let`/`var`, `fn`, `struct`, `enum`, `trait` and
  `impl`), and says so when nothing defines it.
- Settings (`workbench.colorTheme`, `editor.fontSize`, `editor.tabSize`,
  `editor.wordWrap`) persist to VS Code's own `settings.json` for the platform
  (`~/.config/Code/User`, `~/Library/Application Support/Code/User`,
  `~/AppData/Roaming/Code/User`) and are read back at launch.

Not implemented, and so not drawn: extensions, the debugger (Run executes without
one), the Accounts menu, and italic preview tabs (the renderer has no italic face yet,
so a preview tab is dimmed instead).

Launchers, search, task switching and platform panels expose semantic hit regions.
Mobile profiles implement Home, recent apps and supported vertical swipes for
launcher/control panels. Mobile apps are full-screen, not movable desktop windows.
Installed-app and capability checks apply to visible launcher actions and direct
API calls alike. Unsupported decorative app controls are marked disabled.

`application.v1` supports `home`, `launcher`, `minimize`, `maximize`, `switcher`,
`focus`, `close`, `launch` and `event` (for registered SDK applications). Pointer operations include `click`, `down`, `move`,
`up`, `cancel` and `double_click`; always supply the matching viewport dimensions.
Keyboard type/key events target the focused control. The console's **Expand desktop**
enlarges the selected monitor; controls below it are host visualization tools.

Window interactions use `window:<id>:drag`, `window:<id>:resize:<direction>`,
`window:<id>:maximize`, and related names. Child content is namespaced under
`window:<id>:content:`. Nodes retain local bounds, transforms and scene-space clips;
clients must account for these and occlusion when choosing pointer coordinates.
The [programmatic guide](programmatic-computer-use.md#pointer-coordinates-windows-and-gestures)
explains exact transforms and event sequences.

### FreeCAD

`freecad` is installed on the reference world's macOS, Windows and Ubuntu desktops (not
on the phones). It is FreeCAD 1.0's Part Design workbench and Sketcher in the "FreeCAD
Light" theme — its menu bar (FreeCAD's File, Edit, View and Help in the Mac menu bar on
macOS), toolbars that switch to the Sketcher's while a sketch is open, the Combo View
with the model tree, property editor and task panels, the 3D view with navigation cube
and axis cross, the report view, and a status bar with preselection, navigation style
and view size. The model is real, in the `cw-cad` kernel:

- Sketches are FreeCAD's `SketchObject` model (GeoId, PointPos, constraints with
  First/Second/Third and a value) solved by Levenberg–Marquardt over exact dual-number
  Jacobians. The task panel reports FreeCAD's solver messages — "Under constrained: n
  DoFs", "Fully constrained", "Over-constrained: (…)", "Redundant constraints: (…)" —
  from the Jacobian's rank and row dependencies, and fully constrained geometry turns
  green. Tools add FreeCAD's automatic constraints (coincident on a snapped point,
  horizontal/vertical when drawn so); a drag on geometry moves it through the solver.
- Pad, Pocket (length, two lengths, through all, symmetric, reversed), Revolution,
  Groove, Hole (counterbore, countersink, drill point), Fillet and Chamfer (straight
  edges between planes, circular edges between a face and a coaxial cylinder),
  Mirrored, Linear and Polar Pattern are recomputed in order from their parameters;
  sketches attach to base planes or to planar faces, and a face or edge reference keeps
  FreeCAD's element name plus where it was, so it is found again after an upstream edit
  renumbers the shape. A feature that fails is marked in the tree and reported, as
  "Result has multiple solids" and the like.
- Solids are closed triangle meshes whose triangles remember the analytic surface they
  approximate (curves tessellated at 64 segments per turn). Booleans are BSP-tree CSG,
  healed to watertight meshes (weld, T-junction insertion, ear clipping, flat-vertex
  collapse), so volumes, areas and centres of mass are those of the polygonal solid.
- The 3D view is a z-buffer raster (2× supersampled, headlight shading, depth-tested
  edges and vertices) delivered as an `Image` primitive; sketches in edit are drawn as
  vector paths over it. Picking uses the same projection as drawing.
- Files: the native document is FCStd's `Document.xml` structure as JSON
  (`*.FCStd.json`); STL (binary and ASCII) and OBJ import as mesh objects and export
  from any shape; DXF (R12) imports into and exports from sketches; SVG exports a
  hidden-line projection of the view at 1:1 in millimetres. Binary files move through
  `AppEffect::ReadBytes` and `WriteBytes`.
- Undo and Redo cover every document change (30 levels); snapshots hold the document
  and view, and the model is recomputed from it on restore.

## Rendering and fidelity

Compact scenes separate state, layout, primitives and rasterization. Structured
clients can request semantics without rasterizing. Rounded rectangles, soft shadows,
proportional regular/bold UI text and bundled asset images provide the shell visuals.
Original monospace `Text` remains available. Immutable decoded asset backing is
shared within a runtime process, with caches for glyphs, scaled resources and shadow
masks. No host fonts, DOM layout or wall-clock animation participates in rendering.

These are interactive synthetic OS presentations, not actual vendor operating
systems or pixel-perfect replicas. Bundled fonts differ from vendor system fonts;
system panels and native apps implement deliberately bounded behavior. Native mobile
SDKs, phone/camera hardware, arbitrary third-party binaries and arbitrary website
HTML/JavaScript are not implemented. Rendering detail does not imply those features.

## Verification

- `scripts/test-desktop-overhaul.mjs`: actual Chromium pointer drag/resize, stacking,
  window controls, mobile gestures, service launchers and screenshots.
- `scripts/test-desktops.mjs`: profile interaction, transformed browser clicks,
  keyboard input, snapshots and offline operation.
- `crates/computerworld/tests/desktop.rs` and `desktop_extensions.rs`: shell behavior,
  actor/install isolation and configured service apps.
- `crates/render/tests/wasm-gui.cjs`: native/Wasm raster parity.
- `scripts/smoke-desktop-pixels.cjs` plus `examples/python/desktop_pixels.py`: portable
  Node/Python desktop checkpoints and RGBA comparisons.
- [Programmatic interaction demos](programmatic-computer-use.md): reusable external
  Python/JavaScript usage, snapshot branches and cross-binding comparisons.

Verification scripts are reproducible checks; current execution results and timings
belong in generated artifacts/final reports, not implied by the existence of a test.
Per-frame browser smoke timings do not replace the pinned benchmark methodology.

Every family, op, payload shape and interaction target — including the shell panel,
toggle, power and tab targets this page describes in prose — is tabulated in
[action families](action-families.md).
