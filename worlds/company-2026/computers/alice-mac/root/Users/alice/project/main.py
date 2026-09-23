"""Entry point for the editor's Run button: same as ``python3 -m telemetry.cli``."""

import sys

from telemetry.cli import main

if __name__ == "__main__":
    argv = sys.argv[1:] or ["summarize", "data/measurements.csv"]
    sys.exit(main(argv))
