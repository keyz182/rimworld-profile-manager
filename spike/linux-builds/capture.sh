#!/usr/bin/env bash
# T50 spike: capture the Steam RimWorld install as a stored build.
# Usage: capture.sh <steam install dir> <builds dir>
# Copies with reflink (btrfs/xfs) and records version, Steam build id and space use.
set -euo pipefail

src=${1:?steam install dir}
builds=${2:?builds dir}
acf="$(dirname "$(dirname "$src")")/appmanifest_294100.acf"

version=$(tr -d '\r\n' <"$src/Version.txt")
buildid=$(sed -nE 's/^\s*"buildid"\s*"([0-9]+)".*/\1/p' "$acf")
state=$(sed -nE 's/^\s*"StateFlags"\s*"([0-9]+)".*/\1/p' "$acf")
# BetaKey appears in both UserConfig and MountedConfig; take the first.
beta=$(sed -nE 's/^\s*"BetaKey"\s*"([^"]*)".*/\1/p' "$acf" | head -n1)
slug=$(printf '%s' "$version" | tr '[:upper:] ' '[:lower:]-' | tr -cd 'a-z0-9.-')
dest="$builds/$slug"

echo "version='$version' buildid=$buildid StateFlags=$state BetaKey='${beta:-<none>}'"
if [ "$state" != 4 ]; then
  echo "refuse: StateFlags=$state (not fully installed)" >&2
  exit 3
fi
if [ -e "$dest" ]; then
  echo "exists: $dest (no-op)"
  exit 0
fi

mkdir -p "$builds"
staging="$builds/.staging-$slug"
rm -rf "$staging"
start=$(date +%s)
cp -a --reflink=always "$src" "$staging"
# Not part of the game: Steam's appid file (V52 must prove SteamAppId alone) and crash dumps.
rm -f "$staging/steam_appid.txt" "$staging"/mono_crash.*
mv "$staging" "$dest"
end=$(date +%s)

printf 'version = "%s"\nbuildid = "%s"\nbeta = "%s"\nsource_dir = "%s"\n' \
  "$version" "$buildid" "${beta:-}" "$src" >"$dest/build.spike.toml"
echo "captured -> $dest in $((end - start))s"
du -sh --apparent-size "$dest"
command -v btrfs >/dev/null && btrfs filesystem du -s "$dest" 2>/dev/null || true
