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

Thirteen window kinds are launchable. Four are built into `DesktopState::launch`:
`terminal` (output and input), `files`/`file_manager` (tabbed filesystem navigation
with click-to-select and double-click-to-open), `editor`/`text_editor` (with Save),
and `browser`. The other nine are `NativeApp` kinds listed by the `native_apps!`
macro in `crates/applications/src/apps/mod.rs`: `calendar`, `mail`, `chat`, `docs`,
`notes`, `contacts`, `settings`, `calculator` and `clock`. Each is backed by a world
service rather than a static mock, so its contents come through the simulated
network. Browser applications obtain supported pages through simulated DNS,
networking and HTTP.

`metadata.desktop_apps` entries declare a world's application catalog and take a
`kind`. `kind: "native"` names one of the nine native applications and supplies the
service URL it should open against; it is launched in its own right, not as an
alias. `kind: "browser"` is a genuine alias: it opens a browser window at a fixed
URL, and therefore requires both `application.v1` and `browser.v1` grants plus an
installed `browser`. Any other `kind` is rejected. Either way these remain ordinary
world services, not special kernel concepts.

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
