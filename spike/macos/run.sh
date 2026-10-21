#!/usr/bin/env bash
# shellcheck disable=SC2015 # pass() always returns 0, so `cond && pass || fail` is a safe if/else.
# macOS e2e harness (spike). Mirrors spike/linux (removed at tag v1.0.0, kept in
# git history). Launches RimWorld via spike/macos/launch.sh and checks behaviour on
# this Mac, against the real Steam install and the real workshop mod set.
# usage: run.sh <scenario|all>
# exit: 0 pass, 1 fail, 2 inconclusive
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GAME="${RWPM_GAME_DIR:-$HOME/Library/Application Support/Steam/steamapps/common/RimWorld}"
WORKSHOP="${RWPM_WORKSHOP_DIR:-$HOME/Library/Application Support/Steam/steamapps/workshop/content/294100}"
BUNDLE="$GAME/RimWorldMac.app"
GAME_SAVE_DIR="$HOME/Library/Application Support/RimWorld"
# Correction (found live by sc_live_baseline): Unity's macOS default Player.log location
# is ~/Library/Logs/<Company>/<Product>/, NOT under the save dir and NOT Application Support.
GAME_LOG_DIR="$HOME/Library/Logs/Ludeon Studios/RimWorld by Ludeon Studios"
OUT="$ROOT/target/spike-mac"
BUILD="$OUT/build"
PROFILES="$OUT/profiles"
DOORSTOP_DIR="$ROOT/vendor/unitydoorstop/4.5.0/macos"
APPID_FILE="$BUNDLE/Contents/MacOS/steam_appid.txt"
APPID_MARKER="$OUT/.appid-renamed"
HARMONY_WS=2009463077
PREPATCHER_WS=2934420800
DLCS=(ludeon.rimworld.royalty ludeon.rimworld.ideology ludeon.rimworld.biotech ludeon.rimworld.anomaly ludeon.rimworld.odyssey)
RIMWORLD_MANAGED="$BUNDLE/Contents/Resources/Data/Managed"

RESULT=0
log() { printf '[spike-mac] %s\n' "$*" >&2; }
pass() { log "PASS $*"; }
fail() {
  log "FAIL $*"
  RESULT=1
}
inconclusive() {
  log "INCONCLUSIVE $*"
  [[ $RESULT -eq 1 ]] || RESULT=2
}

# Always undo an interrupted appid rename first, whatever scenario runs.
restore_appid() {
  if [[ -f "$APPID_MARKER" ]]; then
    if [[ -f "$APPID_FILE.rwpm-spike" && ! -f "$APPID_FILE" ]]; then
      mv "$APPID_FILE.rwpm-spike" "$APPID_FILE"
      log "restored $APPID_FILE"
    fi
    rm -f "$APPID_MARKER"
  fi
}

game_procs() { pgrep -f -- "${1:-RimWorld by Ludeon Studios}" || true; }

dotnet_build() {
  dotnet build "$@" -nologo >"$BUILD/build.log" 2>&1 || {
    tail -40 "$BUILD/build.log" >&2
    log "build failed: $1"
    exit 1
  }
}

build() {
  log "building payloads + probe (RimWorldManaged=$RIMWORLD_MANAGED)"
  mkdir -p "$BUILD"
  dotnet_build "$ROOT/payload/Rwpm.Payload" -c Release -o "$BUILD/payload-release"
  dotnet_build "$ROOT/e2e/probe/Source" -c Release -o "$BUILD/probe" -p:RimWorldManaged="$RIMWORLD_MANAGED"
  # C4 analogue: release payload must not contain spike switch code (.NET literals are UTF-16).
  python3 -I -c 'import sys; d=open(sys.argv[1],"rb").read(); n="RWPM_SPIKE_".encode("utf-16-le"); sys.exit(1 if n in d else 0)' \
    "$BUILD/payload-release/Rwpm.Payload.dll" || {
    log "release payload contains RWPM_SPIKE_ switches"
    exit 1
  }
}

# new_profile <name> <with-probe 0|1> <active packageId…> — an OFFLINE store profile.
new_profile() {
  local name="$1" probe="$2"
  shift 2
  local p="$PROFILES/${name//_/-}"
  [[ ! -e "$p" ]] || trash "$p"
  mkdir -p "$p/SaveData/Config"
  printf 'schema = 1\nname = "%s"\nmode = "offline"\n' "$name" >"$p/profile.toml"
  if [[ "$probe" == "1" ]]; then
    mkdir -p "$p/Mods/rwpm.spike.probe/Assemblies"
    cp -r "$ROOT/e2e/probe/About" "$p/Mods/rwpm.spike.probe/"
    cp "$BUILD/probe/Rwpm.SpikeProbe.dll" "$p/Mods/rwpm.spike.probe/Assemblies/"
  fi
  write_modsconfig "$p/SaveData/Config/ModsConfig.xml" "$@"
  printf '%s\n' "$p"
}

write_modsconfig() {
  local file="$1"
  shift
  local version
  version="$(tr -d '\r\n' <"$BUNDLE/Version.txt")"
  {
    printf '<?xml version="1.0" encoding="utf-8"?>\n<ModsConfigData>\n  <version>%s</version>\n  <activeMods>\n' "$version"
    printf '    <li>%s</li>\n' "$@"
    printf '  </activeMods>\n  <knownExpansions>\n'
    printf '    <li>%s</li>\n' "${DLCS[@]}"
    printf '  </knownExpansions>\n</ModsConfigData>\n'
  } >"$file"
}

# launch <profile> <payload flavour> [VAR=val…] — backgrounds the game via spike/macos/launch.sh.
# Caller-supplied VAR=val entries are given to `env` AFTER our defaults, so they override
# (e.g. a scenario overriding RWPM_DOORSTOP_DIR for a tampered dylib copy).
launch() {
  local p="$1" flavour="$2"
  shift 2
  env RWPM_PAYLOAD_DLL="$BUILD/payload-$flavour/Rwpm.Payload.dll" RWPM_DOORSTOP_DIR="$DOORSTOP_DIR" \
    "$@" RWPM_SPIKE_DIR="$p" \
    "$ROOT/spike/macos/launch.sh" "$p" -- "$BUNDLE" >"$p/launch.out" 2>&1 &
  LAUNCH_PID=$!
}

launch_alive() {
  local st
  st="$(ps -o stat= -p "${LAUNCH_PID:-0}" 2>/dev/null || true)"
  [[ -n "$st" && "$st" != Z* ]]
}

# wait_gone <profile> <timeout s> → 0 if every process for the profile exited.
wait_gone() {
  local p="$1" deadline=$((SECONDS + $2)) alive procs
  while :; do
    alive=0
    launch_alive && alive=1
    procs="$(game_procs "savedatafolder=$p/SaveData")"
    ((alive == 1)) || [[ -n "$procs" ]] || return 0
    ((SECONDS < deadline)) || return 1
    sleep 2
  done
}

kill_profile() { pkill -f -- "savedatafolder=$1/SaveData" || true; }

probe_lines() { [[ -f "$1/probe.log" ]] && wc -l <"$1/probe.log" | tr -d ' ' || echo 0; }
probe_field() { awk -F'\t' -v k="$2" -v n="$3" 'NR==n{for(i=1;i<=NF;i++) if(index($i,k"=")==1) print substr($i,length(k)+2)}' "$1/probe.log"; }

# shellcheck source=spike/macos/scenarios.sh
source "$ROOT/spike/macos/scenarios.sh"

main() {
  local scenario="${1:?usage: run.sh <scenario|all>}"
  mkdir -p "$OUT" "$PROFILES"
  restore_appid
  [[ -z "$(game_procs)" ]] || {
    log "RimWorld already running; close it first"
    exit 1
  }
  [[ -d "$BUNDLE" ]] || {
    log "RimWorldMac.app not found at $BUNDLE"
    exit 1
  }
  [[ -f "$DOORSTOP_DIR/libdoorstop.dylib" ]] || {
    log "vendored macOS doorstop dylib missing at $DOORSTOP_DIR"
    exit 1
  }
  build
  if [[ "$scenario" == "all" ]]; then
    for s in live_baseline basic quarantine quarantine_control adhoc_sign restart prepatcher appid shellhook payload_fail steam_cmdline; do
      log "=== $s"
      "sc_$s"
    done
  else
    "sc_${scenario//-/_}"
  fi
  log "result=$RESULT"
  exit "$RESULT"
}

main "$@"
