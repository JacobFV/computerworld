# World console

A fully client-side network console over the canonical Rust/Wasm runtime.

```sh
scripts/build-wasm.sh
node examples/browser/build.mjs
python3 -m http.server 8000
```

Open http://localhost:8000/examples/browser/.

The overview shows seven independently simulated devices: macOS and Windows desktops, an Ubuntu laptop, two headless servers, an iOS-style phone and an Android-style phone. Every service the world declares is reachable through the synthetic network — the company's own `.internal` intranet, mail, documents, chat, calendar and issues, plus a synthetic public web on recognisable domains (search, webmail, video, social, encyclopedia, shopping, an AI assistant and more). Addresses are all in IANA documentation ranges, so no simulated host resembles a real one. Links in the diagram come from the actual topology. Click a device to control it; its monitor, keyboard/text input, pointer/touch input, terminal and applications share the same Rust state. Server previews show console output rather than pretending to have physical monitors.

Mail, Calendar, Messages, Documents, Contacts, Notes, Settings, Calculator and Clock are native applications drawn by Rust, not browser shortcuts: they talk to their services over the same simulated network and gateway the browser uses. A single click selects; a double click opens — on a phone, one tap opens, because a touch screen has no second click.

**Add device** creates a real computer, filesystem, process namespace and network identity through `World.addComputer`. Choose an existing node to link it to, or leave it isolated. **Remove** preserves the remaining world. Service-hosting machines cannot be removed until their hosted services are moved; this demo does not provide service migration controls. Device shape/peripheral labels are visualization metadata, not hardware emulation. Phone screens run supported synthetic applications at 390×780. Their status bars, home screens and navigation are OS-style Rust scenes; they do not execute Android/iOS native apps. The three desktop profiles also have distinct Rust-rendered chrome, launchers and window controls. See [desktop presentation](../../docs/desktop-gui.md).

Save/restore/fork includes the live topology and actor sessions. Reset returns to the embedded blueprint and original seed. Everything after static boot works with browser networking blocked. Static boot includes the everyday files of the font pack (`pkg/web/fonts/`: regular Simplified Chinese and Korean, monochrome and colour emoji, 6.4 MB gzipped), fetched in parallel with the first paint and handed to `installFont`; the bold, Traditional Chinese/Japanese/Korean-form and extra-script files (6.5 MB) follow right behind and repaint when they land. Until a file arrives its glyphs draw as boxes at their final positions (emoji draw monochrome until the colour file is in). There is no simulation server and no ambient outbound networking. State is local to the current tab; reload starts a new episode.

The owner-only trace/observation inspector and `window.computerworldDemo` development handle are intentionally privileged. Agents should receive the restricted environment handle, never this owner console.

Verification:

```sh
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CHROME_BIN=/path/to/chrome node scripts/test-browser.mjs
```

The browser test checks real Wasm execution, seven machines, the declared sites, keyboard and pointer input, raster determinism, denied outbound access, dynamic phone/headless creation, isolated networking, service-host removal protection, topology snapshot/restore/fork/reset, and zero browser requests during the episode.

Use the on-screen dock/taskbar/launcher to open apps, or use the auxiliary device controls below the screen. **Expand desktop** increases the selected display. Additional five-OS browser QA: `node scripts/test-desktops.mjs` with the same Playwright/Chrome environment variables.

Desktop title bars support dragging and double-click maximize/restore. Window edges and corners resize; dragging to desktop edges snaps windows. Dock/taskbar items restore running applications. Phones support Home, recent-app, app-drawer and shade gestures. All input is interpreted by Rust.

For agents and other applications, see [programmatic computer interaction](../../docs/programmatic-computer-use.md), the [JavaScript demo](../javascript/computer-interaction.mjs), and the [Python demo](../python/computer_interaction.py). These use the same canonical runtime without the console UI.

Run `node scripts/test-desktop-overhaul.mjs` for the gesture/window regression suite. OS shells are synthetic approximations, not native OS executables or pixel-perfect reproductions.
