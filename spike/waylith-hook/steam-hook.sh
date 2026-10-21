#!/usr/bin/env bash
# Spike W4: Steam launch-option wrapper. Sets the startup-hook env for profile spike-17,
# keeps Steam's wrapper chain as given, and adds -savedatafolder so Default is never used.
set -euo pipefail
P="$HOME/.local/share/RimWorld Profile Manager/profiles/spike-17"
export DOTNET_STARTUP_HOOKS="$HOME/rwpm-spike/hook/Hook/Rwpm.Spike.Hook.dll"
export RWPM_MODS_DIR="$P/Mods" RWPM_LOG_DIR="$P" RWPM_PAYLOAD_LOG="$HOME/rwpm-spike/hook/w4.log"
{
  printf '%s argv:' "$(date -Is)"
  printf ' [%s]' "$@"
  echo
} >>"$HOME/rwpm-spike/hook/w4-argv.log"
exec "$@" "-savedatafolder=$P/SaveData"
