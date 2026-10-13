#!/usr/bin/env bash
# T52 spike: Steam launch-option stand-in for the planned `rwpm run` target swap (macOS).
# Steam launch options:
#   RWPM_SPIKE_BUILD=<build dir> RWPM_SPIKE_PROFILE=<id> <this> %command%
#
# macOS %command% has no wrapper chain and no start_RimWorld.sh. R33: it is a literal argv
# list naming the .app BUNDLE DIRECTORY, e.g.
#   /…/common/RimWorld/RimWorldMac.app -disable-compute-shaders
# so the swap replaces that bundle token and hands the whole line to `rwpm run`, which
# then resolves CFBundleExecutable and execs the inner Mach-O (V41/V49, launch.rs).
#
# MATCH RULE (settled for T52; the Linux script has only rule 1's equivalent):
#   rule 1  first token whose basename is RimWorldMac.app      -> <build>/RimWorldMac.app
#   rule 2  else first token lying under the Steam install dir -> same path under <build>
#   else    exit 2
# Rule 2 exists because the spike may put Steam on a branch whose target is not a .app at
# all: R37 shows RimWorld 1.7 is Waylith with a bare native exe. Without it the script
# exits 2 and the V51 leg reads as a tooling bug rather than a rule gap. The rule that
# fired is logged, so a rule-2 swap is never mistaken for a rule-1 one.
set -euo pipefail

build=${RWPM_SPIKE_BUILD:?set RWPM_SPIKE_BUILD}
profile=${RWPM_SPIKE_PROFILE:?set RWPM_SPIKE_PROFILE}
rwpm=${RWPM_SPIKE_RWPM:-"$HOME/Library/Application Support/RimWorld Profile Manager/bin/rwpm"}
log=${RWPM_SPIKE_LOG:-"$HOME/rwpm-spike/swap.log"}
# Where the Steam install lives, for rule 2. Recorded by capture.sh.
src=${RWPM_SPIKE_SRC:-$(sed -nE 's/^source_dir = "(.*)"$/\1/p' "$build/build.spike.toml" 2>/dev/null)}

mkdir -p "$(dirname "$log")"

args=()
swapped=0
rule=none
for a in "$@"; do
  if [ "$swapped" = 0 ] && [ "$(basename -- "$a")" = RimWorldMac.app ]; then
    args+=("$build/RimWorldMac.app")
    swapped=1
    rule=1
  elif [ "$swapped" = 0 ] && [ -n "$src" ] && [ "${a#"$src"/}" != "$a" ]; then
    args+=("$build/${a#"$src"/}")
    swapped=1
    rule=2
  else
    args+=("$a")
  fi
done

{
  printf '%s in :' "$(date +%Y-%m-%dT%H:%M:%S%z)"
  printf ' [%s]' "$@"
  printf '\n%s out:' "$(date +%Y-%m-%dT%H:%M:%S%z)"
  printf ' [%s]' "${args[@]}"
  printf '\nswapped=%s rule=%s\n' "$swapped" "$rule"
} >>"$log"

if [ "$swapped" = 0 ]; then
  echo "swap.sh: no bundle token and nothing under '$src' in the command" >>"$log"
  exit 2
fi
exec "$rwpm" run "$profile" -- "${args[@]}"
