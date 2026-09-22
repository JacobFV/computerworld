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

Own crates/environment/src, crates/applications/src/lib.rs and its new sibling modules,
and crates/computer/src/shell.rs and its new shell child modules. Consume existing
public APIs; produce identical APIs with implementations grouped by responsibility.

- [ ] Read the existing type/method boundaries and select cohesive groups.
- [ ] Run existing tests as the baseline; preserve tests while moving them with their code.
- [ ] Extract environment dispatch, scene/observation and snapshot helpers; desktop/application state responsibilities; shell lexing/expansion/parser/command responsibilities as warranted by the code.
- [ ] Keep module-level imports explicit where practical and internal visibility narrow.
- [ ] Run affected tests, determinism corpus and formatting. Expected: pass with no behavior changes.
- [ ] Commit only the owned files and report counts and verification.

### Task 2: Organize documentation, research, scripts and generated artifacts

Own docs, research, scripts, site/generated, .github, .gitattributes, and references
outside Task 1. Consume current paths; produce the structure in design.md.

- [ ] Move records/contracts and group research studies and scripts.
- [ ] Rewrite relative references according to source/destination paths; retain stable guide URLs.
- [ ] Move the existing site world to site/generated/world-definition.js; update imports and generators.
- [ ] Add repository, scripts and evidence navigation guidance and generated-file attributes.
- [ ] Run docs generation/check, script syntax checks and generated-content comparison. Expected: pass, generated world bytes unchanged.
- [ ] Commit the verified migration.

### Task 3: Group workspace crates

Own manifests, crate directory moves, path references, boundary checker and navigation.
Consumes completed Tasks 1 and 2. Produces grouped directories with the same packages.

- [ ] Record Cargo metadata; move crates according to design.md.
- [ ] Rewrite manifest relative paths, embedded resources, repository-relative references and script root discovery.
- [ ] Replace implicit workspace discovery with explicit package group globs.
- [ ] Adapt boundary discovery to nested packages so all simulation libraries are covered.
- [ ] Compare Cargo metadata package names, features, dependency edges and target types. Expected: no semantic changes.
- [ ] Run workspace tests, clippy, formatting, Wasm, documentation, generated-content, packaging and browser checks. Expected: pass or documented pre-existing/environment limitation with evidence.
- [ ] Commit the verified migration.

### Task 4: Review and integrate

- [ ] Independently review the full migration and verify fixes.
- [ ] Fast-forward the original clean checkout to the verified branch.
- [ ] Confirm clean status and summarize the structure, tests and any remaining limitations.
