#!/usr/bin/env bash
# Drives the T3 Windows spike on the VM from Linux: build, ship, run, fetch logs, clean up.
# usage: remote.sh <scenario|all> [--allow-steam-folder-write]
# exit: run.ps1's code (0 pass, 1 fail, 2 inconclusive)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HOST="${RWPM_WIN_HOST:-keyz@192.168.1.8}"
SSH="$HOME/dotfiles/bin/ssh.sh"
BUILD="$ROOT/target/spike/build"
STAGE="$ROOT/target/spike-win/stage"
LOGS="$ROOT/target/spike-win"
VENDOR="$ROOT/vendor/unitydoorstop/4.5.0"

log() { printf '[remote] %s\n' "$*" >&2; }

build() {
  mkdir -p "$BUILD"
  local args
  for args in "payload/Rwpm.Payload -c Release -o $BUILD/payload-release" \
    "payload/Rwpm.Payload -c Spike -o $BUILD/payload-spike" \
    "spike/linux/probe/Source -c Release -o $BUILD/probe"; do
    # shellcheck disable=SC2086 # args is a fixed word list above
    (cd "$ROOT" && dotnet build $args -nologo >"$BUILD/build.log" 2>&1) || {
      tail -20 "$BUILD/build.log" >&2
      exit 1
    }
  done
}

stage() {
  [[ ! -e "$STAGE" ]] || trash "$STAGE"
  mkdir -p "$STAGE/vendor" "$STAGE/probe/Assemblies"
  (cd "$VENDOR" && sha256sum -c --quiet SHA256SUMS)
  cp "$VENDOR/win_x64/winhttp.dll" "$STAGE/vendor/"
  cp -r "$BUILD/payload-release" "$BUILD/payload-spike" "$STAGE/"
  cp -r "$ROOT/spike/linux/probe/About" "$STAGE/probe/"
  cp "$BUILD/probe/Rwpm.SpikeProbe.dll" "$STAGE/probe/Assemblies/"
  cp "$ROOT/spike/windows/"{run,launch,scenarios}.ps1 "$STAGE/"
}

# Fail unless every staged script on the VM matches the local copy byte for byte.
verify_deploy() {
  local f local_sum remote_sum
  for f in run.ps1 launch.ps1 scenarios.ps1 payload-release/Rwpm.Payload.dll; do
    local_sum="$(sha256sum "$STAGE/$f" | cut -d' ' -f1)"
    remote_sum="$("$SSH" "$HOST" "powershell -NoProfile -Command (Get-FileHash C:\\rwpm-spike\\${f//\//\\\\}).Hash" 2>/dev/null | tr -d '\r' | tr 'A-F' 'a-f' | tail -1)"
    [[ "$local_sum" == "$remote_sum" ]] || {
      log "deploy mismatch for $f (local $local_sum, remote $remote_sum)"
      exit 1
    }
  done
  log "deploy verified"
}

remote_ps() { "$SSH" "$HOST" "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\rwpm-spike\\run.ps1 $*"; }

# shellcheck disable=SC2329 # reached via `trap finish EXIT`
fetch_logs() {
  mkdir -p "$LOGS"
  "$SSH" "$HOST" "tar -cf - -C C:/rwpm-spike profiles" 2>/dev/null | tar -xf - -C "$LOGS" || log "no logs fetched"
}

# shellcheck disable=SC2329 # reached via `trap finish EXIT`
finish() {
  fetch_logs
  remote_ps -Scenario cleanup || log "WARN remote cleanup failed; run: run.ps1 -Scenario cleanup on the VM"
}

main() {
  local scenario="${1:?usage: remote.sh <scenario|all> [--allow-steam-folder-write]}" flag=""
  [[ "${2:-}" == "--allow-steam-folder-write" ]] && flag="-AllowSteamFolderWrite"
  build
  stage
  log "shipping to $HOST:C:/rwpm-spike"
  # cmd: `if X A && B` puts B inside the if, so parenthesise the mkdir.
  tar -cf - -C "$STAGE" . | "$SSH" "$HOST" "(if not exist C:\\rwpm-spike mkdir C:\\rwpm-spike) && tar -xf - -C C:/rwpm-spike"
  verify_deploy
  trap finish EXIT
  local code=0
  remote_ps -Scenario "$scenario" $flag || code=$?
  log "result=$code"
  exit "$code"
}

main "$@"
