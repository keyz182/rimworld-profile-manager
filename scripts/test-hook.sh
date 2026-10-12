#!/usr/bin/env bash
# T64: checks the Waylith startup-hook payload against stand-in game assemblies
# (payload/tests). Covers V64 (mods redirect), V16 (Mods dir created), V67 (log dir),
# V65 (helpers and LIVE runs untouched) and V26 (fail closed). Needs the .NET 8 and 10
# runtimes and an SDK that builds net6.0.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/target/test-hook"
mkdir -p "$OUT/runs"

build() { dotnet build "$1" -c Release -o "$2" -nologo -v q "${@:3}" >&2; }

echo "[test-hook] building" >&2
build "$ROOT/payload/Rwpm.Payload.Hook" "$OUT/hook"
build "$ROOT/payload/tests/FakeGame" "$OUT/game"
build "$ROOT/payload/tests/FakeHelper" "$OUT/helper"
# A game whose Assembly-CSharp has no modsFolderPath field.
build "$ROOT/payload/tests/FakeAcs" "$OUT/acs-nofield" -p:DefineConstants=NO_FIELD
mkdir -p "$OUT/game-nofield"
cp -R "$OUT/game/." "$OUT/game-nofield/"
cp "$OUT/acs-nofield/Assembly-CSharp.dll" "$OUT/game-nofield/"

HOOK="$OUT/hook/Rwpm.Payload.Hook.dll"
failed=0

case=""
case_failed=0
fail() {
  echo "FAIL $case: $*" >&2
  case_failed=1
  failed=1
}

# run <offline|live> <app dll>: fresh profile dir $p; OFFLINE sets RWPM_MODS_DIR=$p/Mods.
# Sets $out (stdout), $code, $log.
run() {
  p="$(mktemp -d "$OUT/runs/XXXXXX")"
  log="$p/payload.log"
  local env=(DOTNET_STARTUP_HOOKS="$HOOK" RWPM_PAYLOAD_LOG="$log")
  [[ "$1" == live ]] || env+=(RWPM_MODS_DIR="$p/Mods")
  set +e
  out="$(env -u RWPM_MODS_DIR "${env[@]}" dotnet "$2" 2>"$p/stderr")"
  code=$?
  set -e
  touch "$log"
}

has() { grep -qF -- "$2" <<<"$1"; }
start() {
  case="$1"
  case_failed=0
}
done_case() { [[ $case_failed -eq 1 ]] || echo "PASS $case"; }

start V64_redirect
run offline "$OUT/game/RimWorld.dll"
[[ $code -eq 0 ]] || fail "exit $code: $(cat "$p/stderr")"
has "$out" "mods=$p/Mods" || fail "game saw: $out"
[[ -d "$p/Mods" ]] || fail "Mods dir not created (V16)"
grep -qE "patched Assembly-CSharp#[0-9]+ -> $p/Mods\$" "$log" || fail "no patched line in log"
done_case

start V67_log_dir # same run as V64
has "$out" "logdir=$p" || fail "game log dir: $out"
grep -qF "log directory -> $p" "$log" || fail "no log directory line"
done_case

start V65_helper
run offline "$OUT/helper/FakeHelper.dll"
[[ $code -eq 0 ]] || fail "exit $code: $(cat "$p/stderr")"
has "$out" "helper ok" || fail "helper output: $out"
grep -qF "not RimWorld; nothing to do" "$log" || fail "no guard line in log"
! grep -qF "patched" "$log" || fail "helper was patched"
[[ ! -e "$p/Mods" ]] || fail "helper created a Mods dir"
done_case

start V65_live_noop
run live "$OUT/game/RimWorld.dll"
[[ $code -eq 0 ]] || fail "exit $code: $(cat "$p/stderr")"
has "$out" "mods=$OUT/game/Mods" || fail "LIVE run redirected: $out"
has "$out" "logdir=$OUT/game/default-logs" || fail "LIVE run moved the log: $out"
grep -qF "no RWPM_MODS_DIR; nothing to do" "$log" || fail "no LIVE line in log"
done_case

start V26_fail_closed
run offline "$OUT/game-nofield/RimWorld.dll"
[[ $code -ne 0 ]] || fail "game started without the redirect"
! has "$out" "mods=" || fail "game reached Main: $out"
grep -qF "FATAL patch failed" "$log" || fail "no FATAL line in log"
done_case

if [[ $failed -ne 0 ]]; then
  echo "[test-hook] FAILED" >&2
  exit 1
fi
echo "[test-hook] all passed" >&2
