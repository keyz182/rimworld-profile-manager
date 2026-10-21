# QA runbook: open `.rimpack` files from the file manager

Check that a double-click on a `.rimpack` file opens this app on the "Open a modpack" question, and that uninstalling removes the link. This is the same check as the `.rimrim` association runbook (`rimrim-association.md`), for the second file type.

- **Platform:** Windows 10 or 11 (installer), and Linux (tar.gz and AppImage). macOS is out of scope.
- **Time:** about 20 minutes.
- **Needs:** one `.rimpack` file made with "Share as modpack…" (see `rimpack.md`).
- **Not covered by the automated tests:** the Windows steps and every double-click. The tests check the files the installers write, run the Linux script against a scratch home folder, and check that a `.rimpack` argument reaches the Open dialog. The tests do not compile the Windows installer. CI does, and it was also compiled once by hand with warnings as errors.

## What this does to your computer

- Windows: the installer writes more keys under `HKCU\Software\Classes` (`.rimpack` and `rwpm.modpack`). It does not need admin rights.
- Linux tar.gz: the script writes the same files as for the `.rimrim` association. The type file now holds both types.
- Rollback is in the last section.

## Windows (installer)

1. Uninstall any older version of the app. Close the app.
2. Run the new `…-windows-x86_64-setup.exe`. Finish the installer.
3. Open Registry Editor. Expected: `HKCU\Software\Classes\.rimpack` has the default value `rwpm.modpack`. `HKCU\Software\Classes\rwpm.modpack\shell\open\command` is `"<install folder>\rwpm.exe" "%1"`. The `.rimrim` keys are still there.
4. In Explorer, find the `.rimpack` file. Expected: it shows the app icon and the type "RimWorld Profile Manager modpack".
5. Double-click the file. Expected: the app opens and the "Open a modpack" question shows for that file. No console window opens.
6. Put the file in a folder whose name has a space and a `&`. Double-click it. Expected: the question still shows.
7. Uninstall the app from Settings. Expected: the `.rimpack` and `rwpm.modpack` keys are gone, and so are the `.rimrim` ones.
8. Install again. In Registry Editor set `HKCU\Software\Classes\.rimpack` to `other.app`. Uninstall. Expected: `.rimpack` still reads `other.app`, and `rwpm.modpack` is gone.

## Linux, tar.gz

1. Unpack the bundle. Run `./install-desktop-entry.sh`. Expected: it says that backups (`.rimrim`) and modpacks (`.rimpack`) open in the app.
2. Run `xdg-mime query filetype pack.rimpack`. Expected: `application/x-rimpack`. Run `xdg-mime query default application/x-rimpack`. Expected: `rwpm.desktop`.
3. Double-click the file in your file manager. Expected: the app opens on the "Open a modpack" question.
4. Double-click a `.rimrim` file. Expected: the app opens on the Restore question, as before.
5. Run `./install-desktop-entry.sh --remove`. Expected: both `xdg-mime query default` commands print nothing.

## Linux, AppImage

1. Add the AppImage with your AppImage tool.
2. Expected: the tool lists `.rimrim` and `.rimpack` for the app. If your tool does not read the types, note it in the QA result. "Open modpack…" still works.

## Rollback

1. Windows: uninstall the app. Delete the test files.
2. Linux: run `./install-desktop-entry.sh --remove`. Delete the test files and any AppImage entry.
3. Delete any profiles made by the checks.
