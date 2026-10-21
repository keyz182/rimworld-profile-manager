#!/usr/bin/env bash
# Spike: log appmanifest state changes while Steam switches branch.
# Usage: watch-acf.sh <appmanifest_294100.acf> <log file>   (runs until killed)
set -uo pipefail

acf=${1:?acf}
out=${2:?log file}
last=""
while :; do
  now=$(sed -nE 's/^\s*"(StateFlags|buildid|BetaKey|TargetBuildID|BytesToDownload|BytesDownloaded)"\s*"([^"]*)".*/\1=\2/p' "$acf" 2>/dev/null | tr '\n' ' ')
  if [ "$now" != "$last" ]; then
    echo "$(date -Is) $now" >>"$out"
    last=$now
  fi
  sleep 5
done
