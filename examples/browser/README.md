# World console

A fully client-side network console over the canonical Rust/Wasm runtime.

```sh
scripts/build-wasm.sh
node examples/browser/build.mjs
python3 -m http.server 8000
```

Open http://localhost:8000/examples/browser/.

The overview shows six independently simulated devices: macOS and Windows desktops, an Ubuntu laptop, two headless servers and a phone-sized Ubuntu device. Eight services are reachable through the synthetic network. Links in the diagram come from the actual topology. Click a device to control it; its monitor, keyboard/text input, pointer/touch input, terminal and applications share the same Rust state. Server previews show console output rather than pretending to have physical monitors.

**Add device** creates a real computer, filesystem, process namespace and network identity through `World.addComputer`. Choose an existing node to link it to, or leave it isolated. **Remove** preserves the remaining world. Service-hosting machines cannot be removed until their hosted services are moved; this demo does not provide service migration controls. Device shape/peripheral labels are visualization metadata, not hardware emulation. Phone screens run supported synthetic applications at390×720; they do not emulate Android/iOS.

Save/restore/fork includes the live topology and actor sessions. Reset returns to the embedded blueprint and original seed. Everything after static boot works with browser networking blocked. There is no simulation server and no ambient outbound networking. State is local to the current tab; reload starts a new episode.

The owner-only trace/observation inspector and `window.computerworldDemo` development handle are intentionally privileged. Agents should receive the restricted environment handle, never this owner console.

Verification:

```sh
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CHROME_BIN=/path/to/chrome node scripts/test-browser.mjs
```

The browser test checks real Wasm execution, six machines, eight sites, keyboard and pointer input, raster determinism, denied outbound access, dynamic phone/headless creation, isolated networking, service-host removal protection, topology snapshot/restore/fork/reset, and zero browser requests during the episode.
