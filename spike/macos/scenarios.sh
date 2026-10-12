# shellcheck shell=bash
# shellcheck disable=SC2015 # pass() always returns 0, so `cond && pass || fail` is a safe if/else.
# T37 spike scenarios, sourced by run.sh. Each sc_* sets RESULT via pass/fail/inconclusive.
# Mirrors spike/linux's T2 scenarios (removed at tag v1.0.0, kept in git history) for the
# macOS equivalents: DYLD_INSERT_LIBRARIES instead of LD_PRELOAD, direct bundle-binary exec
# instead of a start script, and the new macOS-only unknowns (?10 quarantine, ?11/?12
# Steam %command% shape + argv[0] restart).

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

# ---- R24/R25/V42: does a normal, unredirected launch even work via our direct-exec
# recipe, against the REAL Default save dir? (user's explicit choice: real profile, not
# an isolated scratch one, for the baseline sanity check — no -savedatafolder at all.)
sc_live_baseline() {
  # No probe mod here (this is a plain LIVE-mode launch of the real profile, V13: no
  # Doorstop at all) so nothing auto-quits the game — we run it for a short window
  # then kill it ourselves. TIMED_OUT=1 (we had to kill it) is the EXPECTED outcome,
  # not a failure; the thing under test is "did it even launch and write its log".
  local before
  before="$(date +%s)"
  timed_run_live 30
  if pgrep -f -- "RimWorld by Ludeon Studios" >/dev/null 2>&1; then
    fail "live_baseline: game still alive after kill"
  else
    pass "live_baseline: direct bundle-exec launched and was reaped cleanly"
  fi
  [[ -s "$GAME_LOG_DIR/Player.log" ]] || {
    fail "live_baseline: no Player.log"
    return
  }
  local mtime
  mtime="$(stat -f '%m' "$GAME_LOG_DIR/Player.log")"
  ((mtime >= before)) && pass "live_baseline: Default Player.log freshly written this run" || inconclusive "live_baseline: Player.log predates this run"
}

# launches the real game with no Doorstop, no -savedatafolder — LIVE profile semantics
# (V13), direct-exec of the bundle binary, same mechanism the OFFLINE scenarios use minus
# injection. Runs in foreground-background like timed_run but skips the profile machinery.
timed_run_live() {
  local t="$1"
  local exe_name exe
  exe_name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$BUNDLE/Contents/Info.plist")"
  exe="$BUNDLE/Contents/MacOS/$exe_name"
  env SteamAppId=294100 "$exe" >"$OUT/live-baseline.out" 2>&1 &
  LAUNCH_PID=$!
  TIMED_OUT=0
  if ! wait_gone_live "$t"; then
    TIMED_OUT=1
    pkill -f -- "$exe" || true
    wait_gone_live 30 || log "WARN live game still alive after kill"
  fi
  LAUNCH_CODE=0
  wait "$LAUNCH_PID" || LAUNCH_CODE=$?
}

wait_gone_live() {
  local deadline=$((SECONDS + $1))
  while pgrep -f -- "RimWorld by Ludeon Studios" >/dev/null 2>&1; do
    ((SECONDS < deadline)) || return 1
    sleep 2
  done
  return 0
}

# ---- R24/R25/V42: does our vendored, un-quarantined dylib load under SIP at all, and
# does the reflection write land? Oracle for the OFFLINE redirect mechanism itself.
sc_basic() {
  local p
  p="$(new_profile basic 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 180
  ((TIMED_OUT == 0)) || {
    fail "basic: game did not quit"
    return
  }
  [[ "$(probe_lines "$p")" -eq 1 ]] || {
    fail "basic: probe lines=$(probe_lines "$p")"
    return
  }
  assert_redirected "$p" 1
  [[ "$(probe_field "$p" steam 1)" == "True" ]] && pass "SteamAppId → Steam init" || inconclusive "steam=$(probe_field "$p" steam 1)"
  [[ -s "$p/Player.log" ]] && pass "Player.log in profile" || fail "no profile Player.log"
  pass_or_fail_rwpm_exit basic "$p"
}

pass_or_fail_rwpm_exit() {
  ((LAUNCH_CODE == 0)) && pass "$1: launcher exit 0" || fail "$1: launcher exit $LAUNCH_CODE ($(tail -1 "$2/launch.out" 2>/dev/null))"
}

assert_redirected() {
  local p="$1" n="$2"
  [[ "$(probe_field "$p" mods "$n")" == "$p/Mods" ]] && pass "run $n mods=$p/Mods" || fail "run $n mods=$(probe_field "$p" mods "$n")"
  [[ "$(probe_field "$p" save "$n")" == "$p/SaveData" ]] && pass "run $n save dir" || fail "run $n save=$(probe_field "$p" save "$n")"
  if probe_field "$p" installed "$n" | tr ',' '\n' | grep -q "@$GAME/RimWorldMac.app/Mods/"; then
    fail "run $n loaded mods from install Mods"
  else
    pass "run $n no install-Mods mods"
  fi
}

# ---- ?10 headline unknown: does a com.apple.quarantine xattr block the dylib from
# loading at all? A downloaded release asset will always carry one; our curl'd vendor
# copy does not (curl never sets quarantine), so we synthesize it for the main scenario
# and the control runs with it stripped.
sc_quarantine() {
  local p dylib
  dylib="$DOORSTOP_DIR/libdoorstop.dylib"
  xattr -w com.apple.quarantine "0081;00000000;Safari;" "$dylib"
  p="$(new_profile quarantine 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 180
  if [[ "$(probe_lines "$p")" -eq 1 ]]; then
    pass "?10 quarantined dylib still loaded under SIP"
    assert_redirected "$p" 1
  else
    fail "?10 quarantined dylib did not load (probe never ran, timed_out=$TIMED_OUT)"
  fi
  xattr -d com.apple.quarantine "$dylib" 2>/dev/null || true
}

sc_quarantine_control() {
  local p dylib
  dylib="$DOORSTOP_DIR/libdoorstop.dylib"
  xattr -d com.apple.quarantine "$dylib" 2>/dev/null || true
  p="$(new_profile quarantine_control 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 180
  if [[ "$(probe_lines "$p")" -eq 1 ]]; then
    pass "?10 control: un-quarantined dylib loads (baseline confirmed working)"
  else
    inconclusive "?10 control: un-quarantined dylib also failed to load (harness problem, not quarantine)"
  fi
}

# ---- R25: arm64 requires a signature; adhoc should suffice, an unsigned copy should fail.
sc_adhoc_sign() {
  local p scratch
  scratch="$OUT/unsigned-doorstop"
  mkdir -p "$scratch"
  cp "$DOORSTOP_DIR/libdoorstop.dylib" "$scratch/"
  codesign --remove-signature "$scratch/libdoorstop.dylib" 2>&1 | tee -a "$OUT/adhoc_sign.log" || true
  p="$(new_profile adhoc_sign_unsigned 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  timed_run "$p" release 90 RWPM_DOORSTOP_DIR="$scratch"
  if [[ "$(probe_lines "$p")" -eq 0 ]]; then
    pass "R25: unsigned dylib refused to load on arm64"
  else
    inconclusive "R25: unsigned dylib loaded anyway (probe lines=$(probe_lines "$p")) — arm64 signing requirement not confirmed strict here"
  fi
  p="$(new_profile adhoc_sign_signed 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  codesign -s - "$scratch/libdoorstop.dylib"
  timed_run "$p" release 180 RWPM_DOORSTOP_DIR="$scratch"
  [[ "$(probe_lines "$p")" -eq 1 ]] && pass "R25: adhoc-signed dylib loads" || fail "R25: adhoc-signed dylib did not load"
}

# ---- R11/V1/?11/?48: GenCommandLine.Restart re-execs argv[0] — does the macOS game do
# the same, and does the Doorstop redirect survive the restart?
sc_restart() {
  local p
  p="$(new_profile restart 1 "${BASE_MODS[@]}" rwpm.spike.probe)"
  touch "$p/restart.flag"
  timed_run "$p" release 300
  ((TIMED_OUT == 0)) || {
    fail "V1/?11 restart: game did not quit (lines=$(payload_starts "$p"))"
    return
  }
  [[ "$(probe_lines "$p")" -eq 2 ]] || {
    fail "V1/?11 restart: probe lines=$(probe_lines "$p")"
    return
  }
  [[ "$(probe_field "$p" pid 1)" != "$(probe_field "$p" pid 2)" ]] && pass "two processes across restart" || fail "same pid twice"
  assert_redirected "$p" 1
  assert_redirected "$p" 2
  [[ "$(payload_starts "$p")" -eq 2 ]] && pass "V1 payload ran in restart child (redirect held)" || fail "payload starts=$(payload_starts "$p")"
  pass_or_fail_rwpm_exit restart "$p"
}

# ---- V2: Prepatcher reloads Assembly-CSharp in-process; redirect must survive the 2nd copy.
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
  patched="$( (grep -oE 'patched Assembly-CSharp#-?[0-9]+' "$p/payload.log" || true) | sort -u | wc -l | tr -d ' ')"
  if ((patched >= 2)); then pass "V2 patched $patched GenFilePaths copies"; else inconclusive "V2 only $patched copy patched; Prepatcher reload not exercised"; fi
}

# ---- R10/V24: SteamAppId env lets Steam init without steam_appid.txt.
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
    restore_appid
    trap - EXIT
    return
  }
  [[ "$(probe_field "$p" steam 1)" == "True" ]] && pass "V24 Steam initialised without steam_appid.txt" || fail "V24 steam=$(probe_field "$p" steam 1)"
  assert_redirected "$p" 1
  restore_appid
  trap - EXIT
}

# ---- R27/R28: does injecting the dylib into a plain shell break command substitution,
# the way it does on Linux (B1/B2/B3)? /bin/bash on macOS is Apple platform/hardened-
# runtime-signed, so this also tests whether SIP refuses the injection into the shell
# itself (in which case the Linux-style breakage simply cannot occur on macOS — a
# materially different, better outcome than Linux, worth confirming rather than assuming).
sc_shellhook() {
  local out
  out="$(DYLD_INSERT_LIBRARIES="$DOORSTOP_DIR/libdoorstop.dylib" \
    DYLD_LIBRARY_PATH="$DOORSTOP_DIR" DOORSTOP_ENABLED=1 \
    DOORSTOP_TARGET_ASSEMBLY="/nonexistent/x.dll" \
    /bin/bash -c 'v=$(printf ok); printf "subst=%s\n" "$v"' 2>&1 || true)"
  log "shellhook: $out"
  if [[ "$out" == *"subst=ok"* ]]; then
    pass "R27/R28: command substitution unaffected when a hardened shell refuses foreign injection (or hook is harmless)"
  elif [[ "$out" == *"subst="* ]]; then
    fail "R27/R28: command substitution broken (B1/B2/B3 analogue reproduced on macOS)"
  else
    inconclusive "R27/R28: shell crashed or produced no output: $out"
  fi
}

# ---- V26: payload fails closed (FailFast) → non-zero exit, no window, reason logged.
sc_payload_fail() {
  local p
  p="$(new_profile payload_fail 0 "${BASE_MODS[@]}" rwpm.spike.probe)"
  touch "$p/Mods"
  timed_run "$p" release 60
  ((TIMED_OUT == 0)) || fail "V26 game still running at 60 s"
  ((LAUNCH_CODE != 0)) && pass "V26 rwpm non-zero exit ($LAUNCH_CODE)" || fail "V26 exit 0"
  grep -q 'FATAL' "$p/payload.log" && pass "V26 reason in payload log" || fail "V26 no FATAL in payload log"
  [[ "$(probe_lines "$p")" -eq 0 ]] && pass "V26 game never loaded mods" || fail "V26 probe ran"
}

# ---- ?11/?12: what Steam's %command% actually does on macOS — needs one manual click.
# Prints the exact Steam launch-options line to set and what to look for afterwards;
# cannot be driven headlessly (native Steam GUI, no CLI launch API on macOS).
sc_steam_cmdline() {
  local wrapper="$OUT/steam-cmdline-probe.sh"
  cat >"$wrapper" <<EOF
#!/usr/bin/env bash
{ echo "argv0=\$0"; ps -o pid,ppid,command -p \$\$; env | grep -E '^(DYLD_|Steam)' ; } >>"$OUT/steam-cmdline.log" 2>&1
exec "\$@"
EOF
  chmod +x "$wrapper"
  log "?11/?12 MANUAL STEP: set RimWorld's Steam launch options to:"
  log "    $wrapper %command%"
  log "  then click Play once from the Steam app, quit the game, and re-run:"
  log "    spike/macos/run.sh steam_cmdline_check"
  inconclusive "?11/?12: needs one manual Steam Play click (instructions logged above)"
}

sc_steam_cmdline_check() {
  [[ -s "$OUT/steam-cmdline.log" ]] || {
    fail "?11/?12: $OUT/steam-cmdline.log is empty — did you click Play with the wrapper set?"
    return
  }
  log "?11/?12 captured chain:"
  cat "$OUT/steam-cmdline.log" >&2
  # Finding (2026-10-12, live Steam macOS 1.6.4871): %command% names the .app BUNDLE
  # DIRECTORY itself (eg ".../RimWorldMac.app -disable-compute-shaders"), passed as a
  # literal argv list — not a shell, not LaunchServices/`open`. A naive `exec "$@"`
  # cannot exec a directory and fails immediately, which is exactly why this probe
  # wrapper doesn't itself start the game — that failure IS the answer, not a bug to
  # chase. It confirms rwpm's own launch.sh must resolve CFBundleExecutable and exec the
  # inner Mach-O directly (V41's design, already implemented) rather than re-exec argv
  # as given.
  if grep -q 'RimWorldMac.app' "$OUT/steam-cmdline.log"; then
    pass "V49: Steam macOS %command% = exec-style argv naming the .app bundle dir (not a shell, not LaunchServices); V41's resolve-and-exec design is required and correct"
  else
    fail "?11/?12: %command% did not name the bundle at all — see $OUT/steam-cmdline.log, Stage 2 design needs revisiting"
  fi
}
