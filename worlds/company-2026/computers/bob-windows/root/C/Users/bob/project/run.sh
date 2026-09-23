#!/bin/bash
# Summarise the sample log, then check it against the spec limits.
set -e
cd "$(dirname "$0")"
python3 -m telemetry.cli summarize data/measurements.csv
echo
python3 -m telemetry.cli check data/measurements.csv --limit=temp=-20:70
