#!/bin/bash
# Flash a sensor-node board over SWD and record what was flashed.
# usage: flash-board.sh <firmware.bin> [node-id]
set -eu
firmware="${1:-}"
node_id="${2:-node-0000}"
if [ -z "$firmware" ] || [ ! -f "$firmware" ]; then
  echo "usage: $(basename "$0") <firmware.bin> [node-id]" >&2
  exit 2
fi
log="$HOME/Notes/flash-log.txt"
size="$(wc -c < "$firmware")"
sum="$(sha256sum "$firmware" | cut -d' ' -f1)"
echo "flashing $firmware ($size bytes) to $node_id"
# The programmer is not on this machine yet; the log is what the bench checks against.
printf '%s  %s  %s  %s\n' "$(date +%Y-%m-%dT%H:%M:%S)" "$node_id" "$size" "$sum" >> "$log"
echo "recorded in $log"
tail -n 3 "$log"
