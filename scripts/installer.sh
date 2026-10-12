#!/usr/bin/env bash
# Builds the Windows installer from an unpacked Windows bundle (made by package.sh windows).
# usage: installer.sh <target/dist/rwpm-<version>-windows-x86_64>
# out:   target/dist/rwpm-<version>-windows-x86_64-setup.exe
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
src="$(cd "${1:?usage: installer.sh <unpacked windows bundle dir>}" && pwd)"
name="$(basename "$src")"
[[ "$name" =~ ^rwpm-([0-9]+\.[0-9]+\.[0-9]+)-windows-x86_64$ ]] || {
  echo "installer.sh: '$name' is not a Windows bundle folder" >&2
  exit 64
}
version="${BASH_REMATCH[1]}"
command -v makensis >/dev/null || {
  echo "installer.sh: makensis not found (Debian/Ubuntu: apt install nsis)" >&2
  exit 1
}

out="$ROOT/target/dist/$name-setup.exe"
makensis -V2 -WX -DVERSION="$version" -DSRC="$src" -DOUT="$out" "$ROOT/packaging/windows/rwpm.nsi" >&2
echo "$out"
