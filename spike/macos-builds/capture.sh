#!/usr/bin/env bash
# Spike: capture the Steam RimWorld install as a stored build (macOS).
# Usage: capture.sh <steam install dir> <builds dir> [--pristine] [--ditto]
#
# Differences from the Linux capture, all forced by the platform:
#   * Version.txt lives INSIDE the bundle (<install>/RimWorldMac.app/Version.txt), not at
#     the install root. There is no such file at the install root at all.
#   * The copy is an APFS clone (`cp -cR`), the macOS analogue of --reflink=always.
#     --ditto takes a full copy instead, so the two can be compared: only ditto is
#     documented to preserve xattrs and resource forks (scripts/package.sh relies on that
#     for the release zip).
#   * The "already captured" test keys on the appmanifest buildid, NOT on the version
#     string: buildid is the build identity, and requires that the same Version.txt
#     with a new buildid is offered as an update rather than silently skipped.
set -euo pipefail

src=${1:?steam install dir}
builds=${2:?builds dir}
shift 2
pristine=0
mode=clone
for a in "$@"; do
  case "$a" in
  --pristine) pristine=1 ;;
  --ditto) mode=ditto ;;
  *)
    echo "unknown option: $a" >&2
    exit 2
    ;;
  esac
done

app=$src/RimWorldMac.app
acf="$(dirname "$(dirname "$src")")/appmanifest_294100.acf"

# Version.txt is CRLF, so strip \r as well as \n.
version=$(tr -d '\r\n' <"$app/Version.txt")
buildid=$(sed -nE 's/^[[:space:]]*"buildid"[[:space:]]*"([0-9]+)".*/\1/p' "$acf")
state=$(sed -nE 's/^[[:space:]]*"StateFlags"[[:space:]]*"([0-9]+)".*/\1/p' "$acf")
# BetaKey appears in both UserConfig and MountedConfig when set; absent = public branch.
beta=$(sed -nE 's/^[[:space:]]*"BetaKey"[[:space:]]*"([^"]*)".*/\1/p' "$acf" | head -n1)
slug=$(printf '%s' "$version" | tr '[:upper:] ' '[:lower:]-' | tr -cd 'a-z0-9.-')
[ "$pristine" = 1 ] && slug="$slug-pristine"
[ "$mode" = ditto ] && slug="$slug-ditto"
dest="$builds/$slug"

echo "version='$version' buildid=$buildid StateFlags=$state BetaKey='${beta:-<none>}' mode=$mode pristine=$pristine"

# Bit 4 stays set during an update, so only == 4 means idle.
if [ "$state" != 4 ]; then
  echo "refuse: StateFlags=$state (not fully installed)" >&2
  exit 3
fi

# Identity is the buildid. Same version + new buildid is an update, not a no-op.
if [ -e "$dest" ]; then
  have=$(sed -nE 's/^buildid = "([0-9]+)"$/\1/p' "$dest/build.spike.toml" 2>/dev/null || true)
  if [ "$have" = "$buildid" ]; then
    echo "exists: $dest buildid=$have (no-op)"
    exit 0
  fi
  echo "exists: $dest but buildid $have -> $buildid (an update, not a no-op)" >&2
  exit 4
fi

mkdir -p "$builds"
staging="$builds/.staging-$slug"
rm -rf "$staging"

free_before=$(df -k "$builds" | awk 'NR==2{print $4}')
start=$(date +%s)
case "$mode" in
clone) cp -cR "$src" "$staging" ;;
ditto) ditto "$src" "$staging" ;;
esac
end=$(date +%s)

# Not part of the game. steam_appid.txt sits in three places on macOS; it has to be
# proved with the SteamAppId env var alone, so every copy of it goes.
rm -f "$staging/steam_appid.txt" \
  "$staging/RimWorldMac.app/steam_appid.txt" \
  "$staging/RimWorldMac.app/Contents/MacOS/steam_appid.txt"
rm -f "$staging"/mono_crash.* "$staging/libdoorstop.dylib" "$staging/run-nowait.sh"
if [ "$pristine" = 1 ]; then
  # Prior-spike debris (Doorstop.dll, libdoorstop*.dylib, run.sh) and local mods.
  rm -rf "$staging/RimWorldMac.app/bak"
  find "$staging/RimWorldMac.app/Mods" -mindepth 1 -maxdepth 1 ! -name 'Core' \
    -exec rm -rf {} + 2>/dev/null || true
fi

free_after=$(df -k "$builds" | awk 'NR==2{print $4}')
mv "$staging" "$dest"

# Data lives inside the bundle. build.toml wants the DLC FOLDER names, so
# "Place official expansions here.txt" must not appear in the list.
dlc=$(find "$dest/RimWorldMac.app/Data" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort | paste -sd, -)

{
  printf 'version = "%s"\n' "$version"
  printf 'buildid = "%s"\n' "$buildid"
  printf 'beta = "%s"\n' "${beta:-}"
  printf 'dlc = "%s"\n' "$dlc"
  printf 'source_dir = "%s"\n' "$src"
  printf 'copy_mode = "%s"\n' "$mode"
  printf 'pristine = %s\n' "$pristine"
  printf 'seconds = %s\n' "$((end - start))"
} >"$dest/build.spike.toml"

# A truncated copy would only surface at launch as LaunchError::BadBundle, so assert the
# two things launch.rs::resolve_macos_bundle depends on now, while the cause is obvious.
plist=$dest/RimWorldMac.app/Contents/Info.plist
exe_name=$(sed -nE '/<key>CFBundleExecutable<\/key>/{n;s/.*<string>(.*)<\/string>.*/\1/p;}' "$plist")
if [ -z "$exe_name" ]; then
  echo "BAD: no CFBundleExecutable in $plist" >&2
  exit 5
fi
if [ ! -x "$dest/RimWorldMac.app/Contents/MacOS/$exe_name" ]; then
  echo "BAD: inner Mach-O missing or not executable: $exe_name" >&2
  exit 5
fi
echo "ok: CFBundleExecutable='$exe_name' resolves and is executable"

echo "captured -> $dest in $((end - start))s"
echo "du -sh (logical): $(du -sh "$dest" | cut -f1)"
echo "df free delta (KiB, a clone should be ~0): $((free_before - free_after))"
