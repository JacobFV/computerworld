#!/usr/bin/env python3
"""Install a candidate wheel in a fresh venv and exercise its actual runtime."""
from pathlib import Path
import subprocess
import sys
import venv

root = Path(__file__).resolve().parents[1]
wheels = sorted((root / 'target/release-wheels').glob('*.whl'))
assert len(wheels) == 1, f'Expected exactly one candidate wheel: {wheels}'
env = root / 'target/release-wheel-venv'
venv.EnvBuilder(with_pip=True, clear=True).create(env)
python = env / ('Scripts/python.exe' if sys.platform == 'win32' else 'bin/python')
subprocess.run([str(python), '-m', 'pip', 'install', '--no-deps', str(wheels[0])], check=True)
subprocess.run([str(python), '-c',
    "import computerworld, importlib.metadata; "
    "assert computerworld.__version__ == importlib.metadata.version('computerworld'); "
    "assert computerworld.engine_version == '0.1.0-alpha.1'; "
    "print('Installed release:', computerworld.__version__, computerworld.engine_version)"], check=True)
subprocess.run([str(python), str(root / 'examples/python/smoke.py')], check=True, cwd=root)
subprocess.run([str(python), str(root / 'examples/python/computer_interaction.py'),
                '--output', str(root / 'target/release-python-evidence')], check=True, cwd=root)
