# Working on ComputerWorld

Start with the [runtime architecture](architecture.md) to understand the simulation,
then use the [workspace map](../crates/README.md) to find the package that owns your
change. Package names are stable even when their directories move; select one with
`cargo test -p <package>`.

## Find the right place

| Work | Start here |
| --- | --- |
| Actions, actor sessions and snapshots | `crates/core/environment/` |
| Filesystems, processes and shell commands | `crates/machines/computer/` |
| Python and JavaScript inside the simulation | `crates/languages/` |
| HTML, CSS and browser behavior | `crates/web/engine/` and `crates/web/browser/` |
| Scenes and pixels | `crates/graphics/` |
| Native apps and their domain engines | `crates/applications/` and `crates/engines/` |
| Simulated internet services | `crates/services/` |
| Public Rust, Python and JavaScript interfaces | `crates/computerworld/` and `crates/bindings/` |
| Website and documentation | `site/`, `docs/` and `scripts/site/` |

Keep tests and fixtures with their owning package. Extend an existing module when
the responsibility fits; introduce a crate when it creates a useful dependency or
API boundary. Simulation code must use deterministic inputs instead of host time,
randomness, files or network access. The [security](security.md) and
[determinism](determinism.md) guides explain those boundaries.

## Build and check a change

Read the repository's [working instructions](../CLAUDE.md) before running builds.
They include the shared Cargo lock and resource limits for this development machine.
Run commands below from the repository root.

| Command | Purpose |
| --- | --- |
| `bash scripts/test-all.sh` | Workspace tests, isolation checks, lint, Wasm and generated content |
| `python3 scripts/checks/check-boundaries.py` | Check that simulation packages respect host I/O boundaries |
| `bash scripts/build-wasm.sh` | Build the browser and Node bindings into `pkg/` |
| `bash scripts/smoke-bindings.sh` | Check parity across language bindings |
| `bash scripts/build-content.sh` | Regenerate world definitions and search indexes |
| `node scripts/site/build-docs.mjs --check` | Check the documentation index and page inventory |

The [development command map](../scripts/README.md) points to site, content,
release and verification tools. Use [release instructions](releasing.md) when
changing packaging or distribution.

## Edit sources and regenerate outputs

World inputs live in [worlds/](../worlds/README.md). The site's checked-in
`site/generated/world-definition.js` is produced by `bash scripts/build-content.sh`;
edit its inputs and regenerate it. Build products in `target/`, `pkg/`, `site/pkg/`
and `site/docs/` are ignored.

Documentation pages are authored in `docs/`. Register a new guide in the `sections`
list in `scripts/site/build-docs.mjs`, then run `node scripts/site/build-docs.mjs`.
That writes the website pages and refreshes `docs/README.md` from the same reading
order. The [site guide](../site/README.md) covers local previews, live scenes,
browser checks and screenshots.

Current guides live at the top of `docs/`. [Binding contracts](contracts/README.md)
remain requirements; [archived plans and reports](archive/README.md) record earlier
decisions. [Research notes and studies](../research/README.md) retain their evidence
and provenance separately from current contributor guidance.
