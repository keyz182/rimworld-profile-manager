# Spike — macOS stored game builds

Mirrors `spike/linux-builds/` on macOS. Evidence for the pinned-build checks and the
`macos_copy_sign` question, plus the build-identity rules where they touch capture.

Host these results came from: macOS 26.6.2 arm64, APFS, Steam RimWorld **1.6.4871 rev595**,
buildid 23969874, public branch. Only 1.6 runs on macOS; 1.7 (Waylith) is out of scope.

## Scripts

| script | what it does |
|---|---|
| `capture.sh <install> <builds> [--pristine] [--ditto]` | Copies the install as a stored build. APFS clone by default (`cp -cR`), `ditto` with `--ditto`. Refuses unless `StateFlags == 4`. Keys the already-captured test on **buildid**, not the version string. Asserts the copy's `CFBundleExecutable` resolves. |
| `check.sh <profile> <build>` | Evidence from the logs and the live process. |
| `swap.sh` | Steam launch-options stand-in: rewrites the `%command%` target to the build, then `rwpm run`. |
| `translocation.sh <build> <profile>` | Launches a quarantined build by **both** routes and records the process path for each. **Route (b) raises a Gatekeeper dialog on the user's screen — run once, deliberately.** |
| `watch-acf.sh <acf> <log>` | Logs appmanifest state while Steam switches branch. |

## Platform differences from the Linux spike

These are the things that do not carry across; each cost a real measurement.

1. **`Version.txt` is inside the bundle** — `<install>/RimWorldMac.app/Version.txt`, not the
   install root, where Linux keeps it. Content is CRLF (`1.6.4871 rev595\r\n`), as the Linux spike recorded
   elsewhere. `discovery.rs:read_game_dir` reads the install root, so it finds nothing on macOS.
2. **No `/proc`.** Process identity and loaded images come from `lsof -p <pid> -Fn`. The `-Fn`
   form is required: the binary is `RimWorld by Ludeon Studios` and Steam lives under
   `Application Support`, so splitting lsof's default columns on whitespace truncates both paths.
3. **No wrapper chain.** macOS `%command%` is a bare argv naming the `.app` directory —
   no `start_RimWorld.sh`, no soldier/reaper. `swap.sh` rewrites the bundle token instead.
4. **BSD `date`** has no `-Is`; `watch-acf.sh` uses `+%Y-%m-%dT%H:%M:%S%z`.
5. **No workshop log signal.** The Linux check asserts on workshop lines in `Player.log`. macOS
   logs none — not even for the Steam install launched by Steam with 524 workshop items present —
   so that check is informational here and the in-game mod list is the real evidence.
7. **The target swap cannot be a path-prefix rewrite.** 1.7 ships `RimWorld.app` (Waylith) where
   1.6 ships `RimWorldMac.app`, so rewriting `<install>/X` to `<build>/X` yields a path that does
   not exist. `swap.sh` resolves the build's own bundle instead; a per-OS swap must do the same.
6. **`steam_appid.txt` sits in three places** (install root, bundle root, `Contents/MacOS/`);
   all three are removed at capture so Steam init is proved on the `SteamAppId` env var alone.

## Results summary

| question | result |
|---|---|
| Q1 capture | APFS clone: 15 s, ~11 MiB real for a 948 MiB build. `ditto`: 42 s, 993 MiB. Clone wins; `reflink_or_copy` is right on macOS too. |
| Q2 signature | Copy keeps the ad-hoc signature exactly. `codesign -v` fails identically on source and copy — **Ludeon ships the bundle seal-invalid** (`Contents/PlugIns/` sealed vs `Contents/Plugins/` installed). The copy starts. Do **not** re-sign captured builds. |
| Q3 Gatekeeper | Game code carries no quarantine; copying adds none. Quarantine under the install is only in `Mods/` and `bak/`. |
| Q3b translocation | Quarantined build: `rwpm` direct exec runs in place. Via `open`, a translocated path was measured **and** macOS then refused the app outright ("RimWorldMac is damaged and can't be opened") — the broken seal turns the silent relocation into a hard refusal. Route-dependent either way; direct exec is unaffected. |
| Q4 | `Mono path[0]` and the live exe both point into the build, with a real version gap (build 1.6.4871 vs install 1.7.4976). |
| Q5/Q6 | **Pass.** With Steam on 1.7 and a stored 1.6 build, Steam Play runs the stored build (`started_by = "steam"`), Steam init succeeds, and `gameoverlayrenderer.dylib` **is** loaded — macOS does not reproduce the Windows overlay loss. Shift-Tab draw still unconfirmed by eye. |
| Q7 OFFLINE | Doorstop loads in the copy; mods and save dir redirect to the profile. |

Full detail, including the open decisions, is in the spike results write-up that accompanies this
branch.
