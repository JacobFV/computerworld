# Investigation verification

This file records predecessor checks, not results for a new Rust simulator.
Date: 2026-09-17. Node v24.21.0, Python 3.12.3, ARM64 Linux. Concurrent archaeology
was active; durations below are test-run diagnostics, not performance benchmarks.
Pinned commits: [sources.json](../sources.json).

| Source / check | Result | Scope |
|---|---|---|
| SCE complete Vitest source suite | 372 passed, 10 files, 7.38 s runner duration | All discovered package + integration + fidelity tests |
| TCN vendored engine `npm test` | 372 passed, 10 files, 6.45 s | Same core regression coverage with vendored adaptations |
| TCN bridge vs persistent session | 3/3 whole decoded payload comparisons equal | Write, appended read, changed-seed reset; see tcn.md |
| SynthUX internet tests | 6 passed | Python API/store tests, not rendered website fidelity |
| SynthUX v2 tests | 89 passed | Stores, logical scheduling, stub viewports |
| Synthex targeted source probes | EventBus index defect and UUID snapshot instability reproduced | See lineage-boundaries.md |
| Standalone service probes | GitHub mutations/namespace separation, mail Sent-only, Slack/Docs ignored writes verified | See services.md; no upstream test suites exist |

The SCE full workspace `pnpm install --frozen-lockfile` timed out downloading
`@sparticuz/chromium`. This did not block the kernel test run: the compatible
Vitest 3.2.7 already installed for TCN ran the unmodified SCE source/tests with
temporary resolver aliases for SCE workspace packages. No predecessor source was
edited to pass tests. Configuration used:

```javascript
// Generated outside the repository at /tmp/computerworld-predecessor-vitest.config.mjs.
// ROOT is the pinned SCE checkout; ENGINE is TCN generators/computer/engine.
export default {
  root: ROOT,
  resolve: { alias: {
    vitest: `${ENGINE}/node_modules/vitest`,
    // Each packages/* and ecosystems/* package manifest name -> its src/index.ts.
  } },
  test: { pool: 'threads', maxWorkers: 2, minWorkers: 1,
    include: ['packages/**/*.test.ts', 'tests/**/*.test.ts'], testTimeout: 30000 }
};
```

Invocation: `node <ENGINE>/node_modules/vitest/vitest.mjs run --config
/tmp/computerworld-predecessor-vitest.config.mjs`.

SCE test counts: Git 60; software 81; network 54; shell 33; VFS 33; application 39;
processes 22; fidelity acceptance 27; integration 15; catalog 8.

Tooling survey: Rust/cargo 1.97.1 installed; native ARM64 target available;
`wasm32-unknown-unknown`, wasm-pack and maturin not yet installed. Python 3.12.3,
uv, Node and Chrome are available. Wasm build/runtime, Python wheel, new renderer
and simulator benchmarks have **not** been run because implementation awaits
approval. No speedup or bundle-size claim is made for the proposed project.
