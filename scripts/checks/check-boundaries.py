#!/usr/bin/env python3
"""Reject ambient host capabilities in simulation libraries.

Conservative static gate, not a sandbox: registered Rust extensions remain trusted.
Integration tests, benchmarks, examples and binding crates may use host facilities.
"""
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[2]
# Simulation packages are checked by default, at any grouping depth. Only the
# explicit host/binding edges may use ambient capabilities.
HOST_PACKAGES = {'cw-host-adapters', 'cw-python', 'cw-wasm'}
FORBIDDEN_DEPS = {'tokio', 'async-std', 'reqwest', 'hyper', 'ureq', 'surf',
                  'getrandom', 'rand', 'chrono', 'web-sys', 'js-sys', 'pyo3'}
HOST = re.compile(r'\b(?:std\s*::\s*(?:fs|process|thread)|std\s*::\s*net\s*::\s*(?:TcpStream|TcpListener|UdpSocket|ToSocketAddrs)|'
                  r'std\s*::\s*time\s*::\s*(?:SystemTime|Instant)|'
                  r'(?:SystemTime|Instant)\s*::\s*now)\b')
GROUPED_IMPORT = re.compile(
    r'\buse\s+std\s*::\s*\{[^;]*\b(?:fs|process|thread)\b[^;]*\}\s*;'
    r'|\buse\s+std\s*::\s*net\s*::\s*\{[^;]*\b(?:TcpStream|TcpListener|UdpSocket|ToSocketAddrs)\b[^;]*\}\s*;',
    re.S,
)
errors = []
for manifest in sorted((ROOT / 'crates').rglob('Cargo.toml')):
    directory = manifest.parent
    data = tomllib.loads(manifest.read_text(encoding='utf-8'))
    if data.get('package', {}).get('name') in HOST_PACKAGES:
        continue
    tables = [data] + list(data.get('target', {}).values())
    for table in tables:
        for name, spec in table.get('dependencies', {}).items():
            actual = spec.get('package', name) if isinstance(spec, dict) else name
            if actual in FORBIDDEN_DEPS:
                errors.append(f'{manifest.relative_to(ROOT)}: forbidden dependency {actual}')
    # Follow explicit test-only module declarations. A module named tests is not
    # automatically exempt: it must actually be gated by cfg(test).
    sources = list((directory / 'src').rglob('*.rs'))
    test_roots = set()
    for path in sources:
        source = path.read_text(encoding='utf-8')
        base = path.parent if path.name in {'lib.rs', 'mod.rs'} else path.with_suffix('')
        for name in re.findall(r'#\[cfg\(test\)\]\s*mod\s+(\w+)\s*;', source):
            test_roots.update({base / f'{name}.rs', base / name})
    for path in sources:
        if any(path == test or test in path.parents for test in test_roots):
            continue
        relative = path.relative_to(directory / 'src')
        if relative.parts[0] == 'bin' or relative == Path('main.rs'):
            continue  # Explicit executable transport, outside the pure library.
        source = path.read_text(encoding='utf-8')
        # Conventional trailing unit-test modules are outside the runtime surface.
        source = re.split(r'#\[cfg\(test\)\]\s*(?:mod\s+tests|mod\s+test)\b', source)[0]
        source = re.sub(r'/\*.*?\*/|//[^\n]*', '', source, flags=re.S)
        for match in list(HOST.finditer(source)) + list(GROUPED_IMPORT.finditer(source)):
            errors.append(f'{path.relative_to(ROOT)}: ambient capability {match.group()}')
if errors:
    print('\n'.join(errors), file=sys.stderr)
    sys.exit(1)
print('Pure simulation crate boundaries passed.')
