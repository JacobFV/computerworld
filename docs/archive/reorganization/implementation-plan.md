# Repository organization implementation plan

> Execute with the executing-plans workflow; use a scoped worker for the independent module extraction and an independent final reviewer.

**Goal:** Make the repository navigable while preserving behavior and public interfaces.
**Architecture:** Mechanical directory migration plus cohesive internal module extraction. Existing packages, sources of generated data, and reference evidence remain intact.
**Tech stack:** Rust/Cargo, Python, Node, shell, GitHub Actions.
**Spec:** design.md

## Global constraints

- Keep package names, APIs, features, snapshots and world schemas unchanged.
- Preserve historical records and reference fixtures; do not delete evidence.
- Cargo commands run serially under /home/brandonin/.claude/jobs/cargo.lock with CARGO_BUILD_JOBS=4, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0.
- Commit verified units; do not push.

## Review focus

- Relative resource paths and manifest paths still resolve after moves.
- Package release archives and Python asset notices include the same content.
- Nested script entry points work from arbitrary current directories.
- Documentation references and generated-content checks cover relocated files.
- Refactoring preserves method visibility, serialized data and test coverage.

### Task 1: Extract oversized modules

Own crates/core/environment/src, crates/applications/src/lib.rs and its new sibling modules,
and crates/machines/computer/src/shell.rs and its new shell child modules. Consume existing
public APIs; produce identical APIs with implementations grouped by responsibility.

- [x] Read the existing type/method boundaries and select cohesive groups.
- [x] Run existing tests as the baseline; preserve tests while moving them with their code.
- [x] Extract environment dispatch, scene/observation and snapshot helpers; desktop/application state responsibilities; shell lexing/expansion/parser/command responsibilities as warranted by the code.
- [x] Keep module-level imports explicit where practical and internal visibility narrow.
- [x] Run affected tests, determinism corpus and formatting. Expected: pass with no behavior changes.
- [x] Commit only the owned files and report counts and verification.

### Task 2: Organize documentation, research, scripts and generated artifacts

Own docs, research, scripts, site/generated, .github, .gitattributes, and references
outside Task 1. Consume current paths; produce the structure in design.md.

- [x] Move records/contracts and group research studies and scripts.
- [x] Rewrite relative references according to source/destination paths; retain stable guide URLs.
- [x] Move the existing site world to site/generated/world-definition.js; update imports and generators.
- [x] Add repository, scripts and evidence navigation guidance and generated-file attributes.
- [x] Run docs generation/check, script syntax checks and generated-content comparison. Expected: pass, generated world bytes unchanged.
- [x] Commit the verified migration.

### Task 3: Group workspace crates

Own manifests, crate directory moves, path references, boundary checker and navigation.
Consumes completed Tasks 1 and 2. Produces grouped directories with the same packages.

- [x] Record Cargo metadata; move crates according to design.md.
- [x] Rewrite manifest relative paths, embedded resources, repository-relative references and script root discovery.
- [x] Replace implicit workspace discovery with explicit package group globs.
- [x] Adapt boundary discovery to nested packages so all simulation libraries are covered.
- [x] Compare Cargo metadata package names, features, dependency edges and target types. Expected: no semantic changes.
- [x] Run workspace tests, clippy, formatting, Wasm, documentation, generated-content, packaging and browser checks. Expected: pass or documented pre-existing/environment limitation with evidence.
- [x] Commit the verified migration.

### Task 4: Review and integrate

- [x] Independently review the full migration and verify fixes.
- [x] Fast-forward the original clean checkout to the verified branch.
- [x] Confirm clean status and summarize the structure, tests and any remaining limitations.

## Verification recorded on 2026-09-22

- Workspace: 2,875 tests passed across 296 suites; 65 explicitly ignored.
- Native HTTP host adapter: 3 tests passed.
- Final restored-resource checks: 86 tests passed; 1 ignored.
- Final determinism and documented-router checks: 13 passed; 2 ignored.
- Rust 1.98 clippy with warnings denied, rustfmt, and Rust documentation passed.
- Cargo metadata preserves all 58 packages, features, dependency declarations and target kinds.
- All six generated world/index artifacts reproduced byte-for-byte.
- Five nested-package isolation regressions, 32 documentation pages and navigation links passed.
- The rebuilt Wasm bundle passed all eight browser checks. npm install smoke and the unpacked Node release example passed.
- Python wheel build/install, five threading checks, state/pixel/checkpoint parity with Wasm,
  replay, fork and the ASCII-locale demo passed. All 14 declared wheel attribution files match their sources.
- The source distribution resolves the complete 53-package Python dependency closure offline;
  standalone tooling and other bindings are intentionally outside that dependency closure.
- Independent review found and verified fixes for a fixture checksum, a source-scanning test,
  historical capture/benchmark provenance and embedded runtime text. No findings remain open.

Historical fixture and runtime-resource bytes are preserved. The renamed Svelte generator
retains its original provenance banner because the fixture checksum pins those bytes.

Integrated into the original `main` checkout. Ignored parity output and Python caches
were relocated without deletion; generated site docs and bindings were refreshed.
Post-integration documentation, package discovery and isolation checks pass.
