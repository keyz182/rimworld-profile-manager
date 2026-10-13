#!/usr/bin/env bash
# T52 spike: evidence for V50/V51/V52 from a running (or just-run) profile (macOS).
# Usage: check.sh <profile id> <expected build dir>
#
# The Linux T50 check reads /proc. macOS has none, so:
#   * live process identity comes from `ps -o comm=` / `lsof`, not /proc/<pid>/exe
#   * the overlay check greps the process's loaded images, not /proc/<pid>/maps
#   * two logs are consulted: the profile-local Player.log the payload writes, and the
#     Unity default at ~/Library/Logs/Ludeon Studios/... (R32)
#
# The version line ALONE cannot decide V50 when both trees hold the same version, which is
# the case on this host (no second macOS branch). `Mono path[0]` is the discriminator: it
# names the tree the engine actually loaded from.
set -uo pipefail

profile=${1:?profile id}
build=${2:?build dir}
pdir="$HOME/Library/Application Support/RimWorld Profile Manager/profiles/$profile"
unity_log="$HOME/Library/Logs/Ludeon Studios/RimWorld by Ludeon Studios/Player.log"
log="$pdir/Player.log"
[ -s "$log" ] || log=$unity_log

pass=0
fail=0
ok() {
  echo "PASS $1"
  pass=$((pass + 1))
}
no() {
  echo "FAIL $1"
  fail=$((fail + 1))
}
info() { echo "info: $1"; }

echo "== build:   $(tr -d '\r' <"$build/RimWorldMac.app/Version.txt" 2>/dev/null) ($build)"
echo "== log:     $log"
echo "== log ver: $(rg -m1 -o 'RimWorld [0-9]+\.[0-9]+\.[0-9]+ rev[0-9]+' "$log" 2>/dev/null)"

# ---- V50: which tree did the engine load from?
mono=$(rg -m1 -o "Mono path\[0\] = '[^']*'" "$log" 2>/dev/null)
case "$mono" in
*"$build/"*) ok "V50 game ran from build ($mono)" ;;
"") no "V50 no Mono path[0] line in $log" ;;
*) no "V50 game ran from: $mono" ;;
esac

# ---- V52: Steam reached with the SteamAppId env var alone.
if rg -q 'SteamAPI_Init\(\): Loaded .* OK|Steam.*initiali[sz]ed' "$log" 2>/dev/null; then
  ok "V52 SteamAPI_Init ok"
else
  no "V52 SteamAPI_Init not seen"
fi
for f in "$build/steam_appid.txt" "$build/RimWorldMac.app/steam_appid.txt" \
  "$build/RimWorldMac.app/Contents/MacOS/steam_appid.txt"; do
  [ -e "$f" ] && no "V52 build still has $f"
done
[ -e "$build/steam_appid.txt" ] || ok "V52 no steam_appid.txt at build root"
# V52 "workshop mods listed" has NO log signal on macOS. Measured 2026-10-13: the Steam
# install, launched by Steam itself with 524 workshop items present, logs zero workshop
# lines - so this is informational here, unlike the Linux T50 check where it is an
# assertion. The real check is the in-game mod list, recorded by hand.
ws=$(rg -c 'WorkshopItem|steamapps/workshop/content/294100' "$log" 2>/dev/null || echo 0)
info "V52 workshop log lines = ${ws:-0} (macOS logs none even for the Steam install; confirm in the mod list by hand)"

# ---- live process: identity, translocation, overlay.
exe_name=$(sed -nE '/<key>CFBundleExecutable<\/key>/{n;s/.*<string>(.*)<\/string>.*/\1/p;}' \
  "$build/RimWorldMac.app/Contents/Info.plist" 2>/dev/null)
pid=$(pgrep -f "$build/RimWorldMac.app/Contents/MacOS/$exe_name" | head -n1)
[ -n "$pid" ] || pid=$(pgrep -x "$exe_name" | head -n1)

if [ -n "${pid:-}" ]; then
  # lsof txt entries list the executable and every loaded image. -Fn is required, not the
  # default columns: the macOS game binary is "RimWorld by Ludeon Studios" and Steam lives
  # under "Application Support", so a column split on whitespace truncates both paths.
  imgs=$(lsof -p "$pid" -Fn 2>/dev/null | sed -n 's/^n//p')
  exe=$(printf '%s\n' "$imgs" | rg -m1 "MacOS/$exe_name" || true)
  echo "== live pid $pid exe: ${exe:-<not resolved>}"
  case "$exe" in
  "$build/"*) ok "V50 live exe under build" ;;
  *) no "V50 live exe: ${exe:-<unresolved>}" ;;
  esac

  # R30: a translocated bundle executes from a randomised read-only path.
  case "$exe" in
  /private/var/folders/*) no "R30 TRANSLOCATED: $exe" ;;
  *) ok "R30 not translocated" ;;
  esac

  # V52 overlay. R46 is the precedent: on Windows the library was present and the overlay
  # still did not draw, so presence is necessary, not sufficient. The human observation in
  # the results file is the primary datum.
  # Literal name measured on this host, Steam install launched by Steam:
  # <Steam.AppBundle>/Steam/Contents/MacOS/gameoverlayrenderer.dylib
  ov=$(printf '%s\n' "$imgs" | rg -i 'gameoverlayrenderer|steamoverlay' || true)
  if [ -n "$ov" ]; then ok "V52 overlay image loaded: $ov"; else no "V52 no overlay image in lsof"; fi
  info "overlay drawn? record the Shift-Tab observation by hand - lsof cannot answer it (R46)"
else
  info "no running game from build (live exe/translocation/overlay checks skipped)"
fi

if [ -f "$pdir/last-run.toml" ]; then
  echo "== last-run.toml:"
  sed 's/^/   /' "$pdir/last-run.toml"
fi
echo "== $pass pass, $fail fail"
[ "$fail" = 0 ]
