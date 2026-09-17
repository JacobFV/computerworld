#!/usr/bin/env bash
# Run from a virtualenv containing maturin. Builds both actual bindings and verifies
# equivalent semantic state plus portable checkpoint import across WASM and Python.
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/build-wasm.sh
maturin build --release --manifest-path crates/python/Cargo.toml --out target/python-wheel
python -m pip install --force-reinstall --no-index --find-links target/python-wheel computerworld
mkdir -p target/binding-checks
node scripts/smoke-node.cjs target/binding-checks/wasm.json
python examples/python/smoke.py target/binding-checks/wasm.json
