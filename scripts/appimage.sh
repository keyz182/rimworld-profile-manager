#!/usr/bin/env bash
# Builds the Linux AppImage from an unpacked Linux bundle (made by package.sh linux).
# usage: appimage.sh <target/dist/rwpm-<version>-linux-x86_64>
# out:   target/dist/rwpm-<version>-x86_64.AppImage
# rwpm copies itself out of the AppImage on start (crates/rwpm-core/src/appimage.rs).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Pinned by version and checksum; downloaded once into target/appimage-tools.
TOOL_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"
TOOL_SHA="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
RUNTIME_URL="https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64"
RUNTIME_SHA="2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"

src="$(cd "${1:?usage: appimage.sh <unpacked linux bundle dir>}" && pwd)"
name="$(basename "$src")"
[[ "$name" =~ ^rwpm-([0-9]+\.[0-9]+\.[0-9]+)-linux-x86_64$ ]] || {
  echo "appimage.sh: '$name' is not a Linux bundle folder" >&2
  exit 64
}
version="${BASH_REMATCH[1]}"

tools="$ROOT/target/appimage-tools"
mkdir -p "$tools"
fetch() { # url sha file
  if [[ -f "$3" ]] && echo "$2  $3" | sha256sum -c --quiet 2>/dev/null; then return; fi
  curl -sSfL -o "$3.part" "$1"
  echo "$2  $3.part" | sha256sum -c --quiet
  mv "$3.part" "$3"
}
fetch "$TOOL_URL" "$TOOL_SHA" "$tools/appimagetool"
fetch "$RUNTIME_URL" "$RUNTIME_SHA" "$tools/runtime"
chmod +x "$tools/appimagetool"

# Built in target/appimage (not target/dist, which the release uploads from).
appdir="$ROOT/target/appimage/$name.AppDir"
[[ ! -e "$appdir" ]] || trash "$appdir"
mkdir -p "$(dirname "$appdir")"
cp -a "$src" "$appdir"
# The AppImage is its own menu entry (via the player's AppImage integration tool).
rm -f "$appdir/install-desktop-entry.sh"
ln -s rwpm "$appdir/AppRun"
cp "$appdir/icons/rwpm-256.png" "$appdir/rwpm.png"
ln -s rwpm.png "$appdir/.DirIcon"
cat >"$appdir/rwpm.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=RimWorld Profile Manager
Comment=Separate RimWorld profiles with their own saves, settings and mods
Exec=rwpm
Icon=rwpm
Terminal=false
Categories=Game;Utility;
StartupWMClass=rwpm
X-AppImage-Version=$version
DESKTOP

out="$ROOT/target/dist/rwpm-$version-x86_64.AppImage"
# Extract-and-run: no FUSE needed on the build machine.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tools/appimagetool" --no-appstream \
  --runtime-file "$tools/runtime" "$appdir" "$out" >&2
echo "$out"
