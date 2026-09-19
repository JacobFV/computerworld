#!/usr/bin/env bash
# Run from a virtualenv containing maturin. Builds both actual bindings and verifies
# equivalent semantic state plus portable checkpoint import across WASM and Python.
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/build-wasm.sh
maturin build --release --manifest-path crates/python/Cargo.toml --out target/python-wheel
PACKAGE_VERSION=$(python -c 'import tomllib; print(tomllib.load(open("pyproject.toml", "rb"))["project"]["version"])')
python -m pip install --force-reinstall --no-index --find-links target/python-wheel "computerworld==$PACKAGE_VERSION"
mkdir -p target/binding-checks
node scripts/smoke-node.cjs target/binding-checks/wasm.json
python examples/python/smoke.py target/binding-checks/wasm.json
python crates/python/tests/test_threading.py
node scripts/smoke-desktop-pixels.cjs target/binding-checks/desktop.json
python examples/python/desktop_pixels.py target/binding-checks/desktop.json
# Byte-compare a whole episode across the two bindings: summary, actions, scene and
# observation, plus a portable checkpoint imported into the other runtime. This is what
# catches a Wasm bundle and a wheel built from different revisions, which otherwise
# surfaces as an unexplained state-hash difference.
node examples/javascript/computer-interaction.mjs --output target/binding-checks/javascript
python examples/python/computer_interaction.py --output target/binding-checks/python \
  --compare target/binding-checks/javascript
# Again under an ASCII locale: Windows decodes text in its locale code page, so an
# example that reads the UTF-8 world without an explicit encoding builds a different
# world there. This reproduces that failure on any platform.
LC_ALL=C PYTHONUTF8=0 python examples/python/computer_interaction.py \
  --output target/binding-checks/python-ascii-locale --compare target/binding-checks/javascript
