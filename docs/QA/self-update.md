# QA runbook: update the app from the app

Check that "Install update" and "Restart now" replace the app for each way of installing it, and that a package-managed copy only shows a notice.

- **Platform:** Linux (AppImage, tar.gz, and a package under `/opt/rwpm`) and Windows 10 or 11 (installer and zip). macOS only needs the notice check.
- **Time:** about 45 minutes.
- **Needs:** two releases of the app, an old one and a newer one that has a `SHA256SUMS` file. A local web server can stand in for GitHub (see "Test without a release" below).
- **Not covered by the automated tests:** every Windows step, and the real download from GitHub. The tests check the checksum rules, the archive unpacking, the undo after a failed step, the buttons, and the "update did not install" notice. They do not run the Windows installer. CI compiles the installer with warnings as errors, but nobody has run its wait and relaunch steps.

## What this does to your computer

- It writes the new version over the old one. The AppImage file, the folder, or the installed files change.
- It writes `.rwpm-update` in the install folder while it works. The app removes it when it ends and on the next start.
- Profiles, saves and settings are not touched.
- Rollback is in the last section.

## Before you start

1. Install the OLD version. Choose "Settings", set "Check for updates" to "Daily", then choose "Save".
2. Wait for the line "Version X is out." If it does not show, choose "Check now".

## Package under `/opt/rwpm` (AUR, deb, rpm)

1. Look at the line at the top. Expected: it shows "Update it the way you installed it." There is no "Install update" button. "Release page" still works.
2. Open "Settings". Expected: the text under "Check for updates" says the copy belongs to the package manager.

## AppImage

1. Put the old AppImage in a folder you can write to. Start it.
2. Choose "Install update". Expected: the button shows "Downloading…", then "Restart now" shows. The `.AppImage` file has not changed yet.
3. Start RimWorld from a profile. Expected: "Restart now" is off. Close the game. Expected: it turns on again.
4. Choose "Restart now". Expected: this window closes and the new version opens. The version next to the title is the new one. The `.AppImage` file has the new size.
5. Check that no `*.rwpm-new` file is left next to the AppImage.
6. Make the folder read-only (`chmod a-w`). Start the AppImage. Expected: no "Install update" button. Put the permission back.

## Linux, tar.gz

1. Unpack the old tar.gz into a folder you own. Put a file `notes.txt` in it. Start `./rwpm`.
2. Choose "Install update", then "Restart now". Expected: the new version opens. The folder has the new files. `notes.txt` is still there.
3. Check that `.rwpm-update` and `*.old` files are gone after the restart.
4. Unpack the old version again. Make `payload` read-only and not writable by you (`chmod a-w payload`). Choose "Install update", then "Restart now". Expected: the app says it could not replace a file, and the old version still works. Every file has its old content.

## Windows, installer

1. Install the old version with the installer. Start the app.
2. Choose "Install update". Expected: "Downloading…", then "Restart now".
3. Choose "Restart now". Expected: the window closes. After a few seconds the new version opens by itself. No installer window shows.
4. Open "Add or remove programs". Expected: one entry, with the new version.
5. Repeat steps 1 and 2. Start RimWorld and keep it open. Expected: "Restart now" is off. Close the game and continue.
6. To test the give-up path: with the app at "Restart now", open a file in the install folder so it stays locked (for example, hold `payload\Rwpm.Payload.dll` open in a script). Choose "Restart now". Expected: after about 30 seconds the old version opens again. It shows "The update to version X did not install…". The notice does not show on the next start.

## Windows, zip

1. Unzip the old version into a folder you own. Add `notes.txt`. Start `rwpm.exe`.
2. Choose "Install update", then "Restart now". Expected: the new version opens. `notes.txt` is still there.
3. Close the app and start it again. Expected: no `.rwpm-update` folder. The `*.old` files are gone. A file that Windows kept locked can stay until the next start after this one.

## Test without a release

1. Make a folder with the files of the newer release and a `SHA256SUMS` made with `sha256sum`.
2. Serve it with `python3 -m http.server`.
3. Build a copy of the app where `LATEST_URL` and `DOWNLOAD_PREFIX` (in `crates/rwpm-core/src/update_check.rs`) point at that server and the HTTPS-only check is off. Do not ship that build.
4. Change one byte of the AppImage and keep the old `SHA256SUMS`. Choose "Install update". Expected: the app says the checksum does not match. No `*.rwpm-new` file is left.

## Rollback

- AppImage and zip or tar.gz: put the old file or folder back from your copy.
- Windows installer: run the old installer.
- Delete any leftover `.rwpm-update` folder.
