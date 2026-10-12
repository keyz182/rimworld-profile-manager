# shellcheck shell=bash
# shellcheck disable=SC2015 # pass() always returns 0, so `cond && pass || fail` is a safe if/else.
# T2 spike scenarios, sourced by run.sh. Each sc_* sets RESULT via pass/fail/inconclusive.
# *_control scenarios are fail-first controls: they PASS when the guarded behaviour breaks
# without the fix, proving the matching main scenario actually exercises it.

BASE_MODS=(ludeon.rimworld "${DLCS[@]}")

payload_starts() {
  [[ -f "$1/payload.log" ]] || {
    echo 0
    return
  }
  grep -c ' start pid=' "$1/payload.log" || true
}

# timed_run <profile> <flavour> <timeout> [VAR=val…] → sets TIMED_OUT=0|1 and LAUNCH_CODE
# (the launcher's exit code), kills on timeout.
timed_run() {
  local p="$1" flavour="$2" t="$3"
  shift 3
  launch "$p" "$flavour" "$@"
  TIMED_OUT=0
  if ! wait_gone "$p" "$t"; then
    TIMED_OUT=1
    kill_profile "$p"
    wait_gone "$p" 30 || log "WARN game for $p still alive after kill"
  fi
  LAUNCH_CODE=0
  wait "$LAUNCH_PID" || LAUNCH_CODE=$?
}

# assert_rwpm_waited: `rwpm run` stayed up until the last game process ended (V17) and
# returned the game's exit code 0 after a clean quit.
assert_rwpm_waited() {
  [[ "$LAUNCH_VIA" == rwpm ]] || {
    fail "$1: not launched via rwpm"
    return
  }
  ((LAUNCH_EARLY == 0)) && pass "$1: rwpm outlived every game process" || fail "$1: rwpm exited while the game ran"
  ((LAUNCH_CODE == 0)) && pass "$1: rwpm exit 0" || fail "$1: rwpm exit $LAUNCH_CODE ($(tail -1 "$2/launch.out"))"
}

# V25 control (B1 repro): preloading into start_RimWorld.sh breaks the script.
sc_script_control() {
  local p
  p="$(new_profile script_control 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 60 SPIKE_NO_DIRECT=1
  if [[ "$(payload_starts "$p")" -eq 0 ]] && grep -q 'RimWorldLinux: No such file' "$p/launch.out"; then
    pass "V25 control: preloaded start script never reaches the game"
  else
    fail "V25 control: starts=$(payload_starts "$p") (direct exec not shown necessary)"
  fi
}

sc_basic() {
  local p
  p="$(new_profile basic 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  local preload=()
  [[ -f "$OVERLAY64" ]] && preload=(LD_PRELOAD="$OVERLAY64")
  timed_run "$p" release 180 "${preload[@]}"
  ((TIMED_OUT == 0)) || {
    fail "basic: game did not quit"
    return
  }
  [[ "$(probe_lines "$p")" -eq 1 ]] || {
    fail "basic: probe lines=$(probe_lines "$p")"
    return
  }
  assert_redirected "$p" 1
  if ((${#preload[@]})); then
    [[ "$(probe_field "$p" LD_PRELOAD 1)" == "libdoorstop.so:$OVERLAY64" ]] &&
      pass "V18 LD_PRELOAD kept existing entry" || fail "V18 LD_PRELOAD=$(probe_field "$p" LD_PRELOAD 1)"
  else
    inconclusive "V18: no $OVERLAY64 to pre-load"
  fi
  [[ "$(probe_field "$p" SteamAppId 1)" == "294100" ]] && pass "SteamAppId exported" || fail "SteamAppId missing"
  [[ -s "$p/Player.log" ]] && pass "Player.log in profile" || fail "no profile Player.log"
  assert_rwpm_waited basic "$p"
}

sc_restart() {
  local p
  p="$(new_profile restart 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  touch "$p/restart.flag"
  timed_run "$p" release 300
  ((TIMED_OUT == 0)) || {
    fail "V1 restart: game did not quit (lines=$(probe_lines "$p"))"
    return
  }
  [[ "$(probe_lines "$p")" -eq 2 ]] || {
    fail "V1 restart: probe lines=$(probe_lines "$p")"
    return
  }
  [[ "$(probe_field "$p" pid 1)" != "$(probe_field "$p" pid 2)" ]] && pass "V1 two processes" || fail "V1 same pid twice"
  assert_redirected "$p" 1
  assert_redirected "$p" 2
  [[ "$(payload_starts "$p")" -eq 2 ]] && pass "V1 payload ran in restart child" || fail "V1 payload starts=$(payload_starts "$p")"
  [[ "$(probe_field "$p" arg0 2)" == "$OUT/app-release/rwpm" ]] &&
    pass "V28 restart went through the rwpm trampoline" || fail "V28 restart arg0=$(probe_field "$p" arg0 2)"
  assert_rwpm_waited restart "$p"
}

# V28 control: the payload drops DOORSTOP_ENABLED (RWPM_RESTART set) but the game is
# not started with argv[0] = rwpm, so the restart child runs without Doorstop.
sc_restart_trampoline_control() {
  local p
  p="$(new_profile restart_trampoline_control 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  touch "$p/restart.flag"
  timed_run "$p" release 150 SPIKE_VIA_SCRIPT=1 RWPM_RESTART=x
  if [[ "$(probe_lines "$p")" -eq 1 && "$(payload_starts "$p")" -eq 1 ]]; then
    pass "V28 control: without the trampoline, the restart child lost Doorstop"
  else
    fail "V28 control: lines=$(probe_lines "$p") starts=$(payload_starts "$p") (trampoline not shown necessary)"
  fi
}

sc_restart_control() {
  local p
  p="$(new_profile restart_control 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  touch "$p/restart.flag"
  # Script recipe: rwpm's trampoline re-adds Doorstop itself, so it would hide this.
  timed_run "$p" spike 150 RWPM_SPIKE_NO_CLEAR=1 SPIKE_VIA_SCRIPT=1
  if [[ "$(probe_lines "$p")" -eq 1 && "$(payload_starts "$p")" -eq 1 ]]; then
    pass "V1 control: without clearing, restart child skipped Doorstop"
  else
    fail "V1 control: lines=$(probe_lines "$p") starts=$(payload_starts "$p") (clearing not shown necessary)"
  fi
}

sc_stale_env() {
  local p
  p="$(new_profile stale_env 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 180 DOORSTOP_INITIALIZED=TRUE DOORSTOP_DISABLE=TRUE
  [[ "$(probe_lines "$p")" -eq 1 ]] || {
    fail "V3 stale env: probe lines=$(probe_lines "$p") timed_out=$TIMED_OUT"
    return
  }
  assert_redirected "$p" 1
}

sc_stale_env_control() {
  local p
  p="$(new_profile stale_env_control 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 120 DOORSTOP_INITIALIZED=TRUE DOORSTOP_DISABLE=TRUE SPIKE_KEEP_STALE=1
  if [[ "$(payload_starts "$p")" -eq 0 && "$(probe_lines "$p")" -eq 0 ]]; then
    pass "V3 control: stale guard blocks Doorstop when not unset"
  else
    fail "V3 control: starts=$(payload_starts "$p") lines=$(probe_lines "$p")"
  fi
}

sc_no_mods_dir() {
  local p
  p="$(new_profile no_mods_dir 0 "${BASE_MODS[@]}")"
  timed_run "$p" release 75
  ((TIMED_OUT == 1)) && pass "V16 game alive at 75 s" || fail "V16 game exited early"
  [[ -d "$p/Mods" ]] && pass "V16 Mods dir created" || fail "V16 Mods dir missing"
  grep -q ' created ' "$p/payload.log" && pass "V16 payload logged create" || fail "V16 no create log"
  if grep -q 'DirectoryNotFoundException' "$p/Player.log"; then fail "V16 DirectoryNotFoundException"; else pass "V16 no DirectoryNotFoundException"; fi
}

sc_no_mods_dir_control() {
  local p
  p="$(new_profile no_mods_dir_control 0 "${BASE_MODS[@]}")"
  timed_run "$p" spike 75 RWPM_SPIKE_NO_CREATE=1
  grep -q 'DirectoryNotFoundException' "$p/Player.log" &&
    pass "V16 control: missing dir throws without create" || fail "V16 control: no exception (create not shown necessary)"
}

# C5: only exercise V2 against the real Harmony + Prepatcher.
copy_prepatcher_deps() {
  local p="$1" h="$WORKSHOP/$HARMONY_WS" z="$WORKSHOP/$PREPATCHER_WS"
  grep -q 'Andreas Pardeike' "$h/About/About.xml" || return 1
  [[ -n "$(fd -t f '^0Harmony.dll$' "$h" | head -1)" ]] || return 1
  grep -qi 'zetrith' "$z/About/About.xml" || return 1
  cp -r "$h" "$p/Mods/$HARMONY_WS"
  cp -r "$z" "$p/Mods/$PREPATCHER_WS"
}

prepatcher_profile() {
  local p
  p="$(new_profile "$1" 1 brrainz.harmony "${BASE_MODS[@]}" zetrith.prepatcher rwpm.spike.probe)"
  copy_prepatcher_deps "$p" || return 1
  printf '%s\n' "$p"
}

sc_prepatcher() {
  local p patched
  p="$(prepatcher_profile prepatcher)" || {
    inconclusive "V2: Harmony/Prepatcher author or file check failed"
    return
  }
  timed_run "$p" release 300
  [[ "$(probe_lines "$p")" -ge 1 ]] || {
    fail "V2: probe never ran (timed_out=$TIMED_OUT)"
    return
  }
  assert_redirected "$p" "$(probe_lines "$p")"
  patched="$( (grep -oE 'patched Assembly-CSharp#-?[0-9]+' "$p/payload.log" || true) | sort -u | wc -l)"
  if ((patched >= 2)); then pass "V2 patched $patched GenFilePaths copies"; else inconclusive "V2 only $patched copy patched; Prepatcher reload not exercised"; fi
}

sc_prepatcher_control() {
  local p
  p="$(prepatcher_profile prepatcher_control)" || {
    inconclusive "V2 control: dep check failed"
    return
  }
  timed_run "$p" spike 200 RWPM_SPIKE_NO_RELOAD_HOOK=1
  if [[ "$(probe_lines "$p")" -eq 0 ]] && grep -q 'NO_RELOAD_HOOK set, skipped' "$p/payload.log"; then
    pass "V2 control: reload without re-patch loses redirect"
  else
    fail "V2 control: lines=$(probe_lines "$p") (re-patch not shown necessary)"
  fi
}

# C1: a file where the profile's Mods folder should be makes the payload's create fail.
sc_payload_fail() {
  local p
  p="$(new_profile payload_fail 0 "${BASE_MODS[@]}" rwpm.spike.probe)"
  touch "$p/Mods"
  timed_run "$p" release 60
  ((TIMED_OUT == 0)) || fail "C1 game still running at 60 s"
  ((LAUNCH_CODE != 0)) && pass "C1 rwpm non-zero exit ($LAUNCH_CODE)" || fail "C1 exit 0"
  grep -q 'FATAL' "$p/payload.log" && pass "C1 reason in payload log" || fail "C1 no FATAL in payload log"
  [[ "$(probe_lines "$p")" -eq 0 ]] && pass "C1 game never loaded mods" || fail "C1 probe ran"
}

# V24 (opt-in, consented C2): hide steam_appid.txt, rely on SteamAppId env.
hide_appid() {
  [[ -f "$APPID_FILE" ]] || return 1
  printf '%s\n' "$APPID_FILE" >"$APPID_MARKER"
  log "steam_appid.txt hidden; if interrupted: mv '$APPID_FILE.rwpm-spike' '$APPID_FILE'"
  mv "$APPID_FILE" "$APPID_FILE.rwpm-spike"
  trap restore_appid EXIT
}

sc_appid() {
  local p
  hide_appid || {
    inconclusive "V24: no steam_appid.txt to hide"
    return
  }
  p="$(new_profile appid 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 180
  [[ "$(probe_lines "$p")" -eq 1 ]] || {
    fail "V24: probe lines=$(probe_lines "$p") timed_out=$TIMED_OUT"
    return
  }
  [[ "$(probe_field "$p" steam 1)" == "True" ]] && pass "V24 Steam initialised without steam_appid.txt" || fail "V24 steam=$(probe_field "$p" steam 1)"
  if grep -q 'SteamAPI.Init() failed' "$p/Player.log"; then fail "V24 SteamAPI.Init failed"; else pass "V24 no Init failure"; fi
  assert_redirected "$p" 1
}

sc_appid_control() {
  local p
  local -a foreign
  p="$(new_profile appid_control 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 150 SPIKE_NO_APPID=1
  mapfile -t foreign < <(game_procs)
  if ((${#foreign[@]})); then
    pass "V24 control: Steam relaunched the game without our args"
    kill "${foreign[@]}" || true
  elif [[ "$(probe_field "$p" steam 1)" == "False" ]] || grep -q 'SteamAPI.Init() failed' "$p/Player.log"; then
    pass "V24 control: Steam init fails without SteamAppId"
  else
    fail "V24 control: steam=$(probe_field "$p" steam 1) (env not shown necessary)"
  fi
  restore_appid
  trap - EXIT
}

# V5 (T11): an active workshop mod that is not frozen. The guard's "Add to profile"
# freezes it before launch, so the game loads it from the profile, not the workshop.
installed_path() { probe_field "$1" installed "$2" | tr ',' '\n' | awk -F@ -v id="$3" '$1==id{print $2}'; }

guard_profile() {
  new_profile "$1" 1 brrainz.harmony "${BASE_MODS[@]}" rwpm.spike.probe
}

sc_guard() {
  local p
  p="$(guard_profile guard)"
  timed_run "$p" release 180 RWPM_GUARD_CHOICE=add
  [[ "$(probe_lines "$p")" -eq 1 ]] || {
    fail "V5 guard: probe lines=$(probe_lines "$p") timed_out=$TIMED_OUT"
    return
  }
  grep -q 'player chose Add' "$p/rwpm.log" && pass "V5 guard asked, player added" || fail "V5 guard log: $(cat "$p/rwpm.log" 2>/dev/null)"
  [[ -d "$p/Mods/$HARMONY_WS" ]] && pass "V5 Harmony frozen into profile" || fail "V5 Harmony not copied"
  probe_field "$p" active 1 | tr ',' '\n' | grep -qx brrainz.harmony && pass "V5 Harmony active" || fail "V5 active=$(probe_field "$p" active 1)"
  [[ "$(installed_path "$p" 1 brrainz.harmony)" == "$p/Mods/$HARMONY_WS" ]] &&
    pass "V5 Harmony loads from the profile" || fail "V5 Harmony from $(installed_path "$p" 1 brrainz.harmony)"
  assert_rwpm_waited guard "$p"
}

sc_guard_control() {
  local p
  p="$(guard_profile guard_control)"
  timed_run "$p" release 180 RWPM_GUARD_CHOICE=launch
  if [[ "$(installed_path "$p" 1 brrainz.harmony)" == "$WORKSHOP/$HARMONY_WS" ]]; then
    pass "V5 control: without the fix, Harmony loads live from the workshop"
  else
    fail "V5 control: Harmony from '$(installed_path "$p" 1 brrainz.harmony)' (guard not shown necessary)"
  fi
}

# ?U2: a helper the game starts (xdg-open for "Open folder") inherits the Doorstop
# preload. A fake xdg-open first on PATH reports whether its `$(…)` still works.
sc_openurl() {
  local p line
  p="$(new_profile openurl 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  touch "$p/openurl.flag"
  timed_run "$p" release 180 PATH="$ROOT/spike/linux/fake-bin:$PATH"
  for _ in 1 2 3 4 5; do [[ -s "$p/xdg-open.log" ]] && break || sleep 1; done
  [[ -s "$p/xdg-open.log" ]] || {
    inconclusive "?U2: fake xdg-open never ran (game opens URLs another way)"
    return
  }
  line="$(head -1 "$p/xdg-open.log")"
  log "?U2 xdg-open: $line"
  [[ "$line" == *"args=$p"* ]] && pass "?U2 xdg-open got the folder" || fail "?U2 xdg-open args: $line"
  [[ "$line" == subst=ok* ]] && pass "V28 helper's command substitution works" ||
    fail "V28 command substitution broken in game-spawned helper"
  [[ "$line" == *"enabled=<unset>"* ]] && pass "V28 helper has no DOORSTOP_ENABLED" || fail "V28 helper env: $line"
}

# V28 control: payload keeps DOORSTOP_ENABLED → the helper's redirects break (its
# output leaks into the game's stdout instead of its log).
sc_openurl_control() {
  local p
  p="$(new_profile openurl_control 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  touch "$p/openurl.flag"
  timed_run "$p" spike 180 PATH="$ROOT/spike/linux/fake-bin:$PATH" RWPM_SPIKE_KEEP_ENABLED=1
  sleep 3
  if [[ ! -s "$p/xdg-open.log" ]] && grep -q 'subst=' "$p/launch.out"; then
    pass "V28 control: with Doorstop on, the helper's redirect leaked to the game's stdout"
  else
    fail "V28 control: log='$(cat "$p/xdg-open.log" 2>/dev/null)' (unset not shown necessary)"
  fi
}

# ?U2 fix feasibility: does Unity start with a foreign argv[0], and does the game's
# Restart (GenCommandLine.cs:45-47, argv[0]) see it?
sc_arg0() {
  local p
  p="$(new_profile arg0 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 180 SPIKE_ARG0=/nonexistent/rwpm-trampoline
  [[ "$(probe_lines "$p")" -eq 1 ]] || {
    fail "arg0: game did not reach the probe (lines=$(probe_lines "$p"))"
    return
  }
  [[ "$(probe_field "$p" arg0 1)" == /nonexistent/rwpm-trampoline ]] &&
    pass "arg0: game started and reports argv[0] as given" || fail "arg0=$(probe_field "$p" arg0 1)"
}

# T12: a reused foreign Doorstop's own target (stand-in assembly named Doorstop) is
# chain-loaded by our payload after the redirect, and the redirect still holds.
sc_chain() {
  local p
  p="$(new_profile chain 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 180 RWPM_CHAIN_TARGET="$BUILD/chain/Doorstop.dll"
  [[ -s "$p/chain.log" ]] && pass "T12 chain target ran" || fail "T12 chain target did not run ($(grep chain "$p/payload.log"))"
  [[ "$(probe_lines "$p")" -eq 1 ]] && assert_redirected "$p" 1 || fail "T12 chain: probe lines=$(probe_lines "$p")"
}

# T12: a broken chain target is logged and the game still starts, redirected.
sc_chain_fail() {
  local p
  p="$(new_profile chain_fail 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 180 RWPM_CHAIN_TARGET="$p/missing/Doorstop.dll"
  grep -q 'chain-load failed' "$p/payload.log" && pass "T12 chain failure logged" || fail "T12 no chain failure log"
  [[ "$(probe_lines "$p")" -eq 1 ]] && assert_redirected "$p" 1 || fail "T12 chain_fail: probe lines=$(probe_lines "$p")"
}
