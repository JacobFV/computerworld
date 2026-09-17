# Native OS desktop presentation

The OS shell and application views are Rust scene projections. The browser console
only transfers RGBA frames, scales coordinates and forwards input. Native, Wasm
and Python users see the same pixels, hit regions and snapshot state.

## Profiles

The browser example includes these explicitly opted-in profiles:

| Profile ID | Presentation |
|---|---|
| `virtual-macos-golden-gate` | Menu bar, coastal wallpaper, traffic-light window controls, centered dock |
| `virtual-windows-11` | Blue wallpaper, right-side window controls, centered taskbar and Start launcher |
| `virtual-ubuntu-24` | Aubergine wallpaper, dark top bar, left launcher and Activities grid |
| `virtual-ios-18` | Status bar/island, widget, rounded-square app icons, bottom dock and Home indicator |
| `virtual-android-12` | Material-style clock/search layout, circular accents, app launcher and system navigation |

A world author defines ordinary `OsProfile` values with those IDs and chooses
appropriate filesystem family/home/shell fields. Existing generic profiles keep
their original rendering. Alternatively, `world.metadata.desktop_themes` maps
computer IDs to theme names. A browser-only actor without `application.v1` retains
a content-only browser scene, even on a graphical profile.

```json
{"metadata":{"desktop_themes":{"workstation":"virtual-windows-11"}}}
```

The original three desktop implementations and SynthUX's desktop submodules were
inspected again before this change. [Visual archaeology](../research/desktop-visuals.md)
records exact paths and revisions. We recovered OS-specific chrome proportions,
launcher/work-area hierarchy and focus/minimize/restore behavior. No third-party
wallpaper or icon bundle was copied. Wallpaper and icons are original deterministic
scene geometry; mobile shells are new implementations, not recovered predecessor code.

## Interaction

The shell exposes semantic hit regions for app launch, launcher, close, minimize,
maximize, Home, task switching and browser back/forward/reload/address entry.
Applications include native terminal output/input, filesystem navigation, an editor
with cursor-follow scrolling and Save, and a browser whose content comes from the
synthetic network. The console's **Expand desktop** enlarges the selected device;
auxiliary tools remain below the screen.

`application.v1` accepts `home`, `launcher`, `minimize`, `maximize`, `switcher`,
`focus`, `close` and `launch`. `pointer.v1/click` hits the same shell controls in
pixel or structured scenes. `keyboard.v1` supplies text/keys; Alt+Tab cycles apps,
Escape returns Home, and the focused native address field accepts URL text and
Enter. These transitions are serialized with actor session state. Installed-app
and capability checks apply to launcher clicks as well as direct actions.

Browser content nodes retain their semantic IDs but have scene transforms and
clips describing their window placement. Structured clients must apply each node's
transform when turning local bounds into pixel coordinates. They must not assume
that an application occupies the whole display.

## Rendering and fidelity

`RoundedBox` provides deterministic antialiased corners/borders and matching rounded
hit testing. `UiText` uses bundled DejaVu Sans with proportional advances; `Text`
retains the original monospace rendering and golden hash. No host fonts, DOM layout,
wall-clock time or external asset requests participate in simulation rendering.
UI glyphs/masks are cached; the console updates the selected monitor preview from
the same rendered frame instead of rasterizing every idle device after every key.

These are recognizable, interactive synthetic OS shells, not pixel-perfect vendor
replicas or real operating systems. Current application views are deliberately
small; synthetic websites still use the native structured-page layout. There is
one foreground application view per device, with switching to preserved background
app state. Arbitrary window drag/resize, overlapping live app surfaces, Control
Center/settings panels, native mobile SDKs, camera/phone functionality and third-party
native applications are not implemented.

## Verification

- `scripts/test-desktops.mjs`: real Chromium, all five profiles, native address entry,
  transformed webpage clicks, terminal keys, min/max/restore/close/Home, snapshots,
  deterministic distinct frames and zero network requests after bootstrap.
- `crates/computerworld/tests/desktop.rs`: native shell integration and actor/install
  isolation.
- `crates/render/tests/wasm-gui.cjs`: native/Wasm golden and incremental raster parity.
- Node/Python portable desktop checkpoint and 960×640 RGBA equality verified.

Screenshots and reproducible results live in `artifacts/desktop-*.png` and
`artifacts/desktop-verification.json`. Per-frame timings there are browser smoke
measurements, not replacements for the pinned benchmark methodology.
