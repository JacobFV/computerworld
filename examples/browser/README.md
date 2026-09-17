# Fully client-side browser demo

This page imports the canonical Rust runtime compiled to Wasm. The canvas receives
RGBA frames from that runtime; clicks and keys go back through actor actions.
JavaScript only manages controls and copies pixels. Each computer has its own
restricted actor session. The expandable event inspector uses a separate owner
handle intentionally; do not give that handle to an untrusted agent.

From the repository root:

```sh
./scripts/build-wasm.sh
node examples/browser/build.mjs
python3 -m http.server 8000
```

Open http://localhost:8000/examples/browser/. Static hosting serves the HTML, JS,
CSS and Wasm assets. After initialization, simulated episodes require no network
access and no backend. The world definition is embedded by `build.mjs` from
`worlds/company-2026/world.json` rather than fetched during simulation.

Select a computer, open a synthetic domain, use the terminal, or click the rendered
page. Save/restore snapshots, fork an independent world, and inspect the actor
observation and packet/event trajectory below the display. The world owner can
reset all computers and services together with the same seed.

## Verify actual browser execution

```sh
npm install --prefix /tmp/computerworld-browser-tools playwright
PLAYWRIGHT_MODULE=/tmp/computerworld-browser-tools/node_modules/playwright/index.mjs \
CHROME_BIN=/usr/bin/google-chrome node scripts/test-browser.mjs
```

Alternatively install Playwright in the repository and its Chromium browser, then
run `node scripts/test-browser.mjs`. The test denies all outbound requests and,
after bootstrap, **all** browser requests. It exercises terminal actions, multiple
computers, virtual website navigation, denied real URLs, snapshot/fork/replay,
deterministic Rust pixels, actor capability separation and network trajectories.
It writes `artifacts/browser-demo.png` for inspection.
