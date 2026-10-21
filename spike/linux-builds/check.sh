#!/usr/bin/env bash
# Spike: evidence for the pinned-build checks from a running (or just-run) profile.
# Usage: check.sh <profile id> <expected build dir>
set -uo pipefail

profile=${1:?profile id}
build=${2:?build dir}
pdir="$HOME/.local/share/RimWorld Profile Manager/profiles/$profile"
log="$pdir/Player.log"

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

echo "== build: $(tr -d '\r' <"$build/Version.txt" 2>/dev/null) ($build)"
echo "== log version: $(rg -m1 -o 'RimWorld [0-9]+\.[0-9]+\.[0-9]+ rev[0-9]+' "$log")"

mono=$(rg -m1 -o "Mono path\[0\] = '[^']*'" "$log")
case "$mono" in *"$build/"*) ok "game ran from build ($mono)" ;; *) no "game ran from: $mono" ;; esac

if rg -q 'SteamAPI_Init\(\): Loaded .* OK' "$log"; then ok "SteamAPI_Init ok"; else no "SteamAPI_Init"; fi
if [ -e "$build/steam_appid.txt" ]; then no "build still has steam_appid.txt"; else ok "no steam_appid.txt in build"; fi
ws=$(rg -c 'WorkshopItem|steamapps/workshop/content/294100' "$log" || echo 0)
if [ "${ws:-0}" -gt 0 ]; then ok "workshop seen ($ws lines)"; else no "no workshop lines"; fi

# start_RimWorld.sh cds into the install and runs ./RimWorldLinux, so match on /proc exe.
pid=""
for p in /proc/[0-9]*; do
  [ "$(readlink "$p/exe" 2>/dev/null)" = "$build/RimWorldLinux" ] && pid=${p#/proc/}
done
if [ -n "$pid" ]; then
  exe=$(readlink "/proc/$pid/exe")
  case "$exe" in "$build/"*) ok "live exe $exe" ;; *) no "live exe $exe" ;; esac
  if rg -q gameoverlayrenderer "/proc/$pid/maps"; then ok "overlay renderer mapped"; else no "overlay not mapped"; fi
else
  echo "info: no running game from build (exe/overlay checks skipped)"
fi

if [ -f "$pdir/last-run.toml" ]; then
  echo "== last-run.toml:"
  sed 's/^/   /' "$pdir/last-run.toml"
fi
echo "== $pass pass, $fail fail"
[ "$fail" = 0 ]
