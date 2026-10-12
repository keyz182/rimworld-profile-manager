#!/usr/bin/env bash
# T50 spike: Steam launch-option stand-in for the planned `rwpm run` target swap.
# Steam launch options: RWPM_SPIKE_BUILD=<build dir> RWPM_SPIKE_PROFILE=<id> <this> %command%
# Replaces Steam's start_RimWorld.sh token with the build's, keeps every wrapper before
# it, then hands the whole line to `rwpm run` as Steam would.
set -euo pipefail

build=${RWPM_SPIKE_BUILD:?set RWPM_SPIKE_BUILD}
profile=${RWPM_SPIKE_PROFILE:?set RWPM_SPIKE_PROFILE}
rwpm=${RWPM_SPIKE_RWPM:-"$HOME/.local/share/RimWorld Profile Manager/bin/rwpm"}
log=${RWPM_SPIKE_LOG:-"$HOME/rwpm-spike/swap.log"}

args=()
swapped=0
for a in "$@"; do
  if [ "$swapped" = 0 ] && [ "$(basename -- "$a")" = start_RimWorld.sh ]; then
    args+=("$build/start_RimWorld.sh")
    swapped=1
  else
    args+=("$a")
  fi
done

{
  printf '%s in :' "$(date -Is)"
  printf ' [%s]' "$@"
  printf '\n%s out:' "$(date -Is)"
  printf ' [%s]' "${args[@]}"
  printf '\nswapped=%s\n' "$swapped"
} >>"$log"

if [ "$swapped" = 0 ]; then
  echo "swap.sh: no start_RimWorld.sh in command" >>"$log"
  exit 2
fi
exec "$rwpm" run "$profile" -- "${args[@]}"
