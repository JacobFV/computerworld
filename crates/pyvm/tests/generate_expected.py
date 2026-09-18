#!/usr/bin/env python3
"""Regenerates tests/programs/*.expected from a host CPython 3.12.

Run manually (never from the test suite) when a program changes:

    python3 crates/pyvm/tests/generate_expected.py [NAME ...]

Each program runs as /home/user/main.py-equivalent in a scratch directory, with
NAME.stdin (if present) on standard input. The scratch path is rewritten to
/home/user so tracebacks match the simulated machine's layout.
"""
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
PROGRAMS = os.path.join(HERE, "programs")


def generate(name):
    src = os.path.join(PROGRAMS, name + ".py")
    stdin_path = os.path.join(PROGRAMS, name + ".stdin")
    stdin = open(stdin_path, "rb").read() if os.path.exists(stdin_path) else b""
    with tempfile.TemporaryDirectory() as tmp:
        shutil.copy(src, os.path.join(tmp, "main.py"))
        r = subprocess.run([sys.executable, "main.py"], cwd=tmp, input=stdin,
                           capture_output=True, timeout=60)
        out = r.stdout.decode().replace(tmp, "/home/user")
        err = r.stderr.decode().replace(tmp, "/home/user")
    with open(os.path.join(PROGRAMS, name + ".expected"), "w") as f:
        f.write(f"--- exit: {r.returncode}\n--- stdout\n{out}--- stderr\n{err}")
    print(f"{name}: exit {r.returncode}")


def main():
    if sys.version_info[:2] != (3, 12):
        sys.exit("expected outputs are defined by CPython 3.12")
    names = sys.argv[1:] or sorted(f[:-3] for f in os.listdir(PROGRAMS) if f.endswith(".py"))
    for name in names:
        generate(name)


if __name__ == "__main__":
    main()
