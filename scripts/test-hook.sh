#!/usr/bin/env bash
# Checks the Waylith startup-hook payload against stand-in game assemblies
# (payload/tests). Covers the mods redirect, the Mods dir being created, the log dir,
# helpers and LIVE runs left untouched, and failing closed. Needs the .NET 8 and 10
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

# run <offline|livelog|none> <app dll>: fresh profile dir $p. offline: RWPM_MODS_DIR=$p/Mods
# + RWPM_LOG_DIR=$p (OFFLINE profile); livelog: RWPM_LOG_DIR=$p only (LIVE profile); none:
# neither (Default, or a game started without rwpm).
# Sets $out (stdout), $code, $log.
run() {
  p="$(mktemp -d "$OUT/runs/XXXXXX")"
  log="$p/payload.log"
  local env=(DOTNET_STARTUP_HOOKS="$HOOK" RWPM_PAYLOAD_LOG="$log")
  [[ "$1" != offline ]] || env+=(RWPM_MODS_DIR="$p/Mods")
  [[ "$1" == none ]] || env+=(RWPM_LOG_DIR="$p")
  set +e
  out="$(env -u RWPM_MODS_DIR -u RWPM_LOG_DIR "${env[@]}" dotnet "$2" 2>"$p/stderr")"
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

start mods_redirect
run offline "$OUT/game/RimWorld.dll"
[[ $code -eq 0 ]] || fail "exit $code: $(cat "$p/stderr")"
has "$out" "mods=$p/Mods" || fail "game saw: $out"
[[ -d "$p/Mods" ]] || fail "Mods dir not created"
grep -qE "patched Assembly-CSharp#[0-9]+ -> $p/Mods\$" "$log" || fail "no patched line in log"
done_case

start log_dir # same run as the redirect
has "$out" "logdir=$p" || fail "game log dir: $out"
grep -qF "log directory -> $p" "$log" || fail "no log directory line"
done_case

start helper_untouched
run offline "$OUT/helper/FakeHelper.dll"
[[ $code -eq 0 ]] || fail "exit $code: $(cat "$p/stderr")"
has "$out" "helper ok" || fail "helper output: $out"
grep -qF "not RimWorld; nothing to do" "$log" || fail "no guard line in log"
! grep -qF "patched" "$log" || fail "helper was patched"
[[ ! -e "$p/Mods" ]] || fail "helper created a Mods dir"
done_case

start live_log_only
run livelog "$OUT/game/RimWorld.dll"
[[ $code -eq 0 ]] || fail "exit $code: $(cat "$p/stderr")"
has "$out" "logdir=$p" || fail "LIVE log not moved: $out"
has "$out" "mods=$OUT/game/Mods" || fail "LIVE run redirected mods: $out"
[[ ! -e "$p/Mods" ]] || fail "LIVE run created a Mods dir"
! grep -qF "patched" "$log" || fail "LIVE run patched GenFilePaths"
done_case

start no_env_noop
run none "$OUT/game/RimWorld.dll"
[[ $code -eq 0 ]] || fail "exit $code: $(cat "$p/stderr")"
has "$out" "mods=$OUT/game/Mods" || fail "run redirected mods: $out"
has "$out" "logdir=$OUT/game/default-logs" || fail "run moved the log: $out"
grep -qF "no RWPM_MODS_DIR or RWPM_LOG_DIR; nothing to do" "$log" || fail "no no-op line in log"
done_case

start fail_closed
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
