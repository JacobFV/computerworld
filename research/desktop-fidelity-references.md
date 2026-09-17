# Desktop fidelity reference set

Inspected 2026-09-17. This adds concrete visual targets to [desktop archaeology](desktop-visuals.md). References are design evidence, not bundled product assets and not proof that the simulator matches them. Measurements below are logical-pixel implementation targets inferred from screenshots unless marked as source-code constants; display scale and user preferences change real OS geometry.

## Actual OS reference images

Downloaded originals are available for local screenshot review in `/tmp/computerworld-reference-images/`; `manifest.json` records URLs. These vendor reference images are **not redistributed in the repository or runtime**.

| Theme | Official reference and local image | Visual evidence |
| --- | --- | --- |
| macOS | [Apple Sequoia release, window tiling](https://www.apple.com/newsroom/2024/09/macos-sequoia-is-available-today/), `macos.jpg` | Dense proportional menu text, unified toolbar, inactive gray traffic lights, narrow sidebar, independently tiled windows, compact translucent dock |
| Windows 11 | [Microsoft taskbar customization](https://support.microsoft.com/en-us/windows/experience/personalization/customize-the-taskbar-in-windows), `windows.png` | Centered app icons, separate search pill, pale translucent Start surface, recommended rows, subtle separators and elevation |
| Ubuntu 24.04 | [Canonical installation guide](https://ubuntu.com/desktop/docs/en/24.04/tutorial/install-ubuntu-desktop/), `ubuntu.png` | GNOME 46 top-left workspace pill/dot, centered date, narrow left launcher, burgundy Noble wallpaper and symbolic status icons |
| iOS 18 | [Apple iOS 18 release](https://www.apple.com/uk/newsroom/2024/09/ios-18-is-available-today-making-iphone-more-personal-and-capable-than-ever/), `ios.jpg` | Four-column continuous-corner icons, large top safe area, Dynamic Island, compact status glyphs, configurable icon tint/gaps and bottom customization sheet |
| Android 12 | [Google Android 12 launch](https://blog.google/products-and-platforms/platforms/android/android-12/), `android.jpg` | Wallpaper-derived warm tonal palette, rounded/scalloped widgets, circular icons, At a Glance, search pill, centered camera and gesture handle |

Direct image URLs:

- [macOS tiling](https://www.apple.com/newsroom/images/2024/09/macos-sequoia-is-available-today/article/Apple-macOS-Sequoia-window-tiling_big.jpg.large_2x.jpg)
- [Windows Start](https://support.microsoft.com/en-us/windows/media/start-on-the-taskbar.png)
- [Ubuntu desktop](https://ubuntu.com/desktop/docs/en/24.04/_images/ubuntu-24-04-desktop.jpeg)
- [iOS Home customization](https://www.apple.com/newsroom/images/2024/09/ios-18-is-available-today-making-iphone-more-personal-and-capable-than-ever/article/Apple-iOS-18-Home-Screen-widgets-customize_inline.jpg.large_2x.jpg)
- [Android widgets](https://storage.googleapis.com/gweb-uniblog-publish-prod/images/Widget_Overview_Raven.width-1000.format-webp.webp)

## Predecessor implementations inspected again

`macos-web-next` commit `870d09e66816b12538feb83c41f583797d17c376`: `src/components/Desktop/Window/{Window,TrafficLights}.svelte`, `src/components/Dock/{Dock,DockItem}.svelte`, `src/components/TopBar/MenuBar.svelte`, `src/css/theme.css`, `src/configs/wallpapers/wallpaper.config.ts`, `src/assets/wallpapers/`, `public/app-icons/`. The code uses 12px window radii, 0.8rem traffic lights with 0.6rem gaps, unfocused gray controls, traffic colors `#ff5f56/#ffbd2e/#27c93f`, translucent dock plate with fine inner border, and proportional `-apple-system/.../Inter` typography. Its wallpaper catalog and multi-resolution icons make a substantial visual difference versus a few geometric placeholders. Its icon provenance is not automatically established by its MIT code license.

`windows-web-next` commit `948203945a7dae4f54c65cf098fe3f5c9f34750b`: `src/components/{WindowFrame,Taskbar,StartMenu,AltTabSwitcher,TaskView,AppIcon}.svelte`, `src/components/apps/FileExplorer.svelte`, `src/css/global.css`. Source constants: 48px taskbar, 32px window titlebar, 46px title-control widths; corner radii 4/8/12px; `#F3F3F3` main surface, `#0078D4` accent, 12px title text, 14px base text; layered shadow 2px/4px plus 8px/32px. Close-hover color `#C42B1C`; fine 6%-black outlines. Segoe UI Variable is requested from the host, which must be replaced by a bundled licensed font for deterministic cross-host pixels. Start recommendations are hardcoded examples and some quick settings are explicitly visual-only: carry the appearance, not false claims of working state.

`synthetic-computer-environment` commit `88c5bed`: `apps/simulator/src/client/desktop/{geometry.ts,windowManager.ts,MacChrome.tsx,WindowsChrome.tsx,GnomeChrome.tsx,AppWindow.tsx,styles.css,appIcons.ts}` remains the strongest integrated pointer/window state reference. Its named OS visual profiles are stronger than the Python virtual internet layers. The actual accessible submodules were discovered through Synthux/Synthex manifests; no evidence established repositories bearing the exact requested `virtual-*` names.

## Concrete rendering targets

| Theme | Geometry and typography | Material and artwork | Interaction details to verify |
| --- | --- | --- | --- |
| macOS | 24–28px menu bar; 13px regular menus with active app semibold; 12px traffic lights at ~20px pitch; 11–12px window corner; 44–52px dock icons; 12–13px sidebar rows | Wallpaper visible through menu/dock; restrained gray toolbar; fine highlight plus soft broad shadow; rich squircle icons; no monospace system labels | Drag titlebar, resize edges, focus changes controls, close/minimize/restore, menu dismiss/outside click, dock running dots, usable inactive windows |
| Windows 11 | 48px taskbar; 24px icons inside 40px targets; 32px titlebar; 46px controls; 8px window radius; 12px title, 14px body; Start six-column icon grid | Pale Mica-like surface, 1px subtle outline, white content panels, blue selected accents, colored silhouette icons rather than identical colored tiles | Drag/resize, edge snap preview, double-click maximize, restore geometry, taskbar toggle, Start search, hover feedback, Alt-Tab ordering |
| Ubuntu 24 | Official 1920×984 screenshot has 28px topbar and 48px dash; 32px app icons at ~48px vertical pitch; proportional 13–14px shell text | Topbar ~`#131313`, dash ~`#251921`, burgundy/orange wallpaper; warm-gray header, orange accents; symbolic monochrome toolbar glyphs | Workspace pill/overview, left running indicators, applications grid, focused app header, launcher switching, right-side window controls |
| iOS 18 | 390-ish pt phone canvas; four 60pt icons per row; 12pt labels; ~54pt status safe area; 17pt semibold time; 34pt bottom safe area | Continuous rounded-square icons, fullbleed textured wallpaper, translucent rounded dock, white status glyphs, sheet layers with generous corners | Touch launch, Home gesture/button, app switcher, full-screen app instead of tiny desktop window, back affordance, readable keyboard and scrollable content |
| Android 12 | 393-ish dp canvas; ~24dp status bar; 48dp circular icons; 12sp labels; 48–56dp search; 24dp gesture area; large widget display numbers | Material You tonal surfaces from wallpaper palette; rounded rectangles and scalloped clock; white/tonal bottom search; circular icon masks without iOS dock slab | Home/back/recents, app drawer, notification shade, full-screen apps, keyboard, scrollable content; no desktop traffic-light controls |

These measurements are a starting calibration, not an unsupported claim of exact OS metrics. Golden Gate is the simulator's macOS profile name; Sequoia is the official version used for contemporary screenshot comparison. Ubuntu should specifically reflect GNOME 46, not a later GNOME or generic Linux desktop. Android targets Pixel-style Android 12, not every manufacturer's skin.

## Asset and font policy

Do not equate a repository's MIT code license with rights to every photograph, OS wallpaper, vendor font, or app logo in it. `windows-web-next` has no root license in the inspected tree. Vendor press screenshots here are references only. Prefer original wallpaper artwork and original rendered icon glyphs, or individually licensed packaged assets with notices. Bundle open fonts (for example Inter/Noto/Roboto/Ubuntu when the relevant license is included); do not depend on host SF/Segoe availability. Deterministic antialiased proportional typography is necessary: painting all UI with terminal monospace makes even correct geometry look synthetic.

## Visual acceptance method

Compare each simulator screenshot side-by-side with its actual OS reference at matched logical viewport, then inspect: text size and weight; icon silhouettes; chrome density; surface hierarchy; spacing; shadow softness; wallpaper detail; toolbar/sidebar composition. A pixel hash proves determinism, **not** fidelity. Do not report pixel similarity against a different app/window arrangement as meaningful. Capture idle desktop, launcher, two overlapping windows, focused/unfocused frame, dragged/resized geometry, and a real app view; phones additionally need home, open app, keyboard and navigation. Keep interaction tests alongside visual review so realistic controls have real behavior.
