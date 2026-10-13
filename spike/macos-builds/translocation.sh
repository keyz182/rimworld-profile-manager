#!/usr/bin/env bash
# T52 spike (R30 / V46): does a quarantined stored build get app-translocated?
# Usage: translocation.sh <build dir> <profile id>
#
# WARNING, read before re-running: route b puts a Gatekeeper dialog in the user's face
# ("RimWorldMac is damaged and can't be opened"). It was run once, on 2026-10-13, and that
# is enough. Do not repeat it casually. Route a is safe to re-run.
#
# Translocation is a LaunchServices behaviour, not an execve one, so testing only the
# route rwpm actually uses would produce a non-finding that reads as a finding. Both
# routes are run against the SAME quarantined bundle and the running process path is
# recorded for each:
#   route a  rwpm run … -- <build>/RimWorldMac.app     (execve on the inner Mach-O, V41)
#   route b  open <build>/RimWorldMac.app              (LaunchServices)
# Only route b can produce a /private/var/folders/… path.
set -uo pipefail

build=${1:?build dir}
profile=${2:?profile id}
app=$build/RimWorldMac.app
rwpm=${RWPM_SPIKE_RWPM:-"$HOME/Library/Application Support/RimWorld Profile Manager/bin/rwpm"}
exe_name=$(sed -nE '/<key>CFBundleExecutable<\/key>/{n;s/.*<string>(.*)<\/string>.*/\1/p;}' \
  "$app/Contents/Info.plist")

proc_path() {
  local pid
  for _ in $(seq 1 24); do
    pid=$(pgrep -x "$exe_name" | head -n1)
    if [ -n "$pid" ]; then
      lsof -p "$pid" -Fn 2>/dev/null | sed -n 's/^n//p' | rg -m1 "MacOS/$exe_name"
      return 0
    fi
    sleep 5
  done
  echo "<no process started>"
}

stop_game() {
  pkill -x "$exe_name" 2>/dev/null
  sleep 3
}

echo "== tagging $app with com.apple.quarantine"
xattr -w com.apple.quarantine "0081;00000000;Safari;" "$app"
xattr -p com.apple.quarantine "$app" || echo "WARN: tag did not stick"

echo
echo "== route a: rwpm direct exec (execve, no LaunchServices)"
"$rwpm" run "$profile" -- "$app" >"$HOME/rwpm-spike/transloc-a.out" 2>&1 &
a_path=$(proc_path)
echo "route a process: $a_path"
case "$a_path" in
/private/var/folders/*) echo "route a: TRANSLOCATED" ;;
"$build"/*) echo "route a: not translocated (ran in place)" ;;
*) echo "route a: INCONCLUSIVE" ;;
esac
stop_game

echo
echo "== route b: open(1) via LaunchServices"
open "$app" 2>>"$HOME/rwpm-spike/transloc-b.out"
b_path=$(proc_path)
echo "route b process: $b_path"
case "$b_path" in
/private/var/folders/*) echo "route b: TRANSLOCATED" ;;
"$build"/*) echo "route b: not translocated (ran in place)" ;;
*) echo "route b: INCONCLUSIVE" ;;
esac
stop_game

echo
echo "== clearing quarantine"
xattr -d com.apple.quarantine "$app" 2>/dev/null || true
echo "done. route a = $a_path"
echo "      route b = $b_path"
