# T57 QA runbook: RimSort export for a pinned profile

Check that "Add to RimSort" points RimSort at the profile's stored game version, not at the Steam install.

- **Platform:** Linux. The paths below are for native Steam.
- **Flatpak Steam:** the app data is in `~/.var/app/com.valvesoftware.Steam/data/RimWorld Profile Manager/`, not in `~/.local/share/RimWorld Profile Manager/`. Steam's game and workshop folders are also inside `~/.var/app/com.valvesoftware.Steam/`. For the exact folders, use the `STEAM_GAME=`, `WORKSHOP=` and `BUILD_DIR=` lines that the setup helper prints. RimSort's own `settings.json` path does not change.
- **Time:** about 20 minutes, plus the copy time.
- **Scope:** the export, and RimSort reading the instance. Starting the game from RimSort is not in scope, because the pinned launch is task T55.

## What the setup does to your disk

The setup helper stores a full copy of your Steam RimWorld folder in the app's profiles folder.

- **Disk space needed:** the size of your RimWorld folder plus 1 GB of margin. A 1.6.4871 install measured 1.8 GB, so plan for 3 GB free.
- **Where the copy goes:** `~/.local/share/RimWorld Profile Manager/profiles/builds/<key>/`. If you moved the profiles folder in Settings, the copy goes under that folder.
- **Profile it adds:** "T57 Pinned".
- The copy stays on disk until you remove it (see "Rollback").

## Before you start

1. Build and install the app from the branch under test.
2. Close RimWorld, RimSort and the app.
3. Check free space. Run `du -sh ~/.local/share/Steam/steamapps/common/RimWorld` and `df -h ~/.local/share`. Make sure that the free space is more than the RimWorld size plus 1 GB.
4. Back up RimSort's settings: `cp ~/.local/share/RimSort/settings.json ~/t57-rimsort-settings.json.bak`.

## Setup: store a build and pin a profile

This setup uses a test helper that runs the real capture code. It does not use the app's "Capture current game" button, so the checks below do not depend on the screen.

1. In the repository, run:
   `RWPM_T57_E2E=1 cargo test -p rwpm-core t57_e2e_capture_and_pin -- --ignored --nocapture`
2. Write down the `STEAM_GAME=`, `WORKSHOP=`, `KEY=`, `BUILD_DIR=` and `PROFILE=` lines from the output.
3. Expected: the test passes, and `BUILD_DIR` exists and holds a RimWorld game folder.

If this game version is already stored, the helper stops with `REFUSED: a build <key> is already stored`. It does not pin or change that build, because the build is yours. Do not continue on this machine. Use a machine where this game version is not stored.

Run the helper one time only. Each run adds one more "T57 Pinned" profile. To start again, do the rollback first.

## Steps

| # | Action | Expected result |
|---|--------|-----------------|
| 1 | Put a marker mod in the build: `mkdir -p "<BUILD_DIR>/Mods/T57Marker/About"`. Copy the `About.xml` of any mod into it, and set `<name>` to `T57 Marker`. | The folder exists. |
| 2 | Start the app. Select "T57 Pinned". Click "Add to RimSort". If the app asks for permission, allow it. | Notice: the profile was added to RimSort. A backup file is in `~/.local/share/RimWorld Profile Manager/backups/rimsort/`. |
| 3 | Open `~/.local/share/RimSort/settings.json`. Find the instance "T57 Pinned". | `game_folder` = `<BUILD_DIR>`. `local_folder` = `<BUILD_DIR>/Mods`. `workshop_folder` = `~/.local/share/Steam/steamapps/workshop/content/294100`. |
| 4 | Close the app. Start RimSort. Select the instance "T57 Pinned". | RimSort shows no path error. "T57 Marker" is in the local mods list. Workshop mods are listed. The game version is the version in `KEY`. |
| 5 | Close RimSort. In the app, select an unpinned LIVE profile. Click "Add to RimSort". | In `settings.json`, that instance has `local_folder` = `~/.local/share/Steam/steamapps/common/RimWorld/Mods`. |
| 6 | Close the app. Rename `<BUILD_DIR>` to `<BUILD_DIR>.away`. Run `sha256sum ~/.local/share/RimSort/settings.json`. | You have a checksum. |
| 7 | Start the app. Select "T57 Pinned". Click "Add to RimSort". | Error notice: "This profile plays the stored game version “<KEY>”, which is gone or cannot be read. Choose another game version for it under Game version." No job starts. |
| 8 | Run `sha256sum ~/.local/share/RimSort/settings.json` again. Rename `<BUILD_DIR>.away` back to `<BUILD_DIR>`. | The checksum is the same as in step 6. |

Pass: all expected results are true. Record the date, the app version and `KEY` in the PR or in task T68.

## Rollback

Do these steps in this order. The profile uses the build, so remove the profile first.

1. Close RimSort, RimWorld and the app.
2. Restore RimSort's settings: `cp ~/t57-rimsort-settings.json.bak ~/.local/share/RimSort/settings.json`.
3. Start the app. Delete the profile "T57 Pinned". Close the app.
4. Remove the build only if the helper printed `BUILD_DIR=` in your setup run. The helper refuses when the version was already stored, so a printed `BUILD_DIR` is always a build that it made. Run `trash "<BUILD_DIR>"`. If you renamed it in step 6, remove `<BUILD_DIR>.away` instead. If the helper stopped with `REFUSED`, do not remove anything in `builds/`.
5. If you removed the build, check: `ls ~/".local/share/RimWorld Profile Manager/profiles/builds/"` does not show `<KEY>`.
6. Remove the backup copy: `trash ~/t57-rimsort-settings.json.bak`.
