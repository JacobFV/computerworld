# Repository organization design

Approved scope: the repository review followed by “yes, full impl, go”.

Keep one Cargo workspace and the existing package names, public APIs, dependency
features, world schemas, snapshot bytes and runtime behavior. Improve navigation
by grouping crates, separating historical material and generated artifacts, and
extracting cohesive modules from the environment, applications and shell.

Crates live under core (protocol, determinism, sdk, kernel, environment, trajectory,
evaluation, blueprint), machines (computer, network, host-adapters), languages
(pyvm, jsvm, script-host, regex, tz, zlib), web (engine, browser), graphics (scene,
render, artwork, map), engines (cad, eda, sheet, sql, raster, video), bindings
(python, wasm), applications, services, and computerworld. The web engine becomes
crates/web/engine to avoid a grouping/package name collision. Services retain
existing child crates. This migration does not invent new packages.

Move superseded plans/reports into docs/archive; put active web-engine contracts
in docs/contracts. Preserve guide URLs and the generated documentation index.
Organize research as named studies and reference notes. Keep reference captures
and benchmark baselines tracked; label generated data using .gitattributes and
README guidance. Move the generated site world into site/generated; retain it in
git so the static site remains usable without adding a content build prerequisite.

Group scripts by site, content, release, checks and existing conversion/parity
workflows. Preserve top-level build-wasm.sh, build-content.sh, test-all.sh and
smoke-bindings.sh as common entry points. Update paths in code, manifests,
workflows, documentation, asset packaging and generators together.

Verification compares Cargo package/dependency/target metadata before and after,
checks local references and generated content, runs existing affected unit and
integration tests including determinism, builds Wasm/docs/bindings, and runs CI
format and lint gates. All Cargo commands serialize on the repository's shared
lock with four build jobs. Review the final diff independently before integration.
