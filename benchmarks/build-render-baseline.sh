#!/usr/bin/env bash
# Build only the historical scene/renderer plus the same standalone runner.
set -euo pipefail
revision="${RENDER_BASELINE_REVISION:-2c530b1}"
destination="${1:-/tmp/computerworld-render-baseline}"
if [ -e "$destination" ]; then
 echo "Destination already exists: $destination (choose an empty path)" >&2
 exit 1
fi
mkdir -p "$destination/benchmarks/runner/src"
git archive "$revision" crates/scene crates/render | tar -x -C "$destination"
# The original pre-optimization runner (prior to adding cold renderer/reset cases).
git show c8396d6:benchmarks/runner/src/main.rs > "$destination/benchmarks/runner/src/main.rs"
python3 - "$destination" <<'PY'
import pathlib,sys
p=pathlib.Path(sys.argv[1])
root=pathlib.Path('Cargo.toml').read_text()
# Preserve versions and release optimization settings, replace workspace membership.
start=root.index('[workspace.package]')
header='[workspace]\nresolver="2"\nmembers=["crates/scene","crates/render","benchmarks/runner"]\n\n'
(p/'Cargo.toml').write_text(header+root[start:])
manifest=pathlib.Path('benchmarks/runner/Cargo.toml').read_text()
# Historical source archives retain the old layout; only today's runner moved.
manifest=manifest.replace('../../crates/graphics/', '../../crates/')
manifest='\n'.join(line for line in manifest.splitlines() if not line.startswith('computerworld ='))+'\n'
(p/'benchmarks/runner/Cargo.toml').write_text(manifest)
PY
cp Cargo.lock "$destination/Cargo.lock"
cargo build --release --offline --manifest-path "$destination/Cargo.toml" -p cw-benchmarks
printf '%s\n' "$destination/target/release/cw-benchmarks"
