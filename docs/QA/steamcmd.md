# Restore a modpack with steamcmd (manual QA)

Run this on a machine where Steam is closed or not installed, so the Steam route is not offered. Use a test pack of 2 or 3 small mods.

## Repro

1. Close Steam. Start the app. Choose "Open modpack…" and pick the pack.
2. Check: the dialog says "Steam cannot be reached here…" and has "Restore and download…".
3. Choose it. Check: a dialog "Download steamcmd" lists four points. Choose Cancel. Check: no profile is made and `consent.toml` has no `steamcmd` entry.
4. Choose "Restore and download…" again, then "Download steamcmd and continue". Check: the list of mod names and the warning show. Choose "Download".
5. Wait. Check: the progress bar moves, and Cancel is on the footer.
6. Check: a new OFFLINE profile exists, its `Mods` folder holds each mod, and `<app data>/steamcmd/install/steamapps/workshop/content/294100` is empty.
7. Check: `settings.toml` has `steamcmd_sha256`. Open the same pack again and restore it under another name. Check: no consent question, and the same fingerprint.

## Expected results

- steamcmd is in `<app data>/steamcmd/` only. Nothing is written under the real Steam folders.
- A mod that fails leaves the profile LIVE and the notice lists it with a reason.
- Cancel during the download stops steamcmd. `pgrep -f steamcmd` shows nothing a few seconds later.

## Changed file

Edit `steamcmd_sha256` in `settings.toml` to another 64-digit hex value, delete `<app data>/steamcmd/`, and restore a pack again. Expect: the dialog "steamcmd file changed" with both fingerprints, and no new profile. "Trust the new file" downloads again and records the new fingerprint.

## Rollback

Delete `<app data>/steamcmd/`, remove the `steamcmd` entry from `consent.toml`, remove `steamcmd_sha256` from `settings.toml`, and delete the test profile.

## Not yet run

Windows and macOS. Linux without the 32-bit libraries, to see the plain-words message. A Mac without Rosetta is parked: the consent dialog says Rosetta 2 is needed, and the failure message is covered by a test.
