#!/usr/bin/env bash
# Run only after the owner has stopped concurrent build/test workloads.
set -euo pipefail
mkdir -p benchmarks/results
CPU="${BENCH_CPU:-19}"
if [ -n "${RENDER_BASELINE_BINARY:-}" ]; then
 BENCH_OUTPUT=benchmarks/results/render-before.json taskset -c "$CPU" "$RENDER_BASELINE_BINARY" > /tmp/cw-bench-before.log
fi
BENCH_OUTPUT=benchmarks/results/render-after.json taskset -c "$CPU" target/release/cw-benchmarks > /tmp/cw-bench-render.log
taskset -c "$CPU" target/release/world > /tmp/cw-bench-world.log
BENCH_FILTER=dirty BENCH_OUTPUT=benchmarks/results/native-dirty-reset.json taskset -c "$CPU" target/release/world > /tmp/cw-bench-dirty.log
taskset -c "$CPU" target/release/many_worlds > /tmp/cw-bench-many.log
taskset -c "$CPU" benchmarks/run-predecessor.sh > /tmp/cw-bench-predecessor.log
taskset -c "$CPU" node benchmarks/bindings.mjs > /tmp/cw-bench-node.log
taskset -c "$CPU" .venv/bin/python benchmarks/python_binding.py > /tmp/cw-bench-python.log
taskset -c "$CPU" node benchmarks/browser-render.mjs > /tmp/cw-bench-browser.log
python3 benchmarks/machine.py
python3 benchmarks/summarize.py
