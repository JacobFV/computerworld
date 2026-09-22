#!/usr/bin/env python3
"""Regenerates tests/programs/*.expected from a host Node.js v24.

Run manually (never from the test suite) when a program changes:

    python3 crates/languages/jsvm/tests/generate_expected.py [NAME ...]

Each program runs as main.js (main.mjs for .mjs programs) in a scratch
directory, with NAME.stdin (if present) on standard input and TZ=UTC. The
scratch path is rewritten to /home/user so stack traces match the simulated
machine's layout.
"""
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
PROGRAMS = os.path.join(HERE, "programs")
NODE_VERSION = "v24.21.0"


def generate(file):
    name, ext = os.path.splitext(file)
    stdin_path = os.path.join(PROGRAMS, name + ".stdin")
    stdin = open(stdin_path, "rb").read() if os.path.exists(stdin_path) else b""
    main = "main" + ext
    with tempfile.TemporaryDirectory() as tmp:
        shutil.copy(os.path.join(PROGRAMS, file), os.path.join(tmp, main))
        # The host environment is kept: version-manager shims need it.
        env = dict(os.environ, TZ="UTC", NO_COLOR="1")
        env.pop("FORCE_COLOR", None)
        env.pop("NODE_OPTIONS", None)
        r = subprocess.run(["node", main], cwd=tmp, input=stdin, env=env,
                           capture_output=True, timeout=60)
        out = r.stdout.decode().replace(tmp, "/home/user")
        err = r.stderr.decode().replace(tmp, "/home/user")
    with open(os.path.join(PROGRAMS, name + ".expected"), "w") as f:
        f.write(f"--- exit: {r.returncode}\n--- stdout\n{out}--- stderr\n{err}")
    print(f"{name}: exit {r.returncode}")


def main():
    version = subprocess.run(["node", "--version"], capture_output=True, text=True).stdout.strip()
    if version != NODE_VERSION:
        sys.exit(f"expected outputs are defined by Node.js {NODE_VERSION}, found {version}")
    files = sorted(f for f in os.listdir(PROGRAMS) if f.endswith((".js", ".mjs")))
    if sys.argv[1:]:
        files = [f for f in files if os.path.splitext(f)[0] in sys.argv[1:]]
    for file in files:
        generate(file)


if __name__ == "__main__":
    main()
