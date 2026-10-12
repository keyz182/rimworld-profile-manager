#!/usr/bin/env bash
# Adds RimWorld Profile Manager to your desktop's app menu, with its icon, for this user.
# Writes only below ~/.local/share (applications/ and icons/). Run it again after moving
# this folder; run with --remove to take the entry out again.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
entry="$data/applications/rwpm.desktop"

if [[ "${1:-}" == "--remove" ]]; then
  rm -f "$entry"
  for n in 16 32 48 64 128 256; do
    rm -f "$data/icons/hicolor/${n}x${n}/apps/rwpm.png"
  done
  rm -f "$data/icons/hicolor/scalable/apps/rwpm.svg"
  echo "Removed the RimWorld Profile Manager menu entry."
  exit 0
fi

for n in 16 32 48 64 128 256; do
  install -Dm644 "$here/icons/rwpm-$n.png" "$data/icons/hicolor/${n}x${n}/apps/rwpm.png"
done
install -Dm644 "$here/icons/rwpm.svg" "$data/icons/hicolor/scalable/apps/rwpm.svg"
mkdir -p "$(dirname "$entry")"
cat >"$entry" <<DESKTOP
[Desktop Entry]
Type=Application
Name=RimWorld Profile Manager
Comment=Separate RimWorld profiles with their own saves, settings and mods
Exec="$here/rwpm"
Icon=rwpm
Terminal=false
Categories=Game;Utility;
StartupWMClass=rwpm
DESKTOP
echo "Added RimWorld Profile Manager to the app menu ($entry)."
