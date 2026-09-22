# Desktop visual archaeology

Inspected 2026-09-17 for the realistic OS GUI follow-up. This is implementation evidence, not a claim of pixel-perfect OS emulation.

| Subsystem | Strongest inspected source | What should survive |
| --- | --- | --- |
| Integrated macOS / Windows / Ubuntu desktop | `synthetic-computer-environment` `88c5bed`, `apps/simulator/src/client/desktop/` (shell deepening commit `b68912f`) | Different OS chrome, floating window geometry, focus/MRU, minimize/maximize/restore, launchers, shell/application separation |
| macOS dock and menu detail | `JacobFV/macos-web-next` `870d09e`, `src/components/Dock/{Dock,DockItem}.svelte`, `src/components/TopBar/MenuBar.svelte` | Adaptive dock sizing, running indicators, contextual active-app menus, coherent icon silhouette; deterministic optional magnification |
| Windows desktop detail | `JacobFV/windows-web-next` `9482039`, `src/components/{Taskbar,StartMenu,WindowFrame,TaskView,AltTabSwitcher}.svelte` | Centered pinned plus running app list, separate tray, Start search/grid, titlebar controls, window switching |
| Generic workspaces | `JacobFV/browser-os` `36d4e90`, `packages/{workspace,taskbar,os}/src/` | Workspace/window organization, independent UI theme tokens; less authoritative for any particular OS appearance |
| Mobile shell | No mobile desktop implementation found in the originally referenced sources | Original iOS/Android-inspired shells need distinct status bars, home grids, full-screen applications and system navigation |

## Lineage and concrete paths

`synthux` `81f2f32` `.gitmodules` and `sim/simulators.json` register `sim/macos-web-next`, `sim/windows-web-next`, and `sim/browser-os`. `synthex` `6c8d624` lists the same repositories under `external/`. Thus the rich rendered OS references live in these submodules, not in the Python virtual-internet implementation. Synthux pins macOS at `58f35ec`; current upstream is `870d09e`. Windows and browser-os current heads match Synthux pins.

The user-provided `virtual-macos-golden-gate`, `virtual-windows-11`, `virtual-ubuntu-24`, `virtual-ios-18`, and `virtual-android-12` are useful profile names. Attempts to fetch those exact repository names under JacobFV returned repository-not-found; web search did not identify corresponding projects. Do not claim they were inspected as repositories. Actual accessible submodules above were inspected instead. The large macOS repository was additionally inspected through pinned GitHub tree/raw endpoints while its clone downloaded.

SCE `desktop/geometry.ts` centralizes reserved chrome: macOS menu 28 px and dock work-area reservation 82 px, Windows taskbar 48 px, GNOME top bar 32 px and left dash 68 px. It defines minimum window 380×240, half/quarter/maximize snap regions, a 14 px edge threshold, and clamping that keeps a titlebar reachable. `desktop/windowManager.ts` preserves restore geometry and implements stable stacking, MRU switching, minimizing and taskbar toggling. These semantics are stronger than decorative static titlebars.

SCE `desktop/MacChrome.tsx` has active-application menus, Spotlight, Launchpad, Mission Control, dock running indicators and popovers. `WindowsChrome.tsx` separates Start, taskbar, search, Task View and system flyouts. `GnomeChrome.tsx` separates Activities, app grid, clock/notifications and a vertical favourites/running dash. `AppWindow.tsx` supplies per-OS controls and pointer-driven geometry. `styles.css` contains original gradient wallpapers, platform-specific window radii/titlebar heights, inset borders and soft shadows. `appIcons.ts` chooses platform-specific icons; `vite.config.ts` combines selected Iconify collections with inline vector artwork. These paths are the best reference for reconstructing visible OS identity.

The newer macOS `DockItem.svelte` explicitly caps dock width to the viewport and budgets magnification: approximately 1.4× peak and 1.18× neighbour rather than uncontrolled 2× growth. `Dock.svelte` supports bottom/left/right placement and preferences. `MenuBar.svelte` switches active menus on hover only while a menu is already open. These are useful interaction details; its Svelte springs and host-window dimensions should become logical-time/viewport inputs if adopted in Rust.

Windows `Taskbar.svelte` derives the taskbar from pinned plus currently open applications, deduplicated. `StartMenu.svelte` supplies a search field, pinned grid, recommendations and alphabetical apps. Its recommendations are hardcoded and quick settings are explicitly visual-only; those should not be copied as purported world state. Likewise its `Date`/`setInterval` clock is not deterministic.

## Rendering recommendations

Keep OS shell state and application content in Rust. Render wallpaper, top bars, dock/taskbar, icons, floating window frames, content and overlays through the same scene contract, with matching hit regions. Browser canvas and native/pixel agents must see the same shell. Presentation JavaScript must not become a second window manager.

Use OS-specific visual grammar, not merely palette changes: macOS centered titles and left traffic-light controls; Windows right-side controls and centered bottom taskbar; Ubuntu a dark top bar and left launcher; iOS a rounded-square icon grid, compact status line and home indicator; Android a larger clock/widget composition, circular app icons, search pill and system navigation. Mobile layouts should open full-screen applications instead of shrinking desktop chrome. Keep launchers and window controls functional and replayable.

Do not reproduce invisible compatibility costs: React/Svelte stores, DOM layout measurements, CSS transitions, animation frames, iframe app execution, host time and browser-only menus are not canonical simulation semantics. Use integer geometry and simulation time. Preserve appearance with original deterministic scene primitives or locally packaged assets, avoiding external font/icon/image requests.

## Assets and licensing

`macos-web-next/LICENSE` is MIT, copyright 2021 Puru Vijay. `browser-os/LICENSE` is MIT, copyright 2024 browser-os contributors. If substantial implementation is reused, retain those notices. No root LICENSE was found in the inspected SCE or windows-web-next trees; do not assume the license of their upstream dependencies licenses their whole source.

The macOS tree contains a large `src/assets/wallpapers/` collection with named Apple-style and photographic files. Repository code licensing alone does not establish provenance for every bundled photograph, product icon, logo or OS wallpaper. For this implementation prefer original procedural wallpapers and original primitive icons. SCE Iconify sources span multiple collections with separate attribution terms; importing the whole set would also add unnecessary payload. No third-party bitmap asset or OS vendor font was copied during this investigation.
