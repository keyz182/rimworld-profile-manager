#!/usr/bin/env bash
# shellcheck disable=SC2015 # pass() always returns 0, so `cond && pass || fail` is a safe if/else.
# Linux e2e harness (T2 spike, retargeted at `rwpm run` in T17). Launches RimWorld per
# scenario and checks §V behaviour. Main scenarios go through a staged copy of the real
# `rwpm run` with OFFLINE store profiles; launch.sh-only controls keep the spike recipe.
# usage: run.sh <scenario|all> [--allow-steam-folder-write]
# exit: 0 pass, 1 fail, 2 inconclusive
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GAME="${RWPM_GAME_DIR:-$HOME/.local/share/Steam/steamapps/common/RimWorld}"
WORKSHOP="${RWPM_WORKSHOP_DIR:-$HOME/.local/share/Steam/steamapps/workshop/content/294100}"
OVERLAY64="$HOME/.local/share/Steam/ubuntu12_64/gameoverlayrenderer.so"
OUT="$ROOT/target/spike"
BUILD="$OUT/build"
# Isolated store: rwpm finds profiles and run.lock under XDG_DATA_HOME (paths.rs).
XDG_DATA="$OUT/xdg-data"
PROFILES="$XDG_DATA/RimWorld Profile Manager/profiles"
APPID_FILE="$GAME/steam_appid.txt"
APPID_MARKER="$OUT/.appid-renamed"
HARMONY_WS=2009463077
PREPATCHER_WS=2934420800
DLCS=(ludeon.rimworld.royalty ludeon.rimworld.ideology ludeon.rimworld.biotech ludeon.rimworld.anomaly ludeon.rimworld.odyssey)

RESULT=0
log() { printf '[spike] %s\n' "$*" >&2; }
pass() { log "PASS $*"; }
fail() {
  log "FAIL $*"
  RESULT=1
}
inconclusive() {
  log "INCONCLUSIVE $*"
  [[ $RESULT -eq 1 ]] || RESULT=2
}

# C2: always undo an interrupted V24 rename first, whatever scenario runs.
restore_appid() {
  if [[ -f "$APPID_MARKER" ]]; then
    if [[ -f "$APPID_FILE.rwpm-spike" && ! -f "$APPID_FILE" ]]; then
      mv "$APPID_FILE.rwpm-spike" "$APPID_FILE"
      log "restored $APPID_FILE"
    fi
    rm -f "$APPID_MARKER"
  fi
}

game_procs() { pgrep -f -- "${1:-RimWorldLinux}" || true; }

dotnet_build() {
  dotnet build "$@" -nologo >"$BUILD/build.log" 2>&1 || {
    tail -20 "$BUILD/build.log" >&2
    log "build failed: $1"
    exit 1
  }
}

build() {
  log "building payloads + probe"
  mkdir -p "$BUILD"
  dotnet_build "$ROOT/payload/Rwpm.Payload" -c Release -o "$BUILD/payload-release"
  dotnet_build "$ROOT/payload/Rwpm.Payload" -c Spike -o "$BUILD/payload-spike"
  dotnet_build "$ROOT/spike/linux/probe/Source" -c Release -o "$BUILD/probe"
  dotnet_build "$ROOT/spike/linux/chain" -c Release -o "$BUILD/chain"
  cargo build -q -p rwpm --manifest-path "$ROOT/Cargo.toml" || {
    log "build failed: rwpm"
    exit 1
  }
  # C4: release payload must not contain spike switch code (.NET literals are UTF-16).
  python3 -I -c 'import sys; d=open(sys.argv[1],"rb").read(); n="RWPM_SPIKE_".encode("utf-16-le"); sys.exit(1 if n in d else 0)' \
    "$BUILD/payload-release/Rwpm.Payload.dll" || {
    log "release payload contains RWPM_SPIKE_ switches"
    exit 1
  }
  python3 -I -c 'import sys; d=open(sys.argv[1],"rb").read(); n="RWPM_SPIKE_".encode("utf-16-le"); sys.exit(0 if n in d else 1)' \
    "$BUILD/payload-spike/Rwpm.Payload.dll" || {
    log "spike payload lacks RWPM_SPIKE_ switches"
    exit 1
  }
  stage_app release
  stage_app spike
}

# stage_app <flavour>: rwpm + payload/ + doorstop/ laid out as shipped (PayloadFiles::beside).
# RWPM_BUNDLE=<unpacked release bundle> makes the release app that bundle (T19 check).
stage_app() {
  local app="$OUT/app-$1"
  [[ ! -e "$app" ]] || trash "$app"
  if [[ "$1" == release && -n "${RWPM_BUNDLE:-}" ]]; then
    cp -r "$RWPM_BUNDLE" "$app"
    return
  fi
  mkdir -p "$app/payload" "$app/doorstop"
  cp "$ROOT/target/debug/rwpm" "$app/"
  cp "$BUILD/payload-$1/Rwpm.Payload.dll" "$app/payload/"
  cp "$ROOT/vendor/unitydoorstop/4.5.0/linux_x64/libdoorstop.so" "$app/doorstop/"
}

# new_profile <name> <with-probe 0|1> <active packageId…> — an OFFLINE store profile.
# The profile id is the name with `_` → `-` (ids allow only [a-z0-9-]).
new_profile() {
  local name="$1" probe="$2"
  shift 2
  local p="$PROFILES/${name//_/-}"
  [[ ! -e "$p" ]] || trash "$p"
  mkdir -p "$p/SaveData/Config"
  printf 'schema = 1\nname = "%s"\nmode = "offline"\n' "$name" >"$p/profile.toml"
  if [[ "$probe" == "1" ]]; then
    mkdir -p "$p/Mods/rwpm.spike.probe/Assemblies"
    cp -r "$ROOT/spike/linux/probe/About" "$p/Mods/rwpm.spike.probe/"
    cp "$BUILD/probe/Rwpm.SpikeProbe.dll" "$p/Mods/rwpm.spike.probe/Assemblies/"
  fi
  write_modsconfig "$p/SaveData/Config/ModsConfig.xml" "$@"
  printf '%s\n' "$p"
}

write_modsconfig() {
  local file="$1"
  shift
  local version
  version="$(tr -d '\r\n' <"$GAME/Version.txt")"
  {
    printf '<?xml version="1.0" encoding="utf-8"?>\n<ModsConfigData>\n  <version>%s</version>\n  <activeMods>\n' "$version"
    printf '    <li>%s</li>\n' "$@"
    printf '  </activeMods>\n  <knownExpansions>\n'
    printf '    <li>%s</li>\n' "${DLCS[@]}"
    printf '  </knownExpansions>\n</ModsConfigData>\n'
  } >"$file"
}

# launch <profile> <payload release|spike> [VAR=val…] — backgrounds the game, sets LAUNCH_PID.
# Default: `rwpm run <id> -- start_RimWorld.sh` from the staged app. A SPIKE_* var is a
# launch.sh-only switch (the product has none), so those controls use the spike recipe.
launch() {
  local p="$1" flavour="$2" arg
  shift 2
  LAUNCH_VIA=rwpm
  for arg in "$@"; do [[ "$arg" != SPIKE_* ]] || LAUNCH_VIA=script; done
  if [[ "$LAUNCH_VIA" == rwpm ]]; then
    env "$@" RWPM_SPIKE_DIR="$p" XDG_DATA_HOME="$XDG_DATA" \
      "$OUT/app-$flavour/rwpm" run "$(basename "$p")" -- "$GAME/start_RimWorld.sh" >"$p/launch.out" 2>&1 &
  else
    env "$@" RWPM_SPIKE_DIR="$p" RWPM_PAYLOAD_DLL="$BUILD/payload-$flavour/Rwpm.Payload.dll" \
      RWPM_DOORSTOP_DIR="$ROOT/vendor/unitydoorstop/4.5.0/linux_x64" \
      "$ROOT/spike/linux/launch.sh" "$p" -- "$GAME/start_RimWorld.sh" >"$p/launch.out" 2>&1 &
  fi
  LAUNCH_PID=$!
}

# launch_alive → 0 while LAUNCH_PID runs (zombies count as gone; they await `wait`).
launch_alive() {
  local st
  st="$(ps -o stat= -p "${LAUNCH_PID:-0}" 2>/dev/null || true)"
  [[ -n "$st" && "$st" != Z* ]]
}

# wait_gone <profile> <timeout s> → 0 if every process for the profile exited, 1 on timeout.
# The launched PID counts too: before launch.sh execs the game its cmdline lacks -savedatafolder.
# Sets LAUNCH_EARLY=1 if the launcher exited while a game process still ran (V17 for rwpm).
wait_gone() {
  local p="$1" deadline=$((SECONDS + $2)) alive procs
  LAUNCH_EARLY=0
  while :; do
    alive=0
    launch_alive && alive=1
    procs="$(game_procs "savedatafolder=$p/SaveData")"
    ((alive == 0)) && [[ -n "$procs" ]] && LAUNCH_EARLY=1
    ((alive == 1)) || [[ -n "$procs" ]] || return 0
    ((SECONDS < deadline)) || return 1
    sleep 2
  done
}

kill_profile() { pkill -f -- "savedatafolder=$1/SaveData" || true; }

probe_lines() { [[ -f "$1/probe.log" ]] && wc -l <"$1/probe.log" || echo 0; }
probe_field() { awk -F'\t' -v k="$2" -v n="$3" 'NR==n{for(i=1;i<=NF;i++) if(index($i,k"=")==1) print substr($i,length(k)+2)}' "$1/probe.log"; }

assert_redirected() {
  local p="$1" n="$2"
  [[ "$(probe_field "$p" mods "$n")" == "$p/Mods" ]] && pass "run $n mods=$p/Mods" || fail "run $n mods=$(probe_field "$p" mods "$n")"
  [[ "$(probe_field "$p" save "$n")" == "$p/SaveData" ]] && pass "run $n save dir" || fail "run $n save=$(probe_field "$p" save "$n")"
  if probe_field "$p" installed "$n" | tr ',' '\n' | grep -q "@$GAME/Mods/"; then
    fail "run $n loaded mods from install Mods"
  else
    pass "run $n no install-Mods mods"
  fi
}

# shellcheck source=spike/linux/scenarios.sh
source "$ROOT/spike/linux/scenarios.sh"

main() {
  local scenario="${1:?usage: run.sh <scenario|all> [--allow-steam-folder-write]}" allow="${2:-}"
  mkdir -p "$OUT"
  restore_appid
  [[ -z "$(game_procs)" ]] || {
    log "RimWorld already running; close it first"
    exit 1
  }
  build
  if [[ "$scenario" == "all" ]]; then
    for s in basic script_control restart restart_control stale_env stale_env_control no_mods_dir no_mods_dir_control \
      prepatcher prepatcher_control payload_fail guard guard_control \
      restart_trampoline_control openurl openurl_control chain chain_fail; do
      log "=== $s"
      "sc_$s"
    done
  elif [[ "$scenario" == "appid" ]]; then
    [[ "$allow" == "--allow-steam-folder-write" ]] || {
      log "appid needs --allow-steam-folder-write"
      exit 1
    }
    sc_appid
    sc_appid_control
  else
    "sc_${scenario//-/_}"
  fi
  log "result=$RESULT"
  exit "$RESULT"
}

main "$@"
