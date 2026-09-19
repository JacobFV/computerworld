# Release process

Releases are versioned GitHub prereleases until the API and snapshot formats have
an explicit stability policy. A release consists of a source tag, release notes,
compatible Python wheels, browser/Node Wasm assets and their checksums. Registry
publication is a separate operation; never imply that a GitHub release publishes
to PyPI, npm or crates.io.

## Version identity

The current prerelease uses:

| Surface | Version |
|---|---|
| Git tag / Cargo / engine | `v0.1.0-alpha.3` / `0.1.0-alpha.3` |
| Python distribution | `0.1.0a3` |

Update workspace/dependent Cargo versions, `Cargo.lock`, Python project metadata,
release packaging metadata and release notes together. Python exposes
`computerworld.__version__` and `computerworld.engine_version`; JavaScript exposes
`engineVersion()`. Compare these with the release being installed. Keep the
resolved full source commit with experiment records, not just a moving branch.

## Acceptance checklist

Do not publish a release with unchecked failures. Record actual results in the
release workflow/logs; the following list describes requirements, not past results.

- [ ] The source commit is clean and pushed; release notes and license notices are included.
- [ ] The complete verification workflow is green for that commit: Rust tests,
  formatting, strict Clippy, boundaries, host-adapter checks and Wasm build.
- [ ] Binding/browser verification is green: Python/Node execution and state/pixel
  parity, programmatic demos, offline browser checks and desktop interaction tests.
- [ ] Each advertised wheel builds on its target and installs into a clean Python
  environment. Import/version checks and a real terminal/world interaction pass.
- [ ] Unpacked browser/Node bundles load their included Wasm, report the expected
  engine version and run the included examples. The browser console runs from a
  static file server with no simulation backend.
- [ ] Asset names and supported platforms match what was actually built; failed or
  unavailable targets are omitted or explicitly documented rather than advertised.
- [ ] Release artifacts carry project/font/icon/wallpaper notices, checksums and
  source/version metadata. Verify SHA-256 after downloading staged artifacts.
- [ ] The version tag resolves to the verified source commit; the release is marked
  **prerelease**, and its downloadable assets match the verified checksums.

## Verification commands

From the tagged source checkout with Rust, Python, Node, `maturin`, the locked
`wasm-bindgen-cli`, and Playwright/Chromium installed:

```sh
bash scripts/test-all.sh
bash scripts/smoke-bindings.sh
node examples/javascript/computer-interaction.mjs --output target/javascript-demo
python examples/python/computer_interaction.py --output target/python-demo --compare target/javascript-demo
node examples/browser/build.mjs
node scripts/test-browser.mjs
node scripts/test-desktops.mjs
node scripts/test-desktop-overhaul.mjs
```

Wheel and Wasm artifact checks must also run against the packaged outputs; source
checkout tests alone do not detect missing archive resources or installation bugs.
The release workflow is the executable platform/build specification. Existing
benchmark reports are measurements of their recorded revisions and hardware;
a release does not silently turn them into measurements of every new build.

## Consumer verification

Download `SHA256SUMS` alongside release assets. On Linux, check the downloaded
files using `sha256sum --check --ignore-missing SHA256SUMS`; on macOS use
`shasum -a 256` to compare with the corresponding entry. On Windows use
`Get-FileHash <asset> -Algorithm SHA256` and compare the hash. Checksums detect file
corruption/substitution relative to that manifest; they are not a separate
cryptographic signature or a claim of reproducible binary builds.

Pin the Git tag or exact asset/version in dependencies. Python and JS consumers
should not mix files from different releases. Preserve world definitions and action
traces in addition to snapshots: alpha checkpoint formats can change. Publish a
new version for fixes rather than replacing an existing version's artifacts.
