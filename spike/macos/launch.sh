#!/usr/bin/env bash
# Spike launch recipe for an OFFLINE profile on macOS (T37). Linux analogue: spike/linux
# (removed at v1.0.0, kept in git history) used LD_PRELOAD/LD_LIBRARY_PATH; this uses
# DYLD_INSERT_LIBRARIES/DYLD_LIBRARY_PATH (R26) and never routes through a shell (V41) —
# we exec the bundle's own Mach-O binary directly, not RimWorldMac.app or any wrapper.
# usage: launch.sh <abs-profile-dir> -- <RimWorldMac.app path>
# needs: RWPM_PAYLOAD_DLL, RWPM_DOORSTOP_DIR (dir holding libdoorstop.dylib)
# spike-only controls: SPIKE_KEEP_STALE=1 (skip V3 unset), SPIKE_NO_APPID=1 (skip V24/R10),
#                      SPIKE_MODS_DIR=<path> (override payload mods dir),
#                      SPIKE_ARG0=<path> (game argv[0]; V28/V48 restart-trampoline check)
set -euo pipefail

profile="$1"
shift
[[ "${1:-}" == "--" ]] && shift
[[ "$profile" == /* ]] || {
  echo "launch.sh: profile path must be absolute" >&2
  exit 64
}
bundle="$1"
shift

# V3: a stale guard from a parent Doorstop process would make ours skip injection.
if [[ "${SPIKE_KEEP_STALE:-0}" != "1" ]]; then
  unset DOORSTOP_INITIALIZED DOORSTOP_DISABLE
fi

# V24/R10: Steam app id via env, independent of steam_appid.txt (R17).
if [[ "${SPIKE_NO_APPID:-0}" != "1" ]]; then
  export SteamAppId=294100
fi

export DOORSTOP_ENABLED=1
export DOORSTOP_TARGET_ASSEMBLY="$RWPM_PAYLOAD_DLL"

# V40: prepend, never replace (R26 nix contract, V18-style). Absolute lib path per the
# plan's "use absolute paths in DYLD_INSERT_LIBRARIES" decision — sidesteps dyld4 leaf-name
# resolution questions the spike would otherwise have to pin down separately.
dylib="$RWPM_DOORSTOP_DIR/libdoorstop.dylib"
export DYLD_LIBRARY_PATH="${RWPM_DOORSTOP_DIR}${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
export DYLD_INSERT_LIBRARIES="${dylib}${DYLD_INSERT_LIBRARIES:+:$DYLD_INSERT_LIBRARIES}"

export RWPM_MODS_DIR="${SPIKE_MODS_DIR:-$profile/Mods}"
export RWPM_PAYLOAD_LOG="$profile/payload.log"

# V41: resolve the actual Mach-O inside the bundle and exec it directly — bash's own
# `exec` replaces this process image in place, so there is no intermediate shell for SIP
# to purge DYLD_* from (R27). CFBundleExecutable's value is read once via PlistBuddy so
# this works even if Ludeon ever renames the binary.
exe_name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$bundle/Contents/Info.plist")"
exe="$bundle/Contents/MacOS/$exe_name"

# V48 experiment: SPIKE_ARG0=<path> runs the game with a different argv[0] (does Unity
# start with a foreign argv[0], and does the game's own Restart see it — R11 is
# Linux-observed only; this spike answers it for macOS too).
if [[ -n "${SPIKE_ARG0:-}" ]]; then
  exec -a "$SPIKE_ARG0" "$exe" "-savedatafolder=$profile/SaveData" -logFile "$profile/Player.log"
fi
exec "$exe" "-savedatafolder=$profile/SaveData" -logFile "$profile/Player.log"
