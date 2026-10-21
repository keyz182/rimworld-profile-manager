#!/usr/bin/env bash
# Spike: log appmanifest state changes while Steam switches branch.
# Usage: watch-acf.sh <appmanifest_294100.acf> <log file>   (runs until killed)
#
# Same as the Linux script except BSD date has no -Is.
set -uo pipefail

acf=${1:?acf}
out=${2:?log file}
last=""
while :; do
  now=$(sed -nE 's/^[[:space:]]*"(StateFlags|buildid|BetaKey|TargetBuildID|BytesToDownload|BytesDownloaded)"[[:space:]]*"([^"]*)".*/\1=\2/p' "$acf" 2>/dev/null | tr '\n' ' ')
  if [ "$now" != "$last" ]; then
    echo "$(date +%Y-%m-%dT%H:%M:%S%z) $now" >>"$out"
    last=$now
  fi
  sleep 5
done
