# Release process

A release is one run of `.github/workflows/publish.yml`, so that what was built, what was
checked and what was uploaded are in a public log rather than in someone's terminal. It
consists of a source tag, release notes, Python wheels, browser/Node Wasm assets, the npm
package, their checksums, and the same wheels and package on PyPI and npm. Versions with
a prerelease suffix are marked prerelease on GitHub and go to npm under the `next` tag.

## Making a release

1. Land the version bump (below), `CHANGELOG.md` and `docs/releases/v<version>.md` on
   `main`, and wait for the `verify` workflow to pass on that commit.
2. Run **publish** from the Actions tab (or
   `gh workflow run publish.yml -f source_commit=<40-character sha>`). It:
   - builds and verifies the candidates with `release.yml` (wheels on three platforms
     installed into clean environments; the Wasm bundles and the npm package installed
     into an empty project);
   - requires all four builds to report the same state and pixel hashes;
   - refuses a version that is already tagged, then tags the commit and creates the
     GitHub release with `SHA256SUMS`;
   - downloads the wheels and the npm tarball back from that release, checks them
     against `SHA256SUMS`, and publishes them.
3. To publish a registry later, or again after a failure, run it with **create_release**
   off: it uses the release that exists and rebuilds nothing. A registry never accepts
   the same version twice, so a partly successful run is finished this way, not redone.

### Credentials

- **PyPI** holds no secret. The project trusts this repository's `publish.yml` in the
  `pypi` environment as a [trusted publisher](https://docs.pypi.org/trusted-publishers/).
- **npm** holds no secret either. The `computerworld` package trusts this repository's
  `publish.yml` in the `npm` environment as a
  [trusted publisher](https://docs.npmjs.com/trusted-publishers/), with direct
  `npm publish` allowed (not only staging), and the package's publishing access is set to
  require two-factor authentication and disallow tokens. The 0.1.0 tarball was published
  once by hand from the GitHub release, because npm cannot trust a workflow for a package
  that does not exist yet; every later version is published by the workflow. To change
  the trust, `npm trust github computerworld --repo JacobFV/computerworld --file publish.yml --environment npm --allow-publish`
  from a logged-in npm 11.15+ with two-factor authentication.
- **crates.io is not published.** The workspace is fifty-one crates joined by path
  dependencies without versions, new crates are rate-limited to one every ten minutes
  after the first five, and `cw-render` embeds about 38 MB of fonts and wallpapers
  against a 10 MB limit per crate. Publishing there means moving those assets out of the
  crate (or having the limit raised), giving every internal dependency a version, and
  publishing in dependency order. Until then Rust consumers pin the Git tag, and no
  document may tell them to `cargo add computerworld`.

## Version identity

The current release uses:

| Surface | Version |
|---|---|
| Git tag / Cargo / engine / npm | `v0.1.2` / `0.1.2` |
| Python distribution | `0.1.2` |

A prerelease spells the two differently (`0.1.0-alpha.3` and `0.1.0a3`).

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
- [ ] The version tag resolves to the verified source commit; a prerelease version is
  marked **prerelease**, and the downloadable assets match the verified checksums.

## Verification commands

From the tagged source checkout with Rust, Python, Node, `maturin` and the locked
`wasm-bindgen-cli` installed:

```sh
bash scripts/test-all.sh
bash scripts/smoke-bindings.sh
node examples/javascript/computer-interaction.mjs --output target/javascript-demo
python examples/python/computer_interaction.py --output target/python-demo --compare target/javascript-demo
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
