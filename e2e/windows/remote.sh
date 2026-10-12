#!/usr/bin/env bash
# T18: Windows e2e on the VM, driven from Linux. Packages the Windows bundle, ships it with
# the probe mod and scripts, runs e2e.ps1, fetches logs, then always cleans up.
# usage: remote.sh <scenario|all>
# exit: e2e.ps1's code (0 pass, 1 fail, 2 inconclusive)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HOST="${RWPM_WIN_HOST:-keyz@192.168.1.8}"
SSH="$HOME/dotfiles/bin/ssh.sh"
OUT="$ROOT/target/e2e-win"
STAGE="$OUT/stage"
REMOTE='C:\rwpm-e2e'

log() { printf '[remote] %s\n' "$*" >&2; }

remote_ps() {
  "$SSH" "$HOST" "powershell -NoProfile -ExecutionPolicy Bypass -File $REMOTE\\e2e.ps1 $*"
}

stage() {
  local zip
  zip="$("$ROOT/scripts/package.sh" windows | tail -1)"
  dotnet build "$ROOT/spike/linux/probe/Source" -c Release -o "$OUT/probe-build" -nologo >"$OUT/build.log" 2>&1 || {
    tail -20 "$OUT/build.log" >&2
    exit 1
  }
  [[ ! -e "$STAGE" ]] || trash "$STAGE"
  mkdir -p "$STAGE/probe/Assemblies"
  unzip -q "$zip" -d "$STAGE"
  mv "$STAGE"/rwpm-*-windows-x86_64 "$STAGE/app"
  cp -r "$ROOT/spike/linux/probe/About" "$STAGE/probe/"
  cp "$OUT/probe-build/Rwpm.SpikeProbe.dll" "$STAGE/probe/Assemblies/"
  cp "$ROOT/e2e/windows/"{e2e,launch,scenarios,guicheck,clicker}.ps1 "$STAGE/"
}

# Fail unless the shipped scripts and rwpm.exe on the VM match the staged copies.
verify_deploy() {
  local f want got
  for f in e2e.ps1 launch.ps1 scenarios.ps1 guicheck.ps1 clicker.ps1 app/rwpm.exe app/doorstop/winhttp.dll; do
    want="$(sha256sum "$STAGE/$f" | cut -d' ' -f1)"
    got="$("$SSH" "$HOST" "powershell -NoProfile -Command \"(Get-FileHash '$REMOTE\\${f//\//\\}').Hash.ToLower()\"" | tr -d '\r' | tail -1)"
    [[ "$want" == "$got" ]] || {
      log "deploy mismatch for $f ($got)"
      exit 1
    }
  done
}

# shellcheck disable=SC2329 # reached via finish (trap)
fetch_logs() {
  mkdir -p "$OUT/logs"
  "$SSH" "$HOST" "tar -cf - -C \"%LOCALAPPDATA%/RimWorld Profile Manager\" profiles -C C:/rwpm-e2e runs" 2>/dev/null |
    tar -xf - -C "$OUT/logs" 2>/dev/null || log "WARN could not fetch logs"
}

# shellcheck disable=SC2329 # reached via `trap finish EXIT`
finish() {
  fetch_logs
  remote_ps -Scenario cleanup || log "WARN remote cleanup failed; run: e2e.ps1 -Scenario cleanup on the VM"
}

main() {
  local scenario="${1:?usage: remote.sh <scenario|all>}"
  mkdir -p "$OUT"
  stage
  log "shipping to $HOST:$REMOTE"
  "$SSH" "$HOST" "(if exist C:\\rwpm-e2e rmdir /s /q C:\\rwpm-e2e) & mkdir C:\\rwpm-e2e"
  tar -cf - -C "$STAGE" . | "$SSH" "$HOST" "tar -xf - -C C:/rwpm-e2e"
  verify_deploy
  trap finish EXIT
  local code=0
  remote_ps -Scenario "$scenario" || code=$?
  log "result=$code"
  exit "$code"
}

main "$@"
