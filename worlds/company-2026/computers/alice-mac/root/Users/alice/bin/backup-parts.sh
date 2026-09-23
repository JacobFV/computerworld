#!/bin/bash
# Archive the CAD parts and KiCad projects into ~/Backups with today's date.
set -eu
stamp="$(date +%Y-%m-%d)"
dest="$HOME/Backups/$stamp"
mkdir -p "$dest"
for folder in "$HOME/Documents/Parts" "$HOME/Documents/KiCad"; do
  if [ -d "$folder" ]; then
    name="$(basename "$folder")"
    tar -czf "$dest/$name.tar.gz" -C "$(dirname "$folder")" "$name"
    echo "archived $name -> $dest/$name.tar.gz"
  fi
done
echo "files backed up:"
find "$dest" -type f | sort
