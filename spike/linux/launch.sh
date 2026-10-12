#!/usr/bin/env bash
# Spike launch recipe for an OFFLINE profile on Linux. T9 ports this to Rust.
# usage: launch.sh <abs-profile-dir> -- <game command…>   (e.g. start_RimWorld.sh)
# needs: RWPM_PAYLOAD_DLL, RWPM_DOORSTOP_DIR
# spike-only controls: SPIKE_KEEP_STALE=1 (skip V3 unset), SPIKE_NO_APPID=1 (skip V24),
#                      SPIKE_MODS_DIR=<path> (override payload mods dir),
#                      SPIKE_NO_DIRECT=1 (run start_RimWorld.sh under preload; B1 repro),
#                      SPIKE_ARG0=<path> (game argv[0]; ?U2 restart trampoline check),
#                      SPIKE_VIA_SCRIPT=1 (no effect; routes run.sh to this recipe)
set -euo pipefail

profile="$1"
shift
[[ "${1:-}" == "--" ]] && shift
[[ "$profile" == /* ]] || {
  echo "launch.sh: profile path must be absolute" >&2
  exit 64
}

# V3: a stale guard from a parent Doorstop process would make ours skip injection.
if [[ "${SPIKE_KEEP_STALE:-0}" != "1" ]]; then
  unset DOORSTOP_INITIALIZED DOORSTOP_DISABLE
fi

# V24: Steam app id via env, independent of steam_appid.txt (R17).
if [[ "${SPIKE_NO_APPID:-0}" != "1" ]]; then
  export SteamAppId=294100
fi

export DOORSTOP_ENABLED=1
export DOORSTOP_TARGET_ASSEMBLY="$RWPM_PAYLOAD_DLL"

# V18: prepend, never replace (Steam preloads its overlay). LD_PRELOAD splits on
# spaces and colons, so preload by name and resolve the dir via LD_LIBRARY_PATH.
export LD_LIBRARY_PATH="${RWPM_DOORSTOP_DIR}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LD_PRELOAD="libdoorstop.so${LD_PRELOAD:+:$LD_PRELOAD}"

export RWPM_MODS_DIR="${SPIKE_MODS_DIR:-$profile/Mods}"
export RWPM_PAYLOAD_LOG="$profile/payload.log"

# V25 / R19: Doorstop's dup2 hook breaks bash `$(…)`, so never preload into the
# game's start script. Run the ELF directly, replicating start_RimWorld.sh.
if [[ "$(basename "$1")" == "start_RimWorld.sh" && "${SPIKE_NO_DIRECT:-0}" != "1" ]]; then
  game_dir="$(dirname "$(readlink -f "$1")")"
  shift
  cd "$game_dir"
  export LC_ALL=C
  set -- ./RimWorldLinux "$@"
fi

# ?U2 experiment: SPIKE_ARG0=<path> runs the game with a different argv[0].
if [[ -n "${SPIKE_ARG0:-}" ]]; then
  exec -a "$SPIKE_ARG0" "$@" "-savedatafolder=$profile/SaveData" -logFile "$profile/Player.log"
fi
exec "$@" "-savedatafolder=$profile/SaveData" -logFile "$profile/Player.log"
