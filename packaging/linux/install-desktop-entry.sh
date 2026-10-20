#!/usr/bin/env bash
# Adds RimWorld Profile Manager to your desktop's app menu, with its icon, and makes it
# open .rimrim backups and .rimpack modpacks, for this user.
# Writes only below ~/.local/share (applications/, icons/ and mime/). Run it again after
# moving this folder; run with --remove to take the entry out again.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
entry="$data/applications/rwpm.desktop"
mime="$data/mime/packages/rwpm.xml"

# Desktop caches. A missing tool only means the desktop finds the change later.
refresh() {
  if command -v update-mime-database >/dev/null && [[ -d "$data/mime" ]]; then
    update-mime-database "$data/mime" || true
  fi
  if command -v update-desktop-database >/dev/null && [[ -d "$data/applications" ]]; then
    update-desktop-database "$data/applications" || true
  fi
}

if [[ "${1:-}" == "--remove" ]]; then
  rm -f "$entry" "$mime"
  for n in 16 32 48 64 128 256; do
    rm -f "$data/icons/hicolor/${n}x${n}/apps/rwpm.png"
  done
  rm -f "$data/icons/hicolor/scalable/apps/rwpm.svg"
  refresh
  echo "Removed the RimWorld Profile Manager menu entry."
  exit 0
fi

for n in 16 32 48 64 128 256; do
  install -Dm644 "$here/icons/rwpm-$n.png" "$data/icons/hicolor/${n}x${n}/apps/rwpm.png"
done
install -Dm644 "$here/icons/rwpm.svg" "$data/icons/hicolor/scalable/apps/rwpm.svg"
install -Dm644 "$here/rwpm-mime.xml" "$mime"
mkdir -p "$(dirname "$entry")"
cat >"$entry" <<DESKTOP
[Desktop Entry]
Type=Application
Name=RimWorld Profile Manager
Comment=Separate RimWorld profiles with their own saves, settings and mods
Exec="$here/rwpm" %f
Icon=rwpm
Terminal=false
Categories=Game;Utility;
MimeType=application/x-rimrim;application/x-rimpack;
StartupWMClass=rwpm
DESKTOP
refresh
echo "Added RimWorld Profile Manager to the app menu ($entry)."
echo "Backups (.rimrim) and modpacks (.rimpack) now open in it."
